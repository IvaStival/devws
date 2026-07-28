#!/usr/bin/env bash
#
# Detects a Mermaid fenced code block in a Markdown file and, if found, opens
# a self-contained CDN-backed HTML preview of the whole file in the system
# browser. Glow cannot render diagrams graphically, so this covers what it
# can't. Called from shell/devws.zsh (devws md) and config/lvim/config.lua
# (<leader>mp), alongside — not instead of — the Glow preview.

set -euo pipefail

usage() {
  printf 'usage: render_mermaid_preview.sh <file.md>\n' >&2
}

if (( $# != 1 )); then
  usage
  exit 2
fi

markdown_file="$1"

if [[ ! -f "$markdown_file" || ! -r "$markdown_file" ]]; then
  printf 'render_mermaid_preview: file not found or unreadable: %s\n' "$markdown_file" >&2
  exit 2
fi

if ! grep -Eiq '^[[:space:]]*(`{3,}|~{3,})[[:space:]]*mermaid[[:space:]]*$' "$markdown_file"; then
  exit 0
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/devws-mermaid.XXXXXX")"
html_file="$tmp_dir/preview.html"
markdown_b64="$(base64 < "$markdown_file")"

cat > "$html_file" <<HTML
<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>$(basename "$markdown_file") — Mermaid preview</title>
<script src="https://cdn.jsdelivr.net/npm/marked@12/marked.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js"></script>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, sans-serif; max-width: 900px; margin: 2rem auto; padding: 0 1rem; line-height: 1.5; }
  pre code { display: block; background: #f4f4f4; padding: 0.75rem; overflow-x: auto; }
</style>
</head>
<body>
<div id="content">Loading…</div>
<script>
const markdownBase64 = \`$markdown_b64\`;
const bytes = Uint8Array.from(atob(markdownBase64.replace(/\s+/g, '')), (c) => c.charCodeAt(0));
const source = new TextDecoder('utf-8').decode(bytes);

const container = document.getElementById('content');
container.innerHTML = marked.parse(source);

container.querySelectorAll('code.language-mermaid').forEach((codeEl) => {
  const div = document.createElement('div');
  div.className = 'mermaid';
  div.textContent = codeEl.textContent;
  codeEl.parentElement.replaceWith(div);
});

mermaid.initialize({ startOnLoad: false });
mermaid.run();
</script>
</body>
</html>
HTML

if command -v open >/dev/null 2>&1; then
  open "$html_file"
elif command -v xdg-open >/dev/null 2>&1; then
  (xdg-open "$html_file" >/dev/null 2>&1 &)
else
  printf 'render_mermaid_preview: no browser opener (open/xdg-open) found; open manually: %s\n' "$html_file" >&2
fi
