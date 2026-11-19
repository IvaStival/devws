#!/usr/bin/env bash
# Claude Code in devws (agents.sh loads it; docs/architecture/agent-modules.md
# lists the hooks). An account is a CLAUDE_CONFIG_DIR. Its hooks and status
# line come from settings.json here, passed on its command line.

claude_program() { printf 'claude'; }
claude_default_home() { printf '%s' "$HOME/.claude"; }
claude_account_variables() { printf 'CLAUDE_CONFIG_DIR\n'; }
claude_account_env() { export CLAUDE_CONFIG_DIR="$1"; }

# <resume> <prompt file> <args…>: the editor prompt appended to its own.
claude_launch() {
  local resume="$1" prompt_file="$2"
  shift 2
  claude ${resume:+--continue} --settings "$(agent_module_dir claude)/settings.json" \
    --append-system-prompt "$(cat "$prompt_file")" "$@"
}

# Ctrl-C stops a turn.
claude_interrupt_keys() { printf 'C-c'; }

# Its limits come from its status line (agent_statusline.sh).
claude_limits() { tmux show-option -gqv "@devws_limits_$1" 2>/dev/null; }

# Anthropic's own when it answers (recorded for the header too), else a
# recent status line reading, else nothing. A reading stands in for 15
# minutes: an older one may be from before a reset (one bought early, too),
# and a stale "used up" blocks the team until a Claude session happens to run.
claude_live_limits() {
  local agent="$1" live five_hour week at reading_lasts=900
  if live="$("$(agent_module_dir claude)/limits.sh" "$agent")"; then
    tmux set-option -g "@devws_limits_$agent" "$live $(date +%s)" 2>/dev/null
    printf '%s' "$live"
    return
  fi
  read -r five_hour week at <<< "$(claude_limits "$agent")"
  [[ "${at:-}" =~ ^[0-9]+$ ]] || return 0
  [ $(($(date +%s) - at)) -le "$reading_lasts" ] || return 0
  printf '%s %s' "$five_hour" "$week"
}

# The account's own model, else Opus, which Claude Code starts on: it writes
# its default nowhere.
claude_default_model() {
  local model
  model="$(jq -r '.model // empty' "$(agent_home "$1")/settings.json" 2>/dev/null)"
  printf '%s' "${model:-opus}"
}

# Its status line passes the plan's limits, as % used.
claude_status_line_limits() {
  printf '%s' "$1" | jq -r '[(.rate_limits.five_hour.used_percentage // empty | floor),
    (.rate_limits.seven_day.used_percentage // empty | floor)] | select(length > 0) | map(tostring) | join(" ")' 2>/dev/null
}
claude_status_line_model() { printf '%s' "$1" | jq -r '.model.id // empty | select(type == "string")' 2>/dev/null; }
# The folder it runs with (agent_runner.sh sets it), else the account's.
claude_settings_file() { printf '%s' "${CLAUDE_CONFIG_DIR:-$1}/settings.json"; }

# The tool call's file_path, at the line its patch changed, or a Read's
# offset; the new text when there is no patch.
claude_follow() {
  local payload file text="" event=--follow line offset
  payload="$(cat)"
  file="$(printf '%s' "$payload" | jq -r '.tool_input.file_path // .tool_input.notebook_path // empty' 2>/dev/null)"
  if [ "$(printf '%s' "$payload" | jq -r '.tool_name // empty' 2>/dev/null)" = "Read" ]; then
    event=--follow-read
    offset="$(printf '%s' "$payload" | jq -r '.tool_input.offset // empty' 2>/dev/null)"
    [ -z "$file" ] || [ -z "$offset" ] || file="$file:$offset"
  else
    # The patch Claude Code reports says where the change is; searching the
    # file for the new text would stop at an earlier line that reads the
    # same (an "end", a "}"), so the text is only the fallback.
    line="$(printf '%s' "$payload" | jq -r '.tool_response.structuredPatch[0] // empty
      | .newStart + (.lines | map(startswith("+") or startswith("-")) | index(true) // 0)' 2>/dev/null)"
    if [ -n "$file" ] && [ -n "$line" ]; then
      file="$file:$line"
    else
      text="$(printf '%s' "$payload" | jq -r '.tool_input.new_string // .tool_input.edits[0].new_string // .tool_input.content // empty' 2>/dev/null |
        awk 'NF { print; exit }')"
    fi
  fi
  printf '%s\n%s\n%s\n' "$event" "$file" "$text"
}

# An interrupted turn leaves a user message "[Request interrupted by user…]"
# as the newest turn boundary in the transcript's tail.
claude_interrupted() {
  [ -r "$1" ] || return 1
  tail -n 50 "$1" 2>/dev/null | jq -rR '
    fromjson? | select(.type == "user") | .message.content |
    if type == "string" then "prompt"
    elif type == "array" then
      (map(select(.type == "text")) | first // empty | .text |
        if startswith("[Request interrupted by user") then "turn_aborted" else "prompt" end)
    else empty end' 2>/dev/null | tail -n 1 | grep -qx turn_aborted
}

# ---- token usage (run inside header_tokens.sh)

# A message's usage is on several lines: one per id.
claude_messages_jq() {
  printf '%s' '[inputs | fromjson? | select(.type == "assistant" and .message.usage and .message.id)]
  | map({id: .message.id, at: (.timestamp | epoch), model: .message.model, usage: .message.usage,
         total: (.message.usage | (.input_tokens + .output_tokens
           + (.cache_creation_input_tokens // 0) + (.cache_read_input_tokens // 0)))})
  | unique_by(.id)'
}

claude_owns_transcript() { grep -q '"type":"assistant"' <<< "$2"; }

# Its logs do not know the context window: the context is shown in tokens.
claude_session() {
  printf '%s\n' "$2" |
    jq -nRr "$human $epoch $(claude_messages_jq) | select(length > 0)
      | \"conv \(map(.total) | add | human) · ctx \(sort_by(.at) | last | .usage
        | (.input_tokens + (.cache_creation_input_tokens // 0) + (.cache_read_input_tokens // 0)) | human)\"" 2>/dev/null
  return 0
}

# Rates: input, cache write 5 min, cache write 1 h, cache read, output. A
# message without the 5 min / 1 h split counts its writes as 5 min ones.
claude_priced_jq() {
  printf '%s' 'price(.model) as $rates
    | (.usage.cache_creation // {}) as $writes
    | (($writes.ephemeral_5m_input_tokens // null) == null and ($writes.ephemeral_1h_input_tokens // null) == null) as $unsplit
    | {input: .usage.input_tokens, output: .usage.output_tokens,
       cache: ((.usage.cache_creation_input_tokens // 0) + (.usage.cache_read_input_tokens // 0)),
       unpriced: (if $rates == null then 1 else 0 end),
       cost: (if $rates == null then 0 else ((.usage.input_tokens * $rates[0]
         + (if $unsplit then (.usage.cache_creation_input_tokens // 0) else ($writes.ephemeral_5m_input_tokens // 0) end) * $rates[1]
         + (if $unsplit then 0 else ($writes.ephemeral_1h_input_tokens // 0) end) * $rates[2]
         + (.usage.cache_read_input_tokens // 0) * $rates[3]
         + .usage.output_tokens * $rates[4]) / 1000000) end)}'
}

claude_usage() {
  printf '%s\n' "$2" |
    jq -nRr --rawfile prices "$prices_file" "$epoch $price $(claude_messages_jq)
      | select(length > 0)
      | map(select(.model != \"<synthetic>\") | $(claude_priced_jq))
      | \"\(map(.input) | add // 0) \(map(.output) | add // 0) \(map(.cache) | add // 0) \(map(.cost) | add // 0 | . * 100 | round / 100) \(map(.unpriced) | max // 0)\"" 2>/dev/null
  return 0
}

# A resumed conversation repeats a message in its log, so each line carries
# the message id: the history counts an id once over every log.
claude_messages() {
  printf '%s\n' "$2" |
    jq -nRr --rawfile prices "$prices_file" "$epoch $price $(claude_messages_jq)
      | map(select(.model != \"<synthetic>\") | {id, day: (.at | strflocaltime(\"%Y-%m-%d\"))} + ($(claude_priced_jq)))
      | .[] | \"\(.id) \(.day) \(.input) \(.output) \(.cache) \(.cost | . * 1000000 | round / 1000000) \(.unpriced)\"" 2>/dev/null
  return 0
}

claude_model() {
  printf '%s\n' "$2" |
    jq -nRr --arg timed "${3:-}" '[inputs | fromjson? | select(.type == "assistant") | . as $record | .message.model
      | select(type == "string" and length > 0 and . != "<synthetic>")
      | {model: ., at: (try ($record.timestamp | sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601) catch 0)}]
      | last // empty
      | if $timed == "timed" then [(.at // 0), .model] | @tsv else .model end'
}

# <since> <home>: "<US$> <unpriced>".
claude_month() {
  recent_logs "$2/projects" 32 | token_records |
    jq -nRr --argjson since "$1" --rawfile prices "$prices_file" "$epoch $price $(claude_messages_jq)
      | map(select(.at >= \$since and .model != \"<synthetic>\") | $(claude_priced_jq))
      | \"\(map(.cost) | add // 0 | . * 100 | round / 100) \(map(.unpriced) | max // 0)\""
}

claude_usage_logs() { printf '%s\t%s\n' "$1/projects" '*.jsonl'; }

# ---- project, memory, commits

# CLAUDE.md and .claude/skills from devws's template.
claude_init_project() {
  local project_root="$1" template_root="$2"
  if [ -e "$project_root/CLAUDE.md" ]; then
    printf 'skipped:CLAUDE.md already exists\n'
  else
    cp "$template_root/CLAUDE.md" "$project_root/CLAUDE.md"
    printf 'created:CLAUDE.md\n'
  fi
  mkdir -p "$project_root/.claude/skills"
  cp -Rn "$template_root/skills/." "$project_root/.claude/skills/"
}

claude_memory_client() { printf 'claude-code'; }

# /commit when it can see a commit skill (the project's or its account's).
claude_commit_command() {
  local home
  home="$(agent_home "$1" 2>/dev/null)"
  if [ -f "$2/.claude/skills/commit/SKILL.md" ] || [ -f "$home/skills/commit/SKILL.md" ]; then
    printf '/commit'
  fi
}
