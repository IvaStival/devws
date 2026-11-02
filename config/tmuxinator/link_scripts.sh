#!/usr/bin/env bash
# Keeps ~/.tmuxinator in step with the checkout: every devws script (and .env,
# and project-template/) there as a link to config/tmuxinator, so a script
# added by an update works without re-running ./install.sh, and a link to one
# that was removed goes. Run by `devws` on every start, `devws reload` and
# ./install.sh.
#
#   link_scripts.sh   prints "linked <name>" / "removed <name>" per change
#
# Only links into this checkout's config/tmuxinator are managed: a real file,
# or a link pointing elsewhere (another tmuxinator project), is never touched.

set -u

# The checkout's folder, through however many links this was run by.
self="$0"
while [ -L "$self" ]; do
  target="$(readlink "$self")"
  case "$target" in
    /*) self="$target" ;;
    *) self="$(dirname "$self")/$target" ;;
  esac
done
source_dir="$(cd "$(dirname "$self")" && pwd)"
dest="$HOME/.tmuxinator"

mkdir -p "$dest" || exit 1

for path in "$source_dir"/* "$source_dir/.env"; do
  [ -e "$path" ] || continue
  name="$(basename "$path")"
  [ -e "$dest/$name" ] || [ -L "$dest/$name" ] && continue
  ln -s "$path" "$dest/$name" && printf 'linked %s\n' "$name"
done

for link in "$dest"/* "$dest"/.env; do
  [ -L "$link" ] || continue
  target="$(readlink "$link")"
  case "$target" in
    "$source_dir"/*) [ -e "$target" ] || { rm -f "$link" && printf 'removed %s\n' "$(basename "$link")"; } ;;
  esac
done
exit 0
