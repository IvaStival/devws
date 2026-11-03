#!/usr/bin/env bash
# The devws guide (docs/guide/ in the checkout), paged with Glow: the menu's
# Help tab and `devws help`.
#
#   guide.sh [page]   shows the page (no page: README, the start); an unknown
#                     page lists the pages and exits 2
#   guide.sh --pages  lists the pages

set -u

# The checkout, through however many links this was run by (~/.tmuxinator
# links here).
self="$0"
while [ -L "$self" ]; do
  target="$(readlink "$self")"
  case "$target" in
    /*) self="$target" ;;
    *) self="$(dirname "$self")/$target" ;;
  esac
done
guide="$(cd "$(dirname "$self")/../../docs/guide" 2>/dev/null && pwd)" || {
  printf 'devws help: the guide is missing (docs/guide in the devws checkout)\n' >&2
  exit 1
}

pages() {
  local page
  for page in "$guide"/*.md; do
    basename "$page" .md
  done
}

case "${1:-}" in
  --pages) pages; exit 0 ;;
esac

page="${1:-README}"
if [ ! -f "$guide/$page.md" ]; then
  printf 'devws help: no page "%s". Pages: %s\n' "$page" "$(pages | grep -vx README | paste -sd ' ' -)" >&2
  exit 2
fi
if ! command -v glow >/dev/null 2>&1; then
  printf 'devws help: Glow is required; run ./install.sh --deps (or read %s)\n' "$guide/$page.md" >&2
  exit 1
fi
exec glow -p "$guide/$page.md"
