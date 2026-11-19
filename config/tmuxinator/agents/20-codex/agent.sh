#!/usr/bin/env bash
# Codex in devws (agents.sh loads it; docs/architecture/agent-modules.md lists
# the hooks). An account is a CODEX_HOME. Its hooks, status line and the
# editor prompt are passed as -c overrides, so the account's config.toml
# stays as it is.

codex_program() { printf 'codex'; }
codex_default_home() { printf '%s' "$HOME/.codex"; }
codex_account_variables() { printf 'CODEX_HOME\n'; }
codex_account_env() { export CODEX_HOME="$1"; }

# Codex stops at "Trust this folder?" in a folder its config does not trust,
# and every task gets new lane folders: trust the folders devAgentics made for
# its tasks, as answering the question would, and no others. Trusting the
# tasks folder itself is not enough: Codex looks for the exact folder.
codex_prepare() {
  local folder config
  folder="$(agent_task_folder)" || return 0
  config="${CODEX_HOME:-$HOME/.codex}/config.toml"
  grep -qxF "[projects.\"$folder\"]" "$config" 2>/dev/null && return 0
  if ! { mkdir -p "$(dirname "$config")" && printf '\n[projects."%s"]\ntrust_level = "trusted"\n' "$folder" >> "$config"; }; then
    printf 'Could not trust %s for Codex in %s: it will ask.\n' "$folder" "$config" >&2
  fi
}

codex_launch() {
  local resume="$1" prompt_file="$2" state_hook event status_line_args=() resume_args=() state_hooks=()
  shift 2
  # Config overrides are TOML; jq's JSON strings are valid TOML strings, quotes escaped.
  # The follow hook in <phase> (agent_follow_hook.sh): every tool call, since
  # Codex's tools vary (exec, shell, apply_patch) and any may change files.
  codex_follow_hook() {
    printf '{hooks=[{type="command",command=%s}]}' \
      "$(jq -n --arg path "$agent_scripts/agent_follow_hook.sh" --arg phase "${1:-}" \
        '"\"\($path)\" codex" + (if $phase == "" then "" else " " + $phase end)')"
  }
  # Every state event reports to agent_state.sh (working / needs you / done).
  state_hook="{hooks=[{type=\"command\",command=$(jq -n --arg path "$agent_scripts/agent_state.sh" '"\"\($path)\""')}]}"
  for event in SessionStart PermissionRequest Stop SessionEnd; do
    state_hooks+=(-c "hooks.$event=[$state_hook]")
  done
  # Codex runs no status line command: its own items show what devws's
  # Claude bar does (agent_usage_bar.sh), unless the account chose its own.
  grep -q '^[[:space:]]*status_line[[:space:]]*=' "${CODEX_HOME:-$HOME/.codex}/config.toml" 2>/dev/null ||
    status_line_args=(-c 'tui.status_line=["model-with-reasoning","git-branch","five-hour-limit","weekly-limit","context-used"]')
  [ -n "$resume" ] && resume_args=(resume --last)
  codex ${resume_args[@]+"${resume_args[@]}"} \
    -c "hooks.UserPromptSubmit=[$state_hook,$(codex_follow_hook prompt)]" \
    -c "hooks.PreToolUse=[$(codex_follow_hook pre),$state_hook]" \
    -c "hooks.PostToolUse=[$(codex_follow_hook),$state_hook]" \
    "${state_hooks[@]}" \
    ${status_line_args[@]+"${status_line_args[@]}"} \
    -c "developer_instructions=$(jq -n --arg prompt "$(cat "$prompt_file")" '$prompt')" \
    "$@"
}

codex_interrupt_keys() { printf 'C-c'; }

# Its logs carry its limits (log_limits, below).
codex_limits() { "$agent_scripts/header_tokens.sh" limits "$1"; }

# The account's own model, else the first one its model list offers.
codex_default_model() {
  local home model
  home="$(agent_home "$1")"
  model="$(sed -n 's/^[[:space:]]*model[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$home/config.toml" 2>/dev/null | head -n 1)"
  [ -n "$model" ] || model="$(jq -r '[.models[] | select(.visibility == "list")] | sort_by(.priority) | first | .slug // empty' \
    "$home/models_cache.json" 2>/dev/null)"
  printf '%s' "$model"
}

# The patch is a string somewhere in the tool input; the last file it changes
# is shown, at the first line it adds there. Its reads are shell commands,
# not seen.
codex_follow() {
  local payload patch file text cwd
  payload="$(cat)"
  patch="$(printf '%s' "$payload" | jq -r '[.. | strings | select(contains("*** Begin Patch"))][0] // empty' 2>/dev/null)"
  file="$(printf '%s\n' "$patch" |
    sed -En -e 's/^\*\*\* (Update|Add) File: //p' -e 's/^\*\*\* Move to: //p' | tail -n 1)"
  text="$(printf '%s\n' "$patch" | awk -v file="$file" '
    /^\*\*\* (Update|Add) File: / || /^\*\*\* Move to: / { current = $0; sub(/^\*\*\* [^:]*: /, "", current) }
    current == file && /^\+/ && length($0) > 1 { print substr($0, 2); exit }')"
  cwd="$(printf '%s' "$payload" | jq -r '.cwd // empty' 2>/dev/null)"
  case "$file" in
    "" | /*) ;;
    *) [ -z "$cwd" ] || file="$cwd/$file" ;;
  esac
  printf '%s\n%s\n%s\n' --follow "$file" "$text"
}

# An interrupted turn logs "turn_aborted" as the newest turn boundary
# (task_started, task_complete) in the transcript's tail.
codex_interrupted() {
  [ -r "$1" ] || return 1
  tail -n 50 "$1" 2>/dev/null | jq -rR '
    fromjson? | select(.type == "event_msg") |
    .payload.type | select(. == "task_started" or . == "task_complete" or . == "turn_aborted")' 2>/dev/null |
    tail -n 1 | grep -qx turn_aborted
}

# ---- token usage (run inside header_tokens.sh)

codex_owns_transcript() { grep -q '"token_count"\|"token_usage_record"\|"turn_context"' <<< "$2"; }

# Codex knows its context window.
codex_session() {
  jq -nRr "$human [inputs | fromjson? | select(.type == \"event_msg\" and .payload.type == \"token_count\" and .payload.info) | .payload.info]
    | last // empty
    | \"conv \(.total_token_usage.total_tokens | human) · ctx \(.last_token_usage.input_tokens * 100 / .model_context_window | floor)%\"" \
    <<< "$2" 2>/dev/null
  return 0
}

# The running total of its latest count.
codex_usage() {
  printf '%s\n' "$2" |
    jq -nRr --rawfile prices "$prices_file" "$price
      [inputs | fromjson?] as \$lines
      | (\$lines | map(select(.type == \"turn_context\") | .payload.model) | last // \"\") as \$model
      | (\$lines | map(select(.type == \"event_msg\" and .payload.type == \"token_count\" and .payload.info) | .payload.info.total_token_usage) | last // empty) as \$total
      | (\$total.cached_input_tokens // 0) as \$cached
      | price(\$model) as \$rates
      | (\$total.input_tokens - \$cached) as \$input
      | \"\(\$input) \(\$total.output_tokens) \(\$cached) \(if \$rates == null then 0 else ((\$input * \$rates[0] + \$cached * \$rates[3] + \$total.output_tokens * \$rates[4]) / 1000000 * 100 | round / 100) end) \(if \$rates == null then 1 else 0 end)\"" 2>/dev/null
  return 0
}

# Each response has its own record; an older log without them counts its last
# total on the day of its last count. Its logs do not repeat: id "-".
codex_messages() {
  printf '%s\n' "$2" |
    jq -nRr --rawfile prices "$prices_file" "$epoch $price"'
      def day: epoch | strflocaltime("%Y-%m-%d");
      def priced($model; $input; $written; $cached; $output):
        price($model) as $rates
        | {input: $input, output: $output, cache: ($written + $cached),
           unpriced: (if $rates == null then 1 else 0 end),
           cost: (if $rates == null then 0
             else (($input * $rates[0] + $written * $rates[1] + $cached * $rates[3] + $output * $rates[4]) / 1000000) end)};
      [inputs | fromjson?] as $lines
      | ($lines | map(select(.type == "turn_context") | {key: (.payload.turn_id // ""), value: .payload.model}) | from_entries) as $models
      | ($lines | map(select(.type == "turn_context") | .payload.model) | last // "") as $last_model
      | ($lines | map(select(.type == "token_usage_record"))) as $responses
      | if ($responses | length) > 0 then
          $responses | map(.payload.usage as $usage
            | ($usage.cached_input_tokens // 0) as $cached
            | ($usage.cache_write_input_tokens // 0) as $written
            | priced($models[.payload.turn_id // ""] // $last_model;
                $usage.input_tokens - $cached - $written; $written; $cached; $usage.output_tokens)
            + {id: "-", day: (.timestamp | day)})
        else
          $lines | map(select(.type == "event_msg" and .payload.type == "token_count" and .payload.info))
          | last // empty
          | [.payload.info.total_token_usage as $total
             | ($total.cached_input_tokens // 0) as $cached
             | priced($last_model; $total.input_tokens - $cached; 0; $cached; $total.output_tokens)
             + {id: "-", day: (.timestamp | day)}]
        end
      | .[] | "\(.id) \(.day) \(.input) \(.output) \(.cache) \(.cost | . * 1000000 | round / 1000000) \(.unpriced)"' 2>/dev/null
  return 0
}

codex_model() {
  printf '%s\n' "$2" |
    jq -nRr --arg timed "${3:-}" '[inputs | fromjson? | select(.type == "turn_context") | . as $record | .payload.model
      | select(type == "string" and length > 0)
      | {model: ., at: (try ($record.timestamp | sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601) catch 0)}]
      | last // empty
      | if $timed == "timed" then [(.at // 0), .model] | @tsv else .model end'
}

# Each response logs its usage (token_usage_record) under its turn, whose
# turn_context names the model. Input counts cached and written tokens.
#   codex_month <since> <home>: "<US$> <unpriced>"
codex_month() {
  local logs=() log
  while IFS= read -r -d '' log; do
    logs+=("$log")
  done < <(recent_logs "$2/sessions" 32)
  if [ "${#logs[@]}" -eq 0 ]; then
    printf '0 0\n'
    return
  fi
  printf '%s\0' "${logs[@]}" | token_records |
    jq -nRr --argjson since "$1" --rawfile prices "$prices_file" "$epoch $price
      [inputs | fromjson?] as \$lines
      | (\$lines | map(select(.type == \"turn_context\") | {key: .payload.turn_id, value: .payload.model}) | from_entries) as \$models
      | \$lines | map(select(.type == \"token_usage_record\" and (.timestamp | epoch) >= \$since)
        | price(\$models[.payload.turn_id] // \"\") as \$rates | .payload.usage as \$usage
        | if \$rates == null then {cost: 0, unpriced: 1}
          else {cost: (((\$usage.input_tokens - (\$usage.cached_input_tokens // 0) - (\$usage.cache_write_input_tokens // 0)) * \$rates[0]
                + (\$usage.cache_write_input_tokens // 0) * \$rates[1]
                + (\$usage.cached_input_tokens // 0) * \$rates[3]
                + \$usage.output_tokens * \$rates[4]) / 1000000),
                unpriced: 0}
          end)
      | \"\(map(.cost) | add // 0 | . * 100 | round / 100) \(map(.unpriced) | max // 0)\""
}

# It logs its plan limits with each token count: the latest one counts, and a
# window that has reset since is back to 0.
#   codex_log_limits <home>: "<5-hour session> <week>"
codex_log_limits() {
  local logs=() log
  while IFS= read -r -d '' log; do
    logs+=("$log")
  done < <(recent_logs "$1/sessions" 8)
  [ "${#logs[@]}" -gt 0 ] || return 0
  printf '%s\0' "${logs[@]}" | token_records |
    jq -nRr --argjson now "$now" "$epoch [inputs | fromjson? | select(.payload.rate_limits.primary)
      | {at: (.timestamp | epoch), limits: .payload.rate_limits}] | sort_by(.at) | last // empty | .limits
      | [.primary, .secondary] | map(if . == null then \"-\" elif (.resets_at // 1e18) < \$now then 0 else .used_percent | floor end)
      | join(\" \")"
}

codex_usage_logs() { printf '%s\t%s\n' "$1/sessions" '*.jsonl'; }

# ---- project, memory

# AGENTS.md (devws's CLAUDE.md, renamed) and .agents/skills.
codex_init_project() { agent_init_agents_md "$1" "$2" Codex; }
codex_memory_client() { printf 'codex'; }
