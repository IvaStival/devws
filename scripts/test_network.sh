#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-network.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'devws network test failed: %s\n' "$*" >&2
  exit 1
}

mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home"
# A closed PATH makes the missing-dependency test independent of the host.
for program in awk basename bash cat dirname ln mkdir readlink rm ruby zsh; do
  ln -s "$(command -v "$program")" "$TEST_ROOT/bin/$program"
done

cat > "$TEST_ROOT/bin/uname" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$NETWORK_TEST_SYSTEM"
EOF
cat > "$TEST_ROOT/bin/route" <<'EOF'
#!/usr/bin/env bash
[ "$*" = "-n get -inet default" ] || exit 2
[ "${NETWORK_TEST_NO_ROUTE:-0}" = 0 ] || exit 1
printf '   route to: default\n  interface: en0\n'
EOF
cat > "$TEST_ROOT/bin/ifconfig" <<'EOF'
#!/usr/bin/env bash
[ "$*" = "en0" ] || exit 2
printf 'en0: flags=8863<UP,BROADCAST,RUNNING,SIMPLEX,MULTICAST> mtu 1500\n'
printf '\tinet6 fe80::1%%en0 prefixlen 64 scopeid 0x1\n'
[ "${NETWORK_TEST_NO_ADDRESS:-0}" = 0 ] || exit 0
printf '\tinet 192.168.23.52 netmask %s broadcast 192.168.23.255\n' "${NETWORK_TEST_MASK:-0xfffffe00}"
EOF
cat > "$TEST_ROOT/bin/ip" <<'EOF'
#!/usr/bin/env bash
case "$*" in
  '-4 route show default')
    printf 'default via 10.42.7.129 dev eth0 proto dhcp metric 100\n' ;;
  '-o -4 addr show dev eth0 scope global')
    printf '2: eth0    inet 10.42.7.202/25 brd 10.42.7.255 scope global eth0\n' ;;
  *) exit 2 ;;
esac
EOF
cat > "$TEST_ROOT/bin/nmap" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$NETWORK_TEST_SCAN_ARGS"
if [ "${NETWORK_TEST_SCAN_STATUS:-0}" != 0 ]; then
  printf 'Nmap discovery failed\n' >&2
  exit "$NETWORK_TEST_SCAN_STATUS"
fi
case " $* " in
  *' -PR '*)
    printf 'Nmap scan report for living-room.lan (192.168.22.8)\nHost is up.\n' ;;
esac
printf 'Nmap scan report for router.lan (192.168.22.1)\nHost is up.\n'
EOF
cat > "$TEST_ROOT/bin/id" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${NETWORK_TEST_UID:-501}"
EOF
cat > "$TEST_ROOT/bin/sudo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$NETWORK_TEST_SCAN_ARGS.sudo"
[ "${NETWORK_TEST_SUDO_STATUS:-0}" = 0 ] || exit "$NETWORK_TEST_SUDO_STATUS"
[ "$1" = -- ] && shift
exec "$@"
EOF
chmod +x "$TEST_ROOT/bin/"{uname,route,ifconfig,ip,nmap,id,sudo}

export HOME="$TEST_ROOT/home" PATH="$TEST_ROOT/bin"
export NETWORK_TEST_SCAN_ARGS="$TEST_ROOT/scan-args" NETWORK_TEST_SYSTEM=Darwin
unset TMUX TMUX_PANE
DEVWS="$ROOT/bin/devws"

expect_failure() {
  local expected_status="$1" expected_message="$2" output status=0
  shift 2
  output="$("$DEVWS" network "$@" 2>&1)" || status=$?
  [ "$status" = "$expected_status" ] || fail "expected exit $expected_status, got $status ($output)"
  [[ "$output" == *"$expected_message"* ]] || fail "expected '$expected_message', got '$output'"
}

output="$("$DEVWS" network --help)"
[[ "$output" == *'devws network devices'* ]] || fail "help did not list the devices command"
expect_failure 2 'Usage:'
expect_failure 2 'Usage:' bogus
expect_failure 2 'Usage:' devices extra
[ ! -e "$NETWORK_TEST_SCAN_ARGS" ] || fail "help or invalid arguments started discovery"

output="$("$DEVWS" network devices)"
[[ "$output" == *'en0'*'192.168.22.0/23'* ]] || fail "macOS scope was missing ($output)"
[[ "$output" == *'router.lan (192.168.22.1)'* ]] || fail "device output was missing ($output)"
[[ "$output" == *'living-room.lan'* ]] || fail "local devices without responding web ports were missed ($output)"
[ "$(cat "$NETWORK_TEST_SCAN_ARGS")" = $'-sn\n-PR\n-e\nen0\n192.168.22.0/23' ] || fail "unexpected macOS scan arguments"
[ -s "$NETWORK_TEST_SCAN_ARGS.sudo" ] || fail "ARP discovery did not request raw packet privileges"

output="$(NETWORK_TEST_SYSTEM=Linux "$DEVWS" network devices)"
[[ "$output" == *'eth0'*'10.42.7.128/25'* ]] || fail "Linux scope was missing ($output)"
[ "$(cat "$NETWORK_TEST_SCAN_ARGS")" = $'-sn\n-PR\n-e\neth0\n10.42.7.128/25' ] || fail "unexpected Linux scan arguments"

rm "$NETWORK_TEST_SCAN_ARGS.sudo"
NETWORK_TEST_UID=0 "$DEVWS" network devices >/dev/null
[ ! -e "$NETWORK_TEST_SCAN_ARGS.sudo" ] || fail "root discovery unnecessarily invoked sudo"

NETWORK_TEST_SCAN_STATUS=3 expect_failure 3 'Nmap discovery failed' devices
rm "$NETWORK_TEST_SCAN_ARGS"
NETWORK_TEST_SUDO_STATUS=1 expect_failure 1 'ARP discovery' devices
[ ! -e "$NETWORK_TEST_SCAN_ARGS" ] || fail "denied administrator access still started discovery"
NETWORK_TEST_NO_ROUTE=1 expect_failure 1 'default IPv4 route' devices
NETWORK_TEST_NO_ADDRESS=1 expect_failure 1 'IPv4 subnet' devices
NETWORK_TEST_MASK=0xff00ff00 expect_failure 1 'IPv4 subnet' devices
NETWORK_TEST_MASK=0x00000000 expect_failure 1 'IPv4 subnet' devices
rm "$TEST_ROOT/bin/sudo"
expect_failure 127 'sudo is required' devices
rm "$TEST_ROOT/bin/nmap"
expect_failure 127 'nmap' devices
[ ! -e "$NETWORK_TEST_SCAN_ARGS" ] || fail "an invalid network or missing dependency started discovery"

printf 'devws network tests passed.\n'
