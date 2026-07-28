#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-markdown.XXXXXX")"
ZSH_BIN="$(command -v zsh)"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'Markdown viewer test failed: %s\n' "$*" >&2
  exit 1
}

make_fake_commands() {
  local fake_bin="$1"

  mkdir -p "$fake_bin"
  cat > "$fake_bin/glow" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$GLOW_LOG"
EOF
  cat > "$fake_bin/fzf" <<'EOF'
#!/usr/bin/env bash
cat > "$FZF_INPUT"
printf '%s\n' "$FZF_SELECTION"
EOF
  cat > "$fake_bin/open" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$1" >> "$OPEN_LOG"
EOF
  chmod +x "$fake_bin/glow" "$fake_bin/fzf" "$fake_bin/open"
}

run_devws() {
  local fake_bin="$1"
  local project="$2"
  shift 2
  rm -f "$TEST_ROOT/open.log"
  HOME="$TEST_ROOT/home" TMUX="" GLOW_LOG="$TEST_ROOT/glow.log" \
    FZF_INPUT="$TEST_ROOT/fzf-input" FZF_SELECTION="${FZF_SELECTION:-}" \
    OPEN_LOG="$TEST_ROOT/open.log" \
    PATH="$fake_bin:/usr/bin:/bin" \
    "$ZSH_BIN" -c 'source "$1"; cd "$2"; shift 2; devws md "$@"' \
    -- "$ROOT/shell/devws.zsh" "$project" "$@"
}

test_direct_file_with_spaces() {
  local project="$TEST_ROOT/direct"
  local fake_bin="$project/bin"
  mkdir -p "$project/docs"
  printf '# Guide\n' > "$project/docs/My Guide.md"
  make_fake_commands "$fake_bin"

  run_devws "$fake_bin" "$project" "docs/My Guide.md"
  [[ "$(sed -n '1p' "$TEST_ROOT/glow.log")" == "-p" ]] ||
    fail "Glow pager flag was not passed"
  [[ "$(sed -n '2p' "$TEST_ROOT/glow.log")" == "docs/My Guide.md" ]] ||
    fail "Markdown path with spaces was not preserved"
}

test_interactive_picker() {
  local project="$TEST_ROOT/picker"
  local fake_bin="$project/bin"
  mkdir -p "$project/docs"
  printf '# First\n' > "$project/README.md"
  printf '# Second\n' > "$project/docs/Guide.markdown"
  make_fake_commands "$fake_bin"

  FZF_SELECTION="./docs/Guide.markdown" \
    run_devws "$fake_bin" "$project"
  grep -q './README.md' "$TEST_ROOT/fzf-input" ||
    fail "picker did not receive README.md"
  grep -q './docs/Guide.markdown' "$TEST_ROOT/fzf-input" ||
    fail "picker did not receive .markdown files"
  [[ "$(sed -n '2p' "$TEST_ROOT/glow.log")" == "./docs/Guide.markdown" ]] ||
    fail "picker selection was not opened"
}

test_invalid_extension_is_rejected() {
  local project="$TEST_ROOT/invalid"
  local fake_bin="$project/bin"
  mkdir -p "$project"
  printf 'text\n' > "$project/notes.txt"
  make_fake_commands "$fake_bin"

  if run_devws "$fake_bin" "$project" "notes.txt" >/dev/null 2>&1; then
    fail "non-Markdown file should be rejected"
  fi
}

test_mermaid_fence_opens_browser() {
  local project="$TEST_ROOT/mermaid"
  local fake_bin="$project/bin"
  mkdir -p "$project"
  cat > "$project/README.md" <<'EOF'
# Diagram

```mermaid
flowchart TB
A-->B
```
EOF
  make_fake_commands "$fake_bin"

  run_devws "$fake_bin" "$project" "README.md"
  [[ -s "$TEST_ROOT/open.log" ]] ||
    fail "Mermaid fence did not trigger a browser open"
  local opened_path
  opened_path="$(cat "$TEST_ROOT/open.log")"
  [[ "$opened_path" == *.html ]] ||
    fail "browser was not opened with an HTML file"
  [[ -f "$opened_path" ]] ||
    fail "generated Mermaid preview HTML file does not exist"
}

test_no_mermaid_fence_skips_browser() {
  local project="$TEST_ROOT/no-mermaid"
  local fake_bin="$project/bin"
  mkdir -p "$project"
  printf '# Guide\n\nJust text, no diagrams.\n' > "$project/README.md"
  make_fake_commands "$fake_bin"

  run_devws "$fake_bin" "$project" "README.md"
  [[ ! -s "$TEST_ROOT/open.log" ]] ||
    fail "browser should not open for a file without a Mermaid fence"
}

test_missing_glow_is_reported() {
  local project="$TEST_ROOT/missing"
  local fake_bin="$project/bin"
  local output
  mkdir -p "$fake_bin"
  printf '# Missing\n' > "$project/README.md"

  if output="$(run_devws "$fake_bin" "$project" "README.md" 2>&1)"; then
    fail "missing Glow should fail"
  fi
  [[ "$output" == *"Glow is required"* ]] ||
    fail "missing Glow error was not actionable"
}

test_direct_file_with_spaces
test_interactive_picker
test_invalid_extension_is_rejected
test_mermaid_fence_opens_browser
test_no_mermaid_fence_skips_browser
test_missing_glow_is_reported

printf 'Markdown viewer tests passed.\n'
