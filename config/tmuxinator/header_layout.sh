#!/usr/bin/env bash
# Lays out the header's three rows of groups at a client's width. tmux can only
# align a status line left, centre or right, so the groups are composed here:
# each group as wide as its widest row, at least two spaces each side of the │
# between groups, the room left over shared between the gaps so the groups
# spread across the width, the rows exactly <width> wide. When the groups do
# not fit, the widest gives way first.
#
#  CPU ▓▓░░  27%  │  claude 5h ▓▓▓▓░░░░░░  42% …  │  ↓ 2.4 MB/s  │  +18°C  Rain
#  MEM ▓▓▓▓  80%  │  work   5h ▓░░░░░░░░░   9% …  │  Wi-Fi …     │  Curitiba
#  DSK ▓▓▓▓  98%  │  month $588 at API prices     │  192.168…    │  humidity 94%
#
#   header_layout.sh <width> <groups> [--optional-first] [--span-last-two]
#                                                     3 rows per group on stdin
#
# --optional-first: the first group (the clock) is left out, not cut, when the
# groups would not fit whole with it.
# --span-last-two: 3 more rows follow the groups' (the background jobs,
# job.sh), drawn across the last two groups' room — theirs and the │ between —
# instead of them. The groups are laid out as without them, so the others stay
# where they were.
#
# Rows may carry tmux styles (#[...]), which take no width. DEVWS_COLOR_RULE
# colors the separators. The ghost is drawn apart (header_ticker.sh).

set -u

width="${1:?Usage: header_layout.sh <width> <groups>}"
groups="${2:?Usage: header_layout.sh <width> <groups>}"
optional_first=""
span_last_two=""
for option in "${@:3}"; do
  case "$option" in
    --optional-first) optional_first="$option" ;;
    --span-last-two) span_last_two="$option" ;;
  esac
done
rule_color="${DEVWS_COLOR_RULE:-colour238}"

rows=()
while IFS= read -r row || [ -n "$row" ]; do
  rows+=("$row")
done
span_rows=()
if [ -n "$span_last_two" ]; then
  span_rows=("${rows[@]:groups * 3:3}")
  rows=("${rows[@]:0:groups * 3}")
  while [ "${#span_rows[@]}" -lt 3 ]; do
    span_rows+=("")
  done
fi
while [ "${#rows[@]}" -lt $((groups * 3)) ]; do
  rows+=("")
done

# Visible characters only: styles take no width, and "##" (a "#", as tmux
# shows it) takes one — it is never the start of a style.
visible_length() {
  local text="$1" length=0 index=0 style
  while [ "$index" -lt "${#text}" ]; do
    if [ "${text:index:2}" = "##" ]; then
      length=$((length + 1))
      index=$((index + 2))
      continue
    fi
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
    if [ "${text:index:2}" = "##" ]; then
      [ "$visible" -ge "$columns" ] && break
      out+="##"
      visible=$((visible + 1))
      index=$((index + 2))
      continue
    fi
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
  printf '%s\n' "${rows[@]:3}" ${span_last_two:+"${span_rows[@]}"} |
    "$0" "$width" $((groups - 1)) $span_last_two
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

# What is left over spreads the groups across the width: shared between the
# gaps (the first ones take what does not divide evenly), half each side of
# the │. A single group keeps it after itself.
leftover=$((available - used))
[ "$leftover" -lt 0 ] && leftover=0
trailing="$leftover"
separators=()
for ((separator = 0; separator < gaps; separator++)); do
  extra=$((leftover / gaps))
  [ "$separator" -lt $((leftover % gaps)) ] && extra=$((extra + 1))
  separators+=("$gap$(spaces $((extra / 2)))#[fg=$rule_color]│#[default]$gap$(spaces $((extra - extra / 2)))")
  trailing=0
done

for row in 0 1 2; do
  line=" "
  for ((group = 0; group < groups; group++)); do
    if [ -n "$span_last_two" ] && [ "$groups" -ge 2 ] && [ "$group" -eq $((groups - 2)) ]; then
      line+="$(fit "${span_rows[row]}" \
        $((columns[group] + $(visible_length "${separators[group]}") + columns[group + 1])))"
      break
    fi
    line+="$(fit "${rows[group * 3 + row]}" "${columns[group]}")"
    [ "$group" -lt "$gaps" ] && line+="${separators[group]}"
  done
  printf '%s%s \n' "$line" "$(spaces "$trailing")"
done
