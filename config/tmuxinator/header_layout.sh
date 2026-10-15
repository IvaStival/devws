#!/usr/bin/env bash
# Lays out the header's three rows of groups at a client's width. tmux can only
# align a status line left, centre or right, so the groups are composed here:
# each group as wide as its widest row, the space left over shared evenly
# between the groups with a │ in the middle of each gap, the rows exactly
# <width> wide. When the groups do not fit, the widest gives way first.
#
#  CPU ▓▓░░  27%     │     claude  31.6M today …     │     codex   0 today …     │     +18°C  Rain
#  MEM ▓▓▓▓  80%     │     session ▓▓▓▓▓▓░░  62%…    │     session ▓▓▓▓▓▓▓▓…     │     Curitiba
#  DSK ▓▓▓▓  98%     │     week    ▓▓▓▓▓▓▓▓  88%…    │     week    ▓▓▓▓▓▓▓▓…     │     humidity 94%
#
#   header_layout.sh <width> <groups>   3 rows per group on stdin
#
# Rows may carry tmux styles (#[...]), which take no width. DEVWS_COLOR_RULE
# colors the separators. The ghost is drawn apart (header_ticker.sh).

set -u

width="${1:?Usage: header_layout.sh <width> <groups>}"
groups="${2:?Usage: header_layout.sh <width> <groups>}"
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

# The room for the groups: a margin each side, and a │ in each gap.
gaps=$((groups - 1))
available=$((width - 2 - gaps))
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
# Too wide for the room (with at least 2 columns per gap): the widest group
# gives up a column at a time, so the narrow ones stay whole.
while [ $((used + 2 * gaps)) -gt "$available" ] && [ "$used" -gt "$groups" ]; do
  widest=0
  for ((group = 1; group < groups; group++)); do
    [ "${columns[group]}" -gt "${columns[widest]}" ] && widest="$group"
  done
  columns[widest]=$((columns[widest] - 1))
  used=$((used - 1))
done

# The leftover, split between the gaps (the first ones take what does not
# divide evenly); with one group, the group takes it.
gap_widths=()
leftover=$((available - used))
[ "$leftover" -lt 0 ] && leftover=0
if [ "$gaps" -eq 0 ]; then
  columns[0]=$((columns[0] + leftover))
fi
for ((gap = 0; gap < gaps; gap++)); do
  gap_width=$((leftover / gaps))
  [ "$gap" -lt $((leftover % gaps)) ] && gap_width=$((gap_width + 1))
  gap_widths+=("$gap_width")
done

rule="#[fg=$rule_color]│#[default]"
for row in 0 1 2; do
  line=" "
  for ((group = 0; group < groups; group++)); do
    line+="$(fit "${rows[group * 3 + row]}" "${columns[group]}")"
    if [ "$group" -lt "$gaps" ]; then
      gap_width="${gap_widths[group]}"
      line+="$(spaces $((gap_width / 2)))$rule$(spaces $((gap_width - gap_width / 2)))"
    fi
  done
  printf '%s \n' "$line"
done
