"""Repeatable devws probes. Output contains metrics, never transcript contents."""

import argparse
import collections
import concurrent.futures
import datetime
import json
import math
import os
from pathlib import Path
import shlex
import shutil
import signal
import socket
import statistics
import subprocess
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parents[1] / "config/tmuxinator"


def distribution(values):
    ordered = sorted(values)
    return {"median": round(statistics.median(ordered), 2),
            "p95": round(ordered[math.ceil(len(ordered) * 0.95) - 1], 2),
            "max": round(ordered[-1], 2), "samples": len(ordered)}


def process_snapshot():
    output = subprocess.check_output(["ps", "-axo", "pid=,time=,rss=,comm="], text=True)
    result = {}
    for line in output.splitlines():
        identifier, cpu_time, resident, command = line.split(None, 3)
        days, separator, clock = cpu_time.rpartition("-")
        seconds = 0.0
        for part in (clock if separator else cpu_time).split(":"):
            seconds = seconds * 60 + float(part)
        seconds += int(days or 0) * 86400
        result[int(identifier)] = (seconds, int(resident), os.path.basename(command))
    return result


def cpu_groups(before, after, seconds):
    groups = collections.defaultdict(lambda: [0.0, 0, 0])
    for identifier, (cpu_time, resident, command) in after.items():
        if identifier not in before or before[identifier][2] != command:
            continue
        groups[command][0] += max(0, cpu_time - before[identifier][0]) * 100 / seconds
        groups[command][1] += resident
        groups[command][2] += 1
    return [{"command": command, "cpu_percent_one_core": round(values[0], 1),
             "resident_mb": round(values[1] / 1024, 1), "processes": values[2]}
            for command, values in sorted(groups.items(), key=lambda item: item[1][0], reverse=True)[:15]]


def live(seconds):
    before = process_snapshot()
    started = time.monotonic()
    latencies = []
    while time.monotonic() - started < seconds:
        called = time.monotonic()
        subprocess.run(["tmux", "display-message", "-p", "#{session_name}"],
                       stdout=subprocess.DEVNULL, check=True)
        latencies.append((time.monotonic() - called) * 1000)
        time.sleep(0.15)
    after = process_snapshot()
    elapsed = time.monotonic() - started
    return {"mode": "live", "seconds": round(elapsed, 2),
            "tmux_query_ms": distribution(latencies),
            "cpu": cpu_groups(before, after, elapsed),
            "limitation": "Excludes exited/new processes; query latency is not screen latency."}


def executable(path, contents):
    path.write_text(contents)
    path.chmod(0o755)


def hooks(samples, file_count):
    results = []
    with tempfile.TemporaryDirectory(prefix="devws-measure-") as temporary:
        root = Path(temporary)
        project = root / "project"
        project.mkdir()
        for number in range(file_count):
            (project / f"file-{number}.txt").write_text("example content\n" * 100)
        subprocess.run(["git", "init", "-q", str(project)], check=True)
        subprocess.run(["git", "-C", str(project), "add", "-A"], check=True)
        binary = root / "bin"
        binary.mkdir()
        executable(binary / "git", '#!/bin/bash\nprintf "%s\\n" "$*" >> "$PROBE_CALLS"\n'
                   + f'exec {shlex.quote(shutil.which("git"))} "$@"\n')
        # A real socket establishes eligibility; editor_open is never called
        # for this unchanged read-only fixture. No real tmux server is touched.
        editor_socket = socket.socket(socket.AF_UNIX)
        socket_path = root / "editor.sock"
        try:
            editor_socket.bind(str(socket_path))
            executable(binary / "tmux", '''#!/bin/bash
case "$*" in
  *@devws_editor_socket*@devws_follow*)
    if [ "$PROBE_LINKED" = 1 ]; then printf "%s|follow\\n" "$PROBE_SOCKET"; else echo "|follow"; fi ;;
  *session_name*) echo fixture ;;
  *@devws_editor_socket*) [ "$PROBE_LINKED" = 1 ] && echo "$PROBE_SOCKET" ;;
  *@devws_follow*) echo follow ;;
  *) exit 1 ;;
esac
''')
            payload = json.dumps({"cwd": str(project), "tool_name": "exec_command",
                                  "tool_input": {"command": "rg --files"}})
            for linked in [False, True]:
                for count in [1, 2, 4]:
                    durations, scan_counts, call_counts = [], [], []
                    for sample in range(samples + 1):
                        calls = root / "calls"
                        calls.write_text("")

                        def one_agent(number):
                            environment = dict(os.environ, PATH=f"{binary}:{os.environ['PATH']}",
                                               TMUX_PANE=f"%probe-{number}", TMPDIR=str(root),
                                               PROBE_CALLS=str(calls), PROBE_LINKED=str(int(linked)),
                                               PROBE_SOCKET=str(socket_path))
                            for phase in ["pre", "post"]:
                                subprocess.run([str(SCRIPTS / "agent_follow_hook.sh"), "codex", phase],
                                               input=payload, text=True, env=environment, check=True,
                                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

                        started = time.monotonic()
                        with concurrent.futures.ThreadPoolExecutor(max_workers=count) as workers:
                            list(workers.map(one_agent, range(count)))
                        if sample == 0:
                            continue
                        durations.append((time.monotonic() - started) * 1000)
                        commands = calls.read_text().splitlines()
                        scan_counts.append(sum(" add -A" in command for command in commands))
                        call_counts.append(len(commands))
                    results.append({"agents": count, "linked_editor": linked,
                                    "files": file_count, "pair_wall_ms": distribution(durations),
                                    "git_calls": call_counts, "repository_scans": scan_counts})
        finally:
            editor_socket.close()
    return {"mode": "hooks", "warmups_per_case": 1, "results": results}


def rendering(seconds, samples):
    ticker = subprocess.check_output(["tmux", "show-option", "-gqv", "@devws_header_ticker"], text=True).strip()
    if not ticker.isdigit():
        raise OSError("no running header ticker")
    identifier = int(ticker)
    command = subprocess.check_output(["ps", "-p", str(identifier), "-o", "command="], text=True)
    if "header_ticker.sh --run" not in command:
        raise OSError("header ticker identifier is stale")
    original_handler = signal.getsignal(signal.SIGTERM)

    def interrupted(signum, frame):
        raise KeyboardInterrupt

    results = []
    signal.signal(signal.SIGTERM, interrupted)
    try:
        for sample in range(samples):
            # Alternate order so a warming/changing workload does not always
            # favor the same condition. Only the ticker is held, never agents.
            for held in ([False, True] if sample % 2 == 0 else [True, False]):
                os.kill(identifier, signal.SIGSTOP if held else signal.SIGCONT)
                result = live(seconds)
                result["ticker_held"] = held
                results.append(result)
    finally:
        os.kill(identifier, signal.SIGCONT)
        signal.signal(signal.SIGTERM, original_handler)
    return {"mode": "rendering", "results": results,
            "limitation": "Live workload may vary; ticker hold includes header maintenance, not agent output."}


def tokens(samples, megabytes):
    results = []
    with tempfile.TemporaryDirectory(prefix="devws-token-probe-") as temporary:
        root = Path(temporary)
        transcript = root / "fixture.jsonl"
        padding = json.dumps({"type": "tool_output", "text": "x" * 8192}) + "\n"
        transcript.write_text(padding * max(1, int(megabytes * 1024 * 1024 / len(padding))))
        metrics = root / "metrics"
        timestamp = datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")
        record = json.dumps({"type": "event_msg", "timestamp": timestamp, "payload": {
            "type": "token_count", "info": {"total_token_usage": {"total_tokens": 1000},
                "last_token_usage": {"input_tokens": 100}, "model_context_window": 10000}}}) + "\n"
        with transcript.open("a") as output:
            output.write(record)
        for condition in ["uncached", "unchanged", "appended"]:
            environment = dict(os.environ, TMPDIR=str(root), DEVWS_TOKEN_CACHE_METRICS=str(metrics),
                               DEVWS_TOKEN_CACHE_DISABLE="1" if condition == "uncached" else "")
            durations = []
            for sample in range(samples + 1):
                if sample == 1:
                    metrics.write_text("")
                if condition == "appended":
                    with transcript.open("a") as output:
                        output.write(record)
                started = time.monotonic()
                subprocess.run([str(SCRIPTS / "header_tokens.sh"), "session", str(transcript)],
                               env=environment, stdout=subprocess.DEVNULL, check=True)
                if sample:
                    durations.append((time.monotonic() - started) * 1000)
            reads = metrics.read_text().splitlines() if metrics.exists() else []
            results.append({"condition": condition, "refresh_ms": distribution(durations),
                            "source_parses": len(reads),
                            "source_bytes": sum(int(line.split()[1]) for line in reads)})
        return {"mode": "tokens", "fixture_bytes": transcript.stat().st_size, "results": results}


def state(samples):
    results = []
    with tempfile.TemporaryDirectory(prefix="devws-state-probe-") as temporary:
        root = Path(temporary)
        binary = root / "bin"
        binary.mkdir()
        calls = root / "calls"
        socket_name = f"devws-state-probe-{os.getpid()}"
        real_tmux = shutil.which("tmux")
        executable(binary / "tmux", '#!/bin/bash\nprintf "%s\\n" "$*" >> "$PROBE_CALLS"\n'
                   + f'exec {shlex.quote(real_tmux)} -L {socket_name} -f /dev/null "$@"\n')
        environment = dict(os.environ, PATH=f"{binary}:{os.environ['PATH']}",
                           TMPDIR=str(root), PROBE_CALLS=str(calls), DEVWS_ENV_FILE=str(root / "settings"))
        (root / "settings").write_text("DEVWS_NOTIFY=0\n")

        def tmux(*arguments):
            return subprocess.check_output([str(binary / "tmux"), *arguments], env=environment, text=True).strip()

        def event(pane, value):
            subprocess.run([str(SCRIPTS / "agent_status.sh"), "probe", value, pane],
                           env=environment, stdout=subprocess.DEVNULL, check=True)

        try:
            tmux("new-session", "-d", "-s", "probe", "-x", "200", "-y", "60", "sleep 600")
            panes = [tmux("display-message", "-p", "-t", "probe:", "#{pane_id}")]
            for count in [1, 2, 4]:
                while len(panes) < count:
                    panes.append(tmux("split-window", "-t", "probe:", "-P", "-F", "#{pane_id}", "sleep 600"))
                    tmux("select-layout", "-t", "probe:", "tiled")
                for pane in panes:
                    tmux("set-option", "-p", "-t", pane, "@devws_role", "extra-agent")
                    event(pane, "working")
                for condition in ["identical", "transitions"]:
                    durations, writes, commands = [], [], []
                    for sample in range(samples + 1):
                        calls.write_text("")
                        started = time.monotonic()
                        if condition == "identical":
                            for pane in panes:
                                event(pane, "working")
                        else:
                            for value in ["waiting", "done", "working"]:
                                event(panes[0], value)
                        elapsed = (time.monotonic() - started) * 1000
                        if sample:
                            recorded = calls.read_text().splitlines()
                            durations.append(elapsed)
                            writes.append(sum(line.startswith("set-option") for line in recorded))
                            commands.append(len(recorded))
                    results.append({"agents": count, "condition": condition,
                                    "batch_ms": distribution(durations), "tmux_calls": commands,
                                    "option_writes": writes})
        finally:
            subprocess.run([real_tmux, "-L", socket_name, "kill-server"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return {"mode": "state", "results": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["live", "hooks", "rendering", "tokens", "state"],
                        help="rendering briefly holds the header ticker and restores it; agents keep running")
    parser.add_argument("--seconds", type=float, default=5)
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--files", type=int, default=1500)
    parser.add_argument("--log-mb", type=float, default=32)
    arguments = parser.parse_args()
    if arguments.seconds <= 0 or arguments.samples < 1 or arguments.files < 1 or arguments.log_mb <= 0:
        parser.error("seconds, samples and files must be positive")
    requirements = {"live": ["tmux", "ps"], "rendering": ["tmux", "ps"],
                    "hooks": ["git", "jq"], "tokens": ["perl", "jq"], "state": ["tmux"]}
    for command in requirements[arguments.mode]:
        if shutil.which(command) is None:
            parser.error(f"required tool is missing: {command}")
    try:
        if arguments.mode == "live":
            result = live(arguments.seconds)
        elif arguments.mode == "rendering":
            result = rendering(arguments.seconds, arguments.samples)
        elif arguments.mode == "tokens":
            result = tokens(arguments.samples, arguments.log_mb)
        elif arguments.mode == "state":
            result = state(arguments.samples)
        else:
            result = hooks(arguments.samples, arguments.files)
    except (OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"measure_load: {type(error).__name__}; check tool availability and permissions\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
