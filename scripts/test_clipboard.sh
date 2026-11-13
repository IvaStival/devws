#!/usr/bin/env bash
# Clipboard routing without access to the real desktop clipboard.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python3 - "$ROOT" <<'PY'
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix="devws-clipboard-") as directory:
    work = Path(directory)
    binaries = work / "bin"
    binaries.mkdir()
    for program in ("bash", "cat", "wc", "head", "base64", "tr", "awk"):
        (binaries / program).symlink_to(shutil.which(program))
    tmux = binaries / "tmux"
    tmux.write_text('''#!/bin/bash
case "$1" in
  show-option) case "$3" in
    @copy_use_osc52_fallback) printf '%s' "${FALLBACK:-on}" ;;
  esac ;;
  load-buffer) printf '%s\\n' "$*" > "$RESULT.args"; cat > "$RESULT" ;;
  *) exit 1 ;;
esac
''')
    tmux.chmod(0o755)
    result = work / "result"
    stale_tty = work / "old-ssh-tty"
    stale_tty.write_bytes(b"untouched")
    environment = dict(os.environ, PATH=str(binaries), RESULT=str(result),
                       SSH_TTY=str(stale_tty), DISPLAY="", TMUX="test")
    payload = b"quotes ' and %s and \\ backslash\n\n"
    def copy(**settings):
        subprocess.run([str(binaries / "bash"), str(root / "config/tmux/yank.sh")],
                       input=payload, env=dict(environment, **settings), check=True)
    copy()
    assert result.read_bytes() == payload, "clipboard changed input bytes"
    assert (work / "result.args").read_text().strip() == "load-buffer -w -"
    assert stale_tty.read_bytes() == b"untouched", "copy used stale SSH_TTY"
    result.unlink()
    copy(FALLBACK="off")
    assert not result.exists(), "disabled fallback still copied"
    pbcopy = binaries / "pbcopy"
    pbcopy.write_text('#!/bin/bash\ncat > "$RESULT"\n')
    pbcopy.chmod(0o755)
    copy(FALLBACK="off")
    assert result.read_bytes() == payload, "local backend lost priority or bytes"
print("Clipboard tests passed")
PY
