// devws's link with an opencode agent, loaded by agent_runner.sh through
// OPENCODE_CONFIG_CONTENT (the user's own opencode config stays as it is).
// opencode has no command hooks as Claude Code and Codex do: this plugin
// sends devws the same hook payloads they would.
//
//   - the agent's state (agent_state.sh): working, waiting on a permission,
//     done, and its conversation (<db>#<session>, for its usage);
//   - a provider error (session.error) as a structured startup error on the
//     pane (@devws_startup_error), which devAgentics reads to fall back to
//     another program when it is about credit, limits or the model;
//   - the files it edits, in the linked editor (agent_follow_hook.sh).
//
// Every call is best effort and in order: a hook that fails never stops the
// agent.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// devws's scripts, two folders up from this module's (agents/30-opencode).
const scripts = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const database = process.env.DEVWS_OPENCODE_DB ?? "";
const editTools = new Set(["edit", "write", "patch", "multiedit", "apply_patch"]);

function run(command, args, input) {
  return new Promise((resolve) => {
    try {
      const child = spawn(command, args, { stdio: ["pipe", "ignore", "ignore"] });
      child.on("error", () => resolve());
      child.on("close", () => resolve());
      child.stdin.on("error", () => {});
      child.stdin.end(input ?? "");
    } catch {
      resolve();
    }
  });
}

// devws's own startup error codes (worker.rs maps them to a fallback scope).
// Not exported: opencode runs every function a plugin module exports.
function errorCode(error) {
  const status = error?.data?.statusCode;
  const text = `${error?.name ?? ""} ${error?.data?.message ?? ""}`.toLowerCase();
  if (status === 402 || /insufficient|balance|credit|billing|payment/.test(text)) return "insufficient_quota";
  if (status === 429 || /rate.?limit/.test(text)) return "rate_limit";
  if (status === 404 || /model.*not.*(found|supported|available)/.test(text)) return "model_not_found";
  if (status === 401 || status === 403 || error?.name === "ProviderAuthError") return "authentication_error";
  if (typeof status === "number" && status >= 500) return "server_overloaded";
  return error?.name || "unknown_provider_error";
}

export const DevwsPlugin = async ({ directory }) => {
  // The pane's own conversations; a subagent's (a child session) is part of
  // its parent's turn, not a turn of its own.
  const own = new Set();
  const pending = new Map();
  let queue = Promise.resolve();
  const later = (task) => {
    queue = queue.then(task, task);
    return queue;
  };
  const state = (event, sessionID, model) =>
    later(() =>
      run(
        join(scripts, "agent_state.sh"),
        [],
        JSON.stringify({
          hook_event_name: event,
          transcript_path: database && sessionID ? `${database}#${sessionID}` : "",
          model: model ?? "",
        }),
      ),
    );
  const follow = (phase, file) =>
    later(() =>
      run(
        join(scripts, "agent_follow_hook.sh"),
        phase ? ["opencode", phase] : ["opencode"],
        JSON.stringify({ cwd: directory, tool_input: { file_path: file ?? "" } }),
      ),
    );

  return {
    event: async ({ event }) => {
      const properties = event.properties ?? {};
      switch (event.type) {
        case "session.created":
          if (properties.info && !properties.info.parentID) {
            own.add(properties.info.id);
            await state("SessionStart", properties.info.id);
          }
          break;
        case "session.status":
          if (own.has(properties.sessionID) && properties.status?.type === "busy") {
            await state("UserPromptSubmit", properties.sessionID);
          }
          break;
        case "permission.asked":
          if (own.has(properties.sessionID)) await state("PermissionRequest", properties.sessionID);
          break;
        case "session.idle":
          if (own.has(properties.sessionID)) await state("Stop", properties.sessionID);
          break;
        case "session.error":
          if (!properties.sessionID || own.has(properties.sessionID)) {
            const envelope = JSON.stringify({ type: "error", error: { code: errorCode(properties.error) } });
            const pane = process.env.TMUX_PANE;
            if (pane) await later(() => run("tmux", ["set-option", "-p", "-t", pane, "@devws_startup_error", envelope]));
          }
          break;
      }
    },
    "tool.execute.before": async (input, output) => {
      if (!editTools.has(input.tool)) return;
      pending.set(input.callID, output.args?.filePath);
      await follow("pre");
    },
    "tool.execute.after": async (input) => {
      if (!pending.has(input.callID)) return;
      const file = pending.get(input.callID);
      pending.delete(input.callID);
      await follow("", file);
    },
  };
};
