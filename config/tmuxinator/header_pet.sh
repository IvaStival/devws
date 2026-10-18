#!/usr/bin/env bash
# The header's ghost, at 20 frames a second: each frame is three tmux-styled
# rows, 12 columns wide.
#
#   header_pet.sh <idle|working|waiting|done> <tick>   one frame
#   header_pet.sh <idle|working|waiting|done>          the mood's whole cycle,
#                                                      frame after frame
#
# A pixel-art ghost, after the arcade ones (a dome, straight sides, a pointed
# hem that flips between two shapes as it moves): 12 x 6 pixels drawn with
# quarter blocks (▘▝▖▗▀▄▌▐▚▞▛▜▙▟█), 2 x 2 pixels a character, each character
# with its own two colors, so the ghost has a body, white eyes and dark pupils.
# It moves a whole character at a time, which keeps its shape. The mood
# follows the workspace's agent:
#   idle     eyes shut, breathing, its hem flipping slowly, a z rising beside it
#   working  drifting across the lane and back, looking where it goes, its
#            hem flipping, blinking now and then
#   waiting  wide-eyed and open-mouthed, shaking, a blinking "!"
#   done     happy eyes, swaying, its hem flipping fast, a blinking heart,
#            for 3 s of every 15; at rest the rest of the time
# The body takes the mood's color: DEVWS_COLOR_DIM (idle), DEVWS_COLOR_GHOST
# (working), DEVWS_COLOR_WARN (waiting), DEVWS_COLOR_OK (done).
# header_ticker.sh draws the cycles once and plays them back.

set -u

mood="${1:-}"
tick="${2:-}"

# Frames in a cycle, a multiple of every motion's period in that mood; and the
# body's color.
case "$mood" in
  idle) period=120 body="${DEVWS_COLOR_DIM:-colour245}" ;;
  working) period=720 body="${DEVWS_COLOR_GHOST:-colour173}" ;;
  waiting) period=180 body="${DEVWS_COLOR_WARN:-yellow}" ;;
  done) period=300 body="${DEVWS_COLOR_OK:-green}" ;;
  *)
    printf 'Unknown ghost mood: %s (idle, working, waiting, done)\n' "$mood" >&2
    exit 2
    ;;
esac

if [ -n "$tick" ]; then
  first=$((tick % period))
  last="$first"
else
  first=0
  last=$((period - 1))
fi

awk -v mood="$mood" -v first="$first" -v last="$last" -v body="$body" '
  function pick(list, index_,    items) {
    split(list, items, " ")
    return items[index_ % length(items) + 1]
  }
  # The pointed hem, in the two shapes of the arcade ghosts, flipped between.
  function hem(phase) {
    return pick(".O.O.OO.O.O. .OO.O..O.OO.", phase)
  }
  # A row of the ghost across both eyes, each eye 2 pixels.
  function eyes_row(left, right) {
    return ".O" left "OOOO" right "O."
  }
  # The pixel (a palette letter, "." for none) of the lane at <column> <row>.
  function pixel(column, row,    sprite_column) {
    sprite_column = column - x
    if (sprite_column < 0 || sprite_column > 11) return "."
    return substr(sprite[row], sprite_column + 1, 1)
  }
  # One character of 2 x 2 pixels, in their two most common colors.
  function cell(column, row,    k, letter, seen, ink, paper, mask, best, best_count, second, second_count) {
    delete seen
    for (k = 0; k < 4; k++) {
      quarter[k] = pixel(column * 2 + k % 2, row * 2 + int(k / 2))
      if (quarter[k] != ".") seen[quarter[k]]++
    }
    best = ""; best_count = 0; second = ""; second_count = 0
    for (letter in seen) {
      if (seen[letter] > best_count) { second = best; second_count = best_count; best = letter; best_count = seen[letter] }
      else if (seen[letter] > second_count) { second = letter; second_count = seen[letter] }
    }
    if (best == "") return ""
    # Drawn as the block: the rarer color, so a pupil or an eye shows whole.
    ink = (second != "" ? second : best)
    paper = (second != "" ? best : "")
    mask = 0
    for (k = 0; k < 4; k++) if (quarter[k] == ink) mask += weight[k]
    return styled("#[fg=" color[ink] ",bg=" (paper != "" ? color[paper] : "default") "]", quads[mask])
  }
  # The character, after its style when that differs from the one before.
  function styled(style, character) {
    if (style == last_style) return character
    last_style = style
    return style character
  }
  function frame(t,    row, column, line, drawn, mouth, phase, head, top, bottom, sweep, rise) {
    note = ""
    phase = int(t / 3)
    head = "...OOOOOO..."
    mouth = ".OOOOOOOOOO."
    if (mood == "idle") {
      x = 6
      phase = int(t / 10)
      # Breathing: the head a little narrower every other second.
      if (int(t / 20) % 2) head = "....OOOO...."
      top = eyes_row("OO", "OO"); bottom = eyes_row("KK", "KK")
      rise = int(t / 10) % 4
      if (rise < 3) { note = (rise == 2 ? "Z" : "z"); note_row = 2 - rise; note_column = 10 }
    } else if (mood == "working") {
      sweep = int(t / 3) % 12
      x = (sweep <= 6 ? sweep : 12 - sweep) * 2
      top = eyes_row("WW", "WW")
      # Looking where it goes: right on the way out, left on the way back.
      if (sweep < 6) bottom = eyes_row("WK", "WK")
      else bottom = eyes_row("KW", "KW")
      if (t % 80 < 3) { top = eyes_row("OO", "OO"); bottom = eyes_row("KK", "KK") }
    } else if (mood == "waiting") {
      x = 6 + (int(t / 3) % 2) * 2
      top = eyes_row("WW", "WW"); bottom = eyes_row("WW", "WW")
      mouth = ".OOOOKKOOOO."
      if (int(t / 10) % 2 == 0) { note = "!"; note_row = 0; note_column = 11 }
    } else {
      # Happy for the first 3 s of every 15, then at rest: done lasts until
      # the next prompt, and every change is a redraw.
      top = eyes_row("KK", "KK"); bottom = eyes_row("OO", "OO")
      note = "♥"; note_column = 11
      if (t % 300 < 60) {
        x = 6 + (int(t / 12) % 2) * 2
        phase = int(t / 2)
        note_row = int(t / 12) % 2
      } else {
        x = 6
        phase = int(t / 20)
        note_row = int(t / 20) % 2
      }
    }
    sprite[0] = head; sprite[1] = ".OOOOOOOOOO."
    sprite[2] = top; sprite[3] = bottom
    sprite[4] = mouth; sprite[5] = hem(phase)
    for (row = 0; row < 3; row++) {
      line = ""
      last_style = ""
      for (column = 0; column < 12; column++) {
        drawn = cell(column, row)
        if (drawn != "") line = line drawn
        else if (note != "" && row == note_row && column == note_column) line = line styled("#[fg=" body ",bg=default]", note)
        else line = line styled("#[fg=default,bg=default]", " ")
      }
      print line "#[default]"
    }
  }
  BEGIN {
    split(" |▗|▖|▄|▝|▐|▞|▟|▘|▚|▌|▙|▀|▜|▛|█", list, "|")
    for (k = 0; k < 16; k++) quads[k] = list[k + 1]
    # Quarters: top left, top right, bottom left, bottom right.
    weight[0] = 8; weight[1] = 4; weight[2] = 2; weight[3] = 1
    color["O"] = body; color["W"] = "colour255"; color["K"] = "colour232"
    for (t = first; t <= last; t++) frame(t)
  }
'
