#!/usr/bin/env bash
# Workspace templates of your own (workspace_template.sh, blank.yml): an empty
# workspace, saved with its panes' commands and layout, opened again on
# another folder, and restored from its template.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS="$ROOT/config/tmuxinator"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-template.XXXXXX")"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-template-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Workspace template test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server, and HOME is the test's own.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator"
# Plain shells, not login ones: with the test's empty HOME a login shell gets
# only /etc/profile's PATH, without the stubs.
printf 'set -g default-command "exec bash --noprofile --norc"\n' > "$TEST_ROOT/tmux.conf"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f "$TEST_ROOT/tmux.conf" "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX TMUX_PANE

for script in blank.yml workspace_template.sh workspace_panes.sh restore_state.sh agents.sh agents start_server.sh; do
  ln -s "$SCRIPTS/$script" "$HOME/.tmuxinator/$script"
done
# The sidebar stands in for session_picker.sh: it only names itself.
printf '#!/usr/bin/env bash\ntmux select-pane -t "$TMUX_PANE" -T devws-picker\nexec sleep 600\n' \
  > "$HOME/.tmuxinator/session_picker.sh"
chmod +x "$HOME/.tmuxinator/session_picker.sh"

TEMPLATE="$HOME/.tmuxinator/templates/watch.yml"

template() { "$HOME/.tmuxinator/workspace_template.sh" "$@"; }

wait_for() {
  local attempt
  for attempt in $(seq 1 100); do
    eval "$1" && return 0
    sleep 0.1
  done
  return 1
}

panes_of() {
  tmux list-panes -t "=$1:" -F '#{pane_id}'
}

sidebar_of() {
  tmux list-panes -t "=$1:" -F '#{pane_id} #{pane_title}' | awk '$2 == "devws-picker" { print $1 }'
}

test_an_empty_workspace_is_the_sidebar_and_a_shell() {
  local project="$TEST_ROOT/project"
  mkdir -p "$project/api"
  tmux new-session -d -s keep -x 200 -y 50
  template start blank "$project" dev-scratch --no-attach >/dev/null 2>&1 || fail "the empty workspace did not start"
  wait_for '[ -n "$(sidebar_of dev-scratch)" ]' || fail "the empty workspace has no sidebar"
  [ "$(panes_of dev-scratch | wc -l | tr -d ' ')" = 2 ] || fail "not the sidebar and one shell: $(panes_of dev-scratch)"
  wait_for '[ "$(tmux show-options -qv -t "=dev-scratch:" @devws_template)" = blank ]' ||
    fail "the empty workspace's template is not recorded"
  [ "$(tmux show-options -qv -t "=dev-scratch:" @devws_root)" = "$project" ] || fail "the workspace is not on its folder"
}

test_save_keeps_the_panes_commands_and_layout() {
  local shell layout
  shell="$(tmux list-panes -t "=dev-scratch:" -F '#{pane_id} #{@devws_role}' | awk '$2 == "terminal" { print $1 }')"
  [ -n "$shell" ] || fail "the empty workspace's shell has no terminal role"
  # A program typed in a shell, in a folder of the workspace's, and a shell.
  tmux split-window -d -h -t "$shell" -c "$TEST_ROOT/project/api"
  tmux send-keys -t "=dev-scratch:.2" "sleep 601" Enter
  wait_for 'tmux list-panes -t "=dev-scratch:" -F "#{pane_current_command}" | grep -qx sleep' ||
    fail "the program did not start"
  layout="$(tmux display-message -p -t "=dev-scratch:" '#{window_layout}')"

  template save dev-scratch watch || fail "save failed"
  [ -f "$TEMPLATE" ] || fail "no template file"
  grep -qF "layout: '$layout'" "$TEMPLATE" || fail "the layout was not kept: $(cat "$TEMPLATE")"
  grep -qx '        - picker: ~/.tmuxinator/session_picker.sh' "$TEMPLATE" || fail "the sidebar was not kept: $(cat "$TEMPLATE")"
  grep -qx '        - # a shell' "$TEMPLATE" || fail "the shell was not kept as one: $(cat "$TEMPLATE")"
  grep -qx "        - 'cd api && sleep 601'" "$TEMPLATE" || fail "the program was not kept in its folder: $(cat "$TEMPLATE")"
  grep -q '@devws_template watch' "$TEMPLATE" || fail "the template does not name itself: $(cat "$TEMPLATE")"
  # The workspace now restores from its own template.
  [ "$(tmux show-options -qv -t "=dev-scratch:" @devws_template)" = watch ] ||
    fail "the workspace does not follow its template"
  [ "$(template list)" = watch ] || fail "the template is not listed: $(template list)"
}

test_a_template_opens_on_another_folder() {
  local other="$TEST_ROOT/other"
  mkdir -p "$other/api"
  template start watch "$other" dev-again --no-attach >/dev/null 2>&1 || fail "the template did not open"
  wait_for '[ -n "$(sidebar_of dev-again)" ]' || fail "the template's workspace has no sidebar"
  [ "$(panes_of dev-again | wc -l | tr -d ' ')" = 3 ] || fail "not the template's three panes"
  # The saved layout: the three side by side, the sidebar 25 columns wide.
  [ "$(tmux list-panes -t "=dev-again:" -F '#{pane_top}' | sort -u | wc -l | tr -d ' ')" = 1 ] ||
    fail "the panes are not side by side: $(tmux list-panes -t "=dev-again:" -F '#{pane_top} #{pane_left} #{pane_width}')"
  [ "$(tmux display-message -p -t "$(sidebar_of dev-again)" '#{pane_width}')" -le 30 ] || fail "the sidebar is too wide"
  wait_for 'tmux list-panes -t "=dev-again:" -F "#{pane_current_command} #{pane_current_path}" | grep -qx "sleep $other/api"' ||
    fail "the program did not run in its folder: $(tmux list-panes -t "=dev-again:" -F '#{pane_current_command} #{pane_current_path}')"
  [ "$(tmux show-options -qv -t "=dev-again:" @devws_template)" = watch ] || fail "the template is not recorded"
  [ "$(tmux show-options -qv -t "=dev-again:" @devws_root)" = "$other" ] || fail "the workspace is not on the folder"
}

test_a_template_workspace_is_restored_from_it() {
  mkdir -p "$HOME/.devws-state"
  printf '#devws-state\t1\nW\tdev-back\t%s\ttemplate\twatch\t1\tLAYOUT\t-\twatch\nP\tdev-back\t0\tpicker\t%s\n' \
    "$TEST_ROOT/other" "$TEST_ROOT/other" > "$HOME/.devws-state/sessions.tsv"
  "$HOME/.tmuxinator/restore_state.sh" >"$TEST_ROOT/restore.out" 2>&1 || fail "restore failed: $(cat "$TEST_ROOT/restore.out")"
  tmux has-session -t "=dev-back" 2>/dev/null || fail "the workspace was not restored"
  [ "$(panes_of dev-back | wc -l | tr -d ' ')" = 3 ] || fail "not restored from its template"
}

test_names_are_checked_and_templates_deleted() {
  ! template save dev-scratch 'bad name' 2>/dev/null || fail "a name with a space was taken"
  ! template save dev-scratch blank 2>/dev/null || fail "a built-in template's name was taken"
  template is-template blank || fail "blank is not a template"
  ! template is-template missing || fail "a missing template is one"
  template delete watch
  [ ! -e "$TEMPLATE" ] && [ -z "$(template list)" ] || fail "the template was not deleted"
}

test_devws_own_templates_are_listed_beside_the_saved_ones() {
  [ "$(template builtins | cut -f1 | tr '\n' ' ')" = 'dev system agentic blank ' ] ||
    fail "devws's own templates are not listed: $(template builtins)"
  template builtins | awk -F '\t' 'NF != 2 || $2 == "" { exit 1 }' ||
    fail "a built-in template has no description: $(template builtins)"
}

test_devws_own_templates_are_listed_beside_the_saved_ones
test_an_empty_workspace_is_the_sidebar_and_a_shell
test_save_keeps_the_panes_commands_and_layout
test_a_template_opens_on_another_folder
test_a_template_workspace_is_restored_from_it
test_names_are_checked_and_templates_deleted

printf 'Workspace template tests passed.\n'
