#!/usr/bin/env bash
# Antigravity (agy) in devws (agents.sh loads it;
# docs/architecture/agent-modules.md lists the hooks). It has one account, in
# its own folder: no variable moves it. agy takes hooks and MCP servers only
# from plugins in its config folder, so devws keeps one there (plugin.sh):
# hook.sh reports its events, mcp.sh serves the leader's tools. menu.sh is
# the devws menu's ◆ Antigravity screen.

antigravity_program() { printf 'agy'; }
antigravity_default_home() { printf '%s' "$HOME/.gemini/antigravity-cli"; }
antigravity_account_variables() { printf 'DEVWS_AGY_MCP_CONFIG\nDEVWS_AGY_PROMPT_FILE\n'; }

# <home>: only its own folder.
antigravity_account_env() {
  [ "$1" = "$(antigravity_default_home)" ] && return 0
  printf 'Antigravity runs only its own account, in %s\n' "$(antigravity_default_home)" >&2
  return 1
}

# It asks "Do you trust …?" of every folder it has not been told to trust:
# the task folders devAgentics made are trusted, and devws's plugin is
# installed before each start.
antigravity_prepare() {
  local folder module
  module="$(agent_module_dir antigravity)"
  if ! "$module/plugin.sh" install; then
    printf 'Could not install the devws plugin for Antigravity: no state, editor link or leader tools.\n' >&2
  fi
  folder="$(agent_task_folder)" || return 0
  "$module/plugin.sh" trust "$folder" ||
    printf 'Could not trust %s for Antigravity: it will ask.\n' "$folder" >&2
}

# It takes one --settings, for this session only: devAgentics' (what its
# agents may do without asking) with devws's status line, which records the
# Gemini quota (agent_statusline.sh); agy's own settings stay. It has no
# --mcp-config: devAgentics' one goes to the plugin's server (mcp.sh).
antigravity_launch() {
  local resume="$1" prompt_file="$2" settings='{}' arguments=()
  shift 2
  unset DEVWS_AGY_MCP_CONFIG
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --mcp-config)
        export DEVWS_AGY_MCP_CONFIG="${2:-}"
        shift 2 || shift
        continue
        ;;
      --settings)
        settings="${2:-}"
        shift 2 || shift
        continue
        ;;
    esac
    arguments+=("$1")
    shift
  done
  if ! settings="$(jq -c --arg command "\"$agent_scripts/agent_statusline.sh\"" \
      '. + {statusLine: {type: "command", command: $command}}' <<<"$settings")"; then
    printf 'The Antigravity settings are not JSON.\n' >&2
    return 2
  fi
  export DEVWS_AGY_PROMPT_FILE="$prompt_file"
  agy ${resume:+--continue} --settings "$settings" ${arguments[@]+"${arguments[@]}"}
}

# One Escape stops a turn ("esc to cancel"). It runs no hook then: its screen
# says "Interrupted".
antigravity_interrupt_keys() { printf 'Escape'; }
antigravity_interrupt_mark() { tmux capture-pane -p -t "$1" 2>/dev/null | grep -c 'Interrupted'; }
antigravity_interrupt_seen() { [ "$(antigravity_interrupt_mark "$1")" -gt "${2:-0}" ]; }
antigravity_pane_options() { printf '@devws_agy_state\n'; }
# Esc by hand: its status line says when it is idle again (@devws_agy_state).
antigravity_interrupted() {
  [ "$(tmux show-options -pqv -t "$2" @devws_agy_state 2>/dev/null)" = idle ]
}

# Its Gemini quota, from its status line (agent_statusline.sh).
antigravity_limits() { tmux show-option -gqv "@devws_limits_$1" 2>/dev/null; }

# The account's own model, else Gemini 4 Carbon (Medium), which agy started on
# whenever no model was given: it writes its default nowhere.
antigravity_default_model() {
  local model
  model="$(jq -r '.model // empty | strings' "$(agent_home "$1")/settings.json" 2>/dev/null)"
  printf '%s' "${model:-gemini-4-carbon-medium}"
}

# The id, without its display name; not the "Fetching…" line.
antigravity_models() {
  command -v agy >/dev/null 2>&1 || { printf 'agy is not installed (see antigravity.google)\n' >&2; return 1; }
  agy models | awk -F '\t' 'NF > 1 { print $1 }'
}

# Its status line payload has Claude Code's shape, its quota as the fraction
# left; its model id there is a display name (its hooks record the id).
antigravity_status_line_limits() {
  printf '%s' "$1" | jq -r '[(.quota["gemini-5h"].remaining_fraction // empty | (1 - .) * 100 | floor),
    (.quota["gemini-weekly"].remaining_fraction // empty | (1 - .) * 100 | floor)] | select(length > 0) | map(tostring) | join(" ")' 2>/dev/null
}
antigravity_status_line_extra() {
  local state
  [ -n "${TMUX_PANE:-}" ] || return 0
  state="$(printf '%s' "$1" | jq -r '.agent_state // empty' 2>/dev/null)"
  [ -z "$state" ] || [ "$(tmux show-options -pqv -t "$TMUX_PANE" @devws_agy_state 2>/dev/null)" = "$state" ] ||
    tmux set-option -p -t "$TMUX_PANE" @devws_agy_state "$state" 2>/dev/null
}
antigravity_settings_file() { printf '%s' "$1/settings.json"; }

# agy's tool call: its Path is the file, and a tool whose name reads or views
# is a read.
antigravity_follow() {
  local payload file text="" event=--follow
  payload="$(cat)"
  file="$(printf '%s' "$payload" | jq -r '.toolCall.args | (.Path // .TargetFile // .AbsolutePath // empty)' 2>/dev/null)"
  if printf '%s' "$payload" | jq -r '.toolCall.name // ""' 2>/dev/null | grep -qiE 'read|view'; then
    event=--follow-read
  else
    text="$(printf '%s' "$payload" | jq -r '.toolCall.args | (.Replacement // .Content // .CodeContent // empty)' 2>/dev/null |
      awk 'NF { print; exit }')"
  fi
  printf '%s\n%s\n%s\n' "$event" "$file" "$text"
}

# ---- token usage (run inside header_tokens.sh)

# A step per response, with what it used: input without the cache read, which
# is counted apart.
antigravity_messages_jq() {
  printf '%s' '[inputs | fromjson? | select(.type == "PLANNER_RESPONSE")]
  | map({at: (.created_at | epoch), model: (.model // ""), input: (.input_tokens // 0),
         output: (.output_tokens // 0), cache: (.cache_read_tokens // 0)}
        | .total = .input + .output + .cache)'
}
antigravity_priced_jq() {
  printf '%s' 'price(.model) as $rates
    | . + {unpriced: (if $rates == null then 1 else 0 end),
           cost: (if $rates == null then 0 else ((.input * $rates[0] + .cache * $rates[3] + .output * $rates[4]) / 1000000) end)}'
}

antigravity_owns_transcript() { grep -q '"PLANNER_RESPONSE"' <<< "$2"; }

antigravity_session() {
  printf '%s\n' "$2" |
    jq -nRr "$human $epoch $(antigravity_messages_jq) | select(length > 0)
      | \"conv \(map(.total) | add | human) · ctx \(last | .input + .cache | human)\"" 2>/dev/null
  return 0
}

antigravity_usage() {
  printf '%s\n' "$2" |
    jq -nRr --rawfile prices "$prices_file" "$epoch $price $(antigravity_messages_jq)
      | select(length > 0)
      | map($(antigravity_priced_jq))
      | \"\(map(.input) | add) \(map(.output) | add) \(map(.cache) | add) \(map(.cost) | add | . * 100 | round / 100) \(map(.unpriced) | max)\"" 2>/dev/null
  return 0
}

# A conversation has one log, so its responses never repeat: id "-".
antigravity_messages() {
  printf '%s\n' "$2" |
    jq -nRr --rawfile prices "$prices_file" "$epoch $price $(antigravity_messages_jq)
      | map($(antigravity_priced_jq) | . + {id: \"-\", day: (.at | strflocaltime(\"%Y-%m-%d\"))})
      | .[] | \"\(.id) \(.day) \(.input) \(.output) \(.cache) \(.cost | . * 1000000 | round / 1000000) \(.unpriced)\"" 2>/dev/null
  return 0
}

antigravity_model() {
  printf '%s\n' "$2" |
    jq -nRr --arg timed "${3:-}" '[inputs | fromjson? | select(.type == "PLANNER_RESPONSE") | . as $record | .model
      | select(type == "string" and length > 0)
      | {model: ., at: (try ($record.created_at | sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601) catch 0)}]
      | last // empty
      | if $timed == "timed" then [(.at // 0), .model] | @tsv else .model end'
}

# On a Google plan, its month is told in tokens: its models have no API price
# here. "0 0 <tokens>".
antigravity_month() {
  local logs=() log
  while IFS= read -r -d '' log; do
    logs+=("$log")
  done < <(recent_logs "$2/brain" 32 transcript_full.jsonl)
  if [ "${#logs[@]}" -eq 0 ]; then
    printf '0 0 0\n'
    return
  fi
  printf '0 0 %s\n' "$(printf '%s\0' "${logs[@]}" | token_records |
    jq -nRr --argjson since "$1" "$epoch $(antigravity_messages_jq) | map(select(.at >= \$since) | .total) | add // 0")"
}

# A conversation's full log; its compact copy beside it repeats it.
antigravity_usage_logs() { printf '%s\t%s\n' "$1/brain" transcript_full.jsonl; }

# ---- project, menu

# It reads AGENTS.md and .agents/skills.
antigravity_init_project() { agent_init_agents_md "$1" "$2" Antigravity; }
antigravity_menu_entry() { printf 'agent|antigravity|◆ Antigravity||agent_screen antigravity\n'; }
antigravity_screen() { "$(agent_module_dir antigravity)/menu.sh"; }
