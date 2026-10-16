#!/usr/bin/env bash
# Lays out the header's three rows of groups at a client's width. tmux can only
# align a status line left, centre or right, so the groups are composed here:
# each group as wide as its widest row, two spaces each side of the │ between
# groups, the room left over after the last group, the rows exactly <width>
# wide. When the groups do not fit, the widest gives way first.
#
#  CPU ▓▓░░  27%  │  claude 5h ▓▓▓▓░░░░░░  42% …  │  ↓ 2.4 MB/s  │  +18°C  Rain
#  MEM ▓▓▓▓  80%  │  work   5h ▓░░░░░░░░░   9% …  │  Wi-Fi …     │  Curitiba
#  DSK ▓▓▓▓  98%  │  month $588 at API prices     │  192.168…    │  humidity 94%
#
#   header_layout.sh <width> <groups> [--optional-first]   3 rows per group on stdin
#
# --optional-first: the first group (the clock) is left out, not cut, when the
# groups would not fit whole with it.
#
# Rows may carry tmux styles (#[...]), which take no width. DEVWS_COLOR_RULE
# colors the separators. The ghost is drawn apart (header_ticker.sh).

set -u

width="${1:?Usage: header_layout.sh <width> <groups>}"
groups="${2:?Usage: header_layout.sh <width> <groups>}"
optional_first="${3:-}"
rule_color="${DEVWS_COLOR_RULE:-colour238}"

rows=()
while IFS= read -r row || [ -n "$row" ]; do
  rows+=("$row")
done
while [ "${#rows[@]}" -lt $((groups * 3)) ]; do
  rows+=("")
done

# Visible characters only: styles take no width.
visible_length() {
  local text="$1" length=0 index=0 style
  while [ "$index" -lt "${#text}" ]; do
    if [ "${text:index:2}" = "#[" ]; then
      style="${text:index}"
      style="${style%%]*}]"
      index=$((index + ${#style}))
      continue
    fi
    length=$((length + 1))
    index=$((index + 1))
  done
  printf '%s' "$length"
}

# Prints the text cut or padded to exactly <columns> visible characters,
# copying styles through, then resets the style for what follows.
fit() {
  local text="$1" columns="$2" out="" visible=0 index=0 style
  while [ "$index" -lt "${#text}" ]; do
    if [ "${text:index:2}" = "#[" ]; then
      style="${text:index}"
      style="${style%%]*}]"
      out+="$style"
      index=$((index + ${#style}))
      continue
    fi
    [ "$visible" -ge "$columns" ] && break
    out+="${text:index:1}"
    visible=$((visible + 1))
    index=$((index + 1))
  done
  while [ "$visible" -lt "$columns" ]; do
    out+=' '
    visible=$((visible + 1))
  done
  printf '%s#[default]' "$out"
}

spaces() {
  printf '%*s' "$1" ''
}

# The room for the groups: a margin each side, and "  │  " in each gap.
gap="  "
gaps=$((groups - 1))
available=$((width - 2 - gaps * (2 * ${#gap} + 1)))
[ "$available" -lt "$groups" ] && available="$groups"

columns=()
used=0
for ((group = 0; group < groups; group++)); do
  column=0
  for row in 0 1 2; do
    length="$(visible_length "${rows[group * 3 + row]}")"
    [ "$length" -gt "$column" ] && column="$length"
  done
  columns+=("$column")
  used=$((used + column))
done
if [ "$optional_first" = --optional-first ] && [ "$groups" -gt 1 ] &&
  [ "$used" -gt "$available" ]; then
  printf '%s\n' "${rows[@]:3}" | "$0" "$width" $((groups - 1))
  exit
fi
# Too wide for the room: the widest group gives up a column at a time, so the
# narrow ones stay whole.
while [ "$used" -gt "$available" ] && [ "$used" -gt "$groups" ]; do
  widest=0
  for ((group = 1; group < groups; group++)); do
    [ "${columns[group]}" -gt "${columns[widest]}" ] && widest="$group"
  done
  columns[widest]=$((columns[widest] - 1))
  used=$((used - 1))
done

# What is left over goes after the last group.
leftover=$((available - used))
[ "$leftover" -lt 0 ] && leftover=0

rule="#[fg=$rule_color]│#[default]"
for row in 0 1 2; do
  line=" "
  for ((group = 0; group < groups; group++)); do
    line+="$(fit "${rows[group * 3 + row]}" "${columns[group]}")"
    [ "$group" -lt "$gaps" ] && line+="$gap$rule$gap"
  done
  printf '%s%s \n' "$line" "$(spaces "$leftover")"
done
