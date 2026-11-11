#!/usr/bin/env bash
# On-demand IPv4 host discovery for the interface carrying the default route.

set -uo pipefail

usage() {
  printf 'Usage: devws network devices\n'
}

if [ "$#" -eq 1 ] && [ "$1" = --help ]; then
  usage
  exit 0
fi
if [ "$#" -ne 1 ] || [ "$1" != devices ]; then
  usage >&2
  exit 2
fi

if ! nmap_command="$(command -v nmap)"; then
  printf 'devws network: nmap is required. Run brew install nmap on macOS or install nmap with your Linux package manager.\n' >&2
  exit 127
fi

system="$(uname -s)"
default_interface() {
  case "$system" in
    Darwin) route -n get -inet default | awk '$1 == "interface:" { print $2 }' ;;
    Linux) ip -4 route show default | awk '
      $1 == "default" && !found {
        for (field = 2; field < NF; field++) {
          if ($field == "dev") { print $(field + 1); found = 1; break }
        }
      }' ;;
  esac
}

interface_address() {
  case "$system" in
    Darwin) ifconfig "$interface" | awk '$1 == "inet" && $3 == "netmask" && !found++ { print $2, $4 }' ;;
    Linux) ip -o -4 addr show dev "$interface" scope global |
      awk '$3 == "inet" && $5 != "peer" && !found++ { sub(/\//, " ", $4); print $4 }' ;;
  esac
}

case "$system" in
  Darwin|Linux) ;;
  *) printf 'devws network: device discovery is supported on macOS and Linux.\n' >&2; exit 1 ;;
esac

if ! interface="$(default_interface)" || [ -z "$interface" ]; then
  printf 'devws network: no default IPv4 route with a network interface.\n' >&2
  exit 1
fi

if ! address_fields="$(interface_address)"; then
  address_fields=''
fi
read -r address mask <<<"$address_fields"

# macOS reports a hexadecimal netmask, Linux a prefix length.
if ! subnet="$(ruby -ripaddr -e '
  address, mask = ARGV
  if mask.match?(/\A0x[[:xdigit:]]{8}\z/)
    mask = IPAddr.new(mask.to_i(16), Socket::AF_INET).to_s
  elsif !mask.match?(/\A[0-9]{1,2}\z/)
    exit 1
  end
  begin
    network = IPAddr.new("#{address}/#{mask}", Socket::AF_INET)
    exit 1 unless (1..31).cover?(network.prefix)
    puts "#{network}/#{network.prefix}"
  rescue ArgumentError
    exit 1
  end
' -- "$address" "$mask")"; then
  printf 'devws network: could not determine the connected IPv4 subnet on %s.\n' "$interface" >&2
  exit 1
fi

printf 'Discovering devices with ARP on %s (%s)\n' "$interface" "$subnet"
if [ "$(id -u)" -eq 0 ]; then
  exec "$nmap_command" -sn -PR -e "$interface" "$subnet"
fi

printf 'ARP discovery needs administrator access. Enter your system password if prompted.\n' >&2
if ! command -v sudo >/dev/null 2>&1; then
  printf 'devws network: sudo is required for ARP discovery.\n' >&2
  exit 127
fi
# Elevate only Nmap, keeping devws configuration under the caller's home.
exec sudo -- "$nmap_command" -sn -PR -e "$interface" "$subnet"
