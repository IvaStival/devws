#!/usr/bin/env bash
# The system workspace (system.yml, system_panel.sh): btop over lazydocker
# beside the sidebar, and a log pane running an editable tail of a chosen file.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS="$ROOT/config/tmuxinator"
PANEL="$SCRIPTS/system_panel.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-system.XXXXXX")"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-system-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'System workspace test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server, and HOME is the test's own.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator"
# The panes' shells are not login shells: with the test's empty HOME, a login
# shell gets only /etc/profile's PATH (on Linux, no Homebrew tmux and none of
# the stubs). A plain one keeps the PATH of whoever started the server.
# Panes numbered from 1, as tmux.conf numbers them (system.yml's startup_pane).
printf 'set -g default-command "exec bash --noprofile --norc"\nset -g base-index 1\nsetw -g pane-base-index 1\n' > "$TEST_ROOT/tmux.conf"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f "$TEST_ROOT/tmux.conf" "\$@"
EOF
# btop and lazydocker say they ran, then stay up as the real ones do.
for tool in btop lazydocker; do
  cat > "$TEST_ROOT/bin/$tool" <<EOF
#!/usr/bin/env bash
printf '%s ran\n' "$tool"
exec sleep 600
EOF
done
# fzf answers the log list from fzf.answers, a line per question (an empty
# one is Esc), as the user would.
cat > "$TEST_ROOT/bin/fzf" <<EOF
#!/usr/bin/env bash
cat > /dev/null
printf 'asked\\n' >> "$TEST_ROOT/fzf.asked"
printf '%s\\n' "\$*" > "$TEST_ROOT/fzf.options"
answer="\$(head -n 1 "$TEST_ROOT/fzf.answers" 2>/dev/null)"
tail -n +2 "$TEST_ROOT/fzf.answers" > "$TEST_ROOT/fzf.answers.next" 2>/dev/null
mv "$TEST_ROOT/fzf.answers.next" "$TEST_ROOT/fzf.answers"
[ -n "\$answer" ] || exit 130
printf '%s\\n' "\$answer"
EOF
chmod +x "$TEST_ROOT/bin/"*
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX TMUX_PANE

panel() { "$PANEL" "$@"; }

state() { printf '%s/.devws-state/%s' "$HOME" "$1"; }

wait_for() {
  local attempt
  for attempt in $(seq 1 100); do
    eval "$1" && return 0
    sleep 0.1
  done
  return 1
}

pane_with_role() {
  tmux list-panes -s -t "=$1:" -F '#{pane_id}|#{@devws_role}' |
    awk -F '|' -v role="$2" '$2 == role { print $1; exit }'
}

test_the_command_quotes_the_path() {
  local command
  command="$(panel command-for '/tmp/a b.log')"
  case "$command" in
    'tail -n 200 -F '*) ;;
    *) fail "not a tail command: $command" ;;
  esac
  eval "set -- ${command#tail -n 200 -F }"
  [ "$#" = 1 ] && [ "$1" = '/tmp/a b.log' ] || fail "the path was not quoted: $command"
}

test_use_remembers_the_log() {
  local log index
  rm -rf "$HOME/.devws-state"
  for index in $(seq 1 12); do
    log="$TEST_ROOT/logs/$index.log"
    mkdir -p "$(dirname "$log")" && : > "$log"
    panel use "$log"
  done
  panel use "$TEST_ROOT/logs/5.log"
  [ "$(cat "$(state system-log-command)")" = "$(panel command-for "$TEST_ROOT/logs/5.log")" ] ||
    fail "use did not save the command: $(cat "$(state system-log-command)")"
  [ "$(head -n 1 "$(state system-logs)")" = "$TEST_ROOT/logs/5.log" ] || fail "the last log is not first"
  [ "$(grep -c '/5.log$' "$(state system-logs)")" = 1 ] || fail "a log was listed twice"
  [ "$(wc -l < "$(state system-logs)" | tr -d ' ')" = 10 ] || fail "more than 10 logs were kept"
  ! panel use "$TEST_ROOT/logs/missing.log" 2>/dev/null || fail "a missing file was accepted"
}

test_choices_list_search_type_recent_then_found() {
  local project="$TEST_ROOT/project" choices keys
  rm -rf "$HOME/.devws-state"
  mkdir -p "$project/var" "$project/node_modules/pkg"
  : > "$project/var/app.log"
  : > "$project/node_modules/pkg/noise.log"
  : > "$TEST_ROOT/recent.log"
  : > "$TEST_ROOT/gone.log"
  panel use "$TEST_ROOT/gone.log"
  panel use "$TEST_ROOT/recent.log"
  rm "$TEST_ROOT/gone.log"

  choices="$(panel choices "$project")"
  keys="$(cut -f1 <<<"$choices")"
  [ "$(sed -n 1p <<<"$keys")" = search ] && [ "$(sed -n 2p <<<"$keys")" = type ] ||
    fail "search and type do not come first: $choices"
  [ "$(sed -n 3p <<<"$keys")" = "$TEST_ROOT/recent.log" ] || fail "the recent log does not follow: $choices"
  ! grep -qx "$TEST_ROOT/gone.log" <<<"$keys" || fail "a deleted log was offered: $choices"
  grep -qx "$project/var/app.log" <<<"$keys" || fail "the project's log was not found: $choices"
  ! grep -q noise.log <<<"$keys" || fail "node_modules was searched: $choices"
}

test_the_template_opens_the_three_panels() {
  local session=dev-system log top docker picker width
  rm -rf "$HOME/.devws-state"
  ln -sf "$SCRIPTS/system.yml" "$HOME/.tmuxinator/system.yml"
  ln -sf "$PANEL" "$HOME/.tmuxinator/system_panel.sh"
  # The sidebar stands in for session_picker.sh: it only names itself.
  printf '#!/usr/bin/env bash\ntmux select-pane -t "$TMUX_PANE" -T devws-picker\nexec sleep 600\n' \
    > "$HOME/.tmuxinator/session_picker.sh"
  chmod +x "$HOME/.tmuxinator/session_picker.sh"
  ln -sf "$SCRIPTS/picker.sh" "$HOME/.tmuxinator/picker.sh"
  printf 'first line\nlast-line-marker\n' > "$TEST_ROOT/app.log"
  # The log pane's question is answered with app.log.
  printf '%s\t%s\n' "$TEST_ROOT/app.log" "$TEST_ROOT/app.log" > "$TEST_ROOT/fzf.answers"
  : > "$TEST_ROOT/fzf.asked"

  tmux new-session -d -s keep -x 200 -y 50
  tmuxinator start --no-attach system choose=1 >/dev/null 2>&1 || fail "tmuxinator could not start system.yml"
  tmux has-session -t "=$session" 2>/dev/null || fail "the session is not $session"
  # pre_window runs in each pane as it starts: the options come a moment later.
  wait_for '[ "$(tmux show-options -qv -t "=$session:" @devws_template)" = system ]' ||
    fail "the template is not recorded"

  # Each panel marks itself as its script starts, in no set order.
  wait_for '[ -n "$(pane_with_role "$session" system-log)" ]' || fail "the log pane did not mark itself"
  wait_for '[ -n "$(pane_with_role "$session" system-top)" ] && [ -n "$(pane_with_role "$session" system-docker)" ]' ||
    fail "btop or lazydocker's pane did not mark itself"
  top="$(pane_with_role "$session" system-top)"
  docker="$(pane_with_role "$session" system-docker)"
  log="$(pane_with_role "$session" system-log)"
  [ -n "$top" ] && [ -n "$docker" ] || fail "btop or lazydocker's pane did not mark itself"
  wait_for '[ -n "$(tmux list-panes -t "=$session:" -F "#{pane_id} #{pane_title}" | awk "\$2 == \"devws-picker\"")" ]' ||
    fail "the sidebar did not start"
  picker="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{pane_title}' | awk '$2 == "devws-picker" { print $1 }')"
  width="$(tmux display-message -p -t "$picker" '#{pane_width}')"
  [ "$width" -le 30 ] || fail "the sidebar is $width columns wide"
  # btop across the whole width beside the sidebar; under it, the log on the
  # left and lazydocker on the right.
  field() { tmux display-message -p -t "$1" "#{$2}"; }
  [ "$(field "$top" pane_left)" = "$(field "$log" pane_left)" ] ||
    fail "the log does not start under btop's left edge"
  [ "$(field "$top" pane_width)" = "$(( $(field "$log" pane_width) + 1 + $(field "$docker" pane_width) ))" ] ||
    fail "btop is not as wide as the log and lazydocker together"
  [ "$(field "$log" pane_top)" = "$(field "$docker" pane_top)" ] &&
    [ "$(field "$top" pane_top)" -lt "$(field "$log" pane_top)" ] ||
    fail "the log and lazydocker are not side by side under btop"
  [ "$(field "$log" pane_left)" -lt "$(field "$docker" pane_left)" ] ||
    fail "the log is not left of lazydocker"

  wait_for 'tmux capture-pane -p -t "$top" | grep -q "btop ran"' || fail "btop did not run"
  wait_for 'tmux capture-pane -p -t "$docker" | grep -q "lazydocker ran"' || fail "lazydocker did not run"

  # From the menu (choose=1) the log pane asks for the log itself, in a box
  # inset in it, and has the focus: btop and lazydocker run meanwhile.
  [ "$(tmux display-message -p -t "=$session:" '#{pane_id}')" = "$log" ] ||
    fail "the log pane does not have the focus"
  wait_for '[ -s "$TEST_ROOT/fzf.asked" ]' || fail "the log pane did not ask for the log"
  grep -q -- '--margin=' "$TEST_ROOT/fzf.options" && grep -q -- '--border=rounded' "$TEST_ROOT/fzf.options" ||
    fail "the question is not a box inside the pane: $(cat "$TEST_ROOT/fzf.options")"
  wait_for 'tmux capture-pane -p -t "$log" | grep -q last-line-marker' ||
    fail "the log pane does not show the file: $(tmux capture-pane -p -t "$log")"

  [ "$(cat "$(state system-log-command)")" = "$(panel command-for "$TEST_ROOT/app.log")" ] ||
    fail "the chosen log was not kept: $(cat "$(state system-log-command)" 2>/dev/null)"

  # Ctrl-C stops the tail and asks again; editing the command comes first,
  # and the edit runs and is remembered.
  printf 'other-marker\n' > "$TEST_ROOT/other.log"
  printf 'edit\t✎ Edit the command\n' > "$TEST_ROOT/fzf.answers"
  tmux send-keys -t "$log" C-c
  wait_for 'tmux capture-pane -p -t "$log" | grep -q "^log \\$ tail"' ||
    fail "Ctrl-C did not offer the command: $(tmux capture-pane -p -t "$log")"
  tmux send-keys -t "$log" C-u "tail -n 5 -F '$TEST_ROOT/other.log'" Enter
  wait_for 'tmux capture-pane -p -t "$log" | grep -q "^other-marker"' ||
    fail "the edited command did not run: $(tmux capture-pane -p -t "$log")"
  [ "$(cat "$(state system-log-command)")" = "tail -n 5 -F '$TEST_ROOT/other.log'" ] ||
    fail "the edited command was not saved: $(cat "$(state system-log-command)")"

  # Esc on the list follows the same command again.
  : > "$TEST_ROOT/fzf.answers"
  : > "$TEST_ROOT/fzf.asked"
  tmux send-keys -t "$log" C-c
  wait_for '[ -s "$TEST_ROOT/fzf.asked" ]' || fail "Ctrl-C did not ask for the log again"
  wait_for 'tmux capture-pane -p -t "$log" | grep -q "^other-marker"' ||
    fail "Esc did not follow the same log: $(tmux capture-pane -p -t "$log")"
}

# Restored (no choose), the log pane follows the log it had, asking nothing.
# A tmux server started before the user joined the socket's group runs its
# panes without it: lazydocker joins it (sg) to reach Docker.
test_lazydocker_joins_the_docker_group_it_lacks() {
  local socket="$TEST_ROOT/docker.sock"
  python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$socket"
  ! DOCKER_HOST="unix://$socket" panel docker-group >/dev/null || fail "a reachable socket asked for a group"
  chmod 000 "$socket"
  [ "$(DOCKER_HOST="unix://$socket" panel docker-group)" = "$(id -gn)" ] ||
    fail "the socket's group was not joined: $(DOCKER_HOST="unix://$socket" panel docker-group)"
  ! DOCKER_HOST=tcp://127.0.0.1:2375 panel docker-group >/dev/null || fail "a TCP Docker asked for a group"
}

test_a_restored_workspace_follows_its_log() {
  local session=dev-restored log
  : > "$TEST_ROOT/fzf.answers"
  : > "$TEST_ROOT/fzf.asked"
  tmuxinator start --no-attach system --name "$session" >/dev/null 2>&1 || fail "tmuxinator could not start system.yml again"
  wait_for '[ -n "$(pane_with_role "$session" system-log)" ]' || fail "the restored log pane did not mark itself"
  log="$(pane_with_role "$session" system-log)"
  wait_for 'tmux capture-pane -p -t "$log" | grep -q "^other-marker"' ||
    fail "the restored log pane does not follow its log: $(tmux capture-pane -p -t "$log")"
  [ ! -s "$TEST_ROOT/fzf.asked" ] || fail "the restored log pane asked for the log"
  tmux kill-session -t "=$session"
}

# The panels' headers name them (Log · its command), as the agents' do.
test_the_headers_name_the_panels() {
  local session=dev-system log header
  log="$(pane_with_role "$session" system-log)"
  header="$(sed -n 's/^set -g pane-border-format "\(.*\)"$/\1/p' "$ROOT/config/tmux/tmux.conf")"
  [ -n "$header" ] || fail "could not read the header format"
  tmux display-message -p -t "$log" "$header" | grep -q "Log · tail -n 5" ||
    fail "the log's header: $(tmux display-message -p -t "$log" "$header")"
  tmux display-message -p -t "$(pane_with_role "$session" system-top)" "$header" | grep -q 'System' ||
    fail "btop's header: $(tmux display-message -p -t "$(pane_with_role "$session" system-top)" "$header")"
}

test_the_command_quotes_the_path
test_use_remembers_the_log
test_choices_list_search_type_recent_then_found
test_the_template_opens_the_three_panels
test_the_headers_name_the_panels
test_lazydocker_joins_the_docker_group_it_lacks
test_a_restored_workspace_follows_its_log

printf 'System workspace tests passed.\n'
