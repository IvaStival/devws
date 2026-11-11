#!/usr/bin/env bash
# The header's ghost, at 20 frames a second: each frame is three tmux-styled
# rows, 12 columns wide.
#
#   header_pet.sh <idle|working|waiting|done> <tick>   one frame
#   header_pet.sh <idle|working|waiting|done>          the mood's whole cycle,
#                                                      frame after frame
#
# devws's pixel ghost (its icon): a round head with two tall dark eyes and a
# smile between them, its arms out, its body tapering into a tail that
# curls off to the right, shaded darker under it. 16 x 9 pixels drawn with
# sextants (the 2 x 3 blocks of Unicode's Symbols for Legacy Computing, 🬀 to 🬻, with ▌ ▐ █),
# each character with its own two colors. It moves a whole character at a
# time, which keeps its shape. Terminals that draw these blocks themselves
# (Ghostty, iTerm2, kitty, WezTerm) show it whole; elsewhere the font must
# have them. The mood follows the workspace's agent:
#   idle     eyes shut, breathing, a z rising beside it
#   working  drifting across the lane and back, looking where it goes,
#            blinking now and then
#   waiting  wide-eyed and open-mouthed, shaking, a blinking "!"
#   done     happy eyes, swaying, a blinking heart, for 3 s of every 15; at
#            rest the rest of the time
# The body takes the mood's color, its edge a darker one that still shows on
# a dark terminal (the hem and tail are the body's, so the ghost never looks
# cut off there): DEVWS_COLOR_DIM (idle), DEVWS_COLOR_GHOST (working),
# DEVWS_COLOR_WARN (waiting), DEVWS_COLOR_OK (done).
# header_ticker.sh draws the cycles once and plays them back.

set -u

mood="${1:-}"
tick="${2:-}"

# Frames in a cycle, a multiple of every motion's period in that mood; the
# body's color and its edge's.
case "$mood" in
  idle) period=120 body="${DEVWS_COLOR_DIM:-colour245}" edge=colour242 ;;
  working) period=720 body="${DEVWS_COLOR_GHOST:-colour214}" edge=colour172 ;;
  waiting) period=180 body="${DEVWS_COLOR_WARN:-yellow}" edge=colour178 ;;
  done) period=300 body="${DEVWS_COLOR_OK:-green}" edge=colour34 ;;
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

awk -v mood="$mood" -v first="$first" -v last="$last" -v body="$body" -v edge="$edge" '
  # The pixel (a palette letter, "." for none) of the lane at <column> <row>.
  function pixel(column, row,    sprite_column) {
    sprite_column = column - x
    if (sprite_column < 0 || sprite_column >= width) return "."
    return substr(sprite[row], sprite_column + 1, 1)
  }
  # One character of 2 x 3 pixels, in two colors. With pixels of none (the
  # edge of the ghost) the terminal background is one: the character is
  # its most common color on it, unless an eye or the mouth is there,
  # which shows whole, its pixels of none filled in. Else the two most
  # common, the rarer drawn as the block so an eye or the mouth shows whole.
  function cell(column, row,    k, letter, seen, empty, ink, paper, mask, best, best_count, second, second_count) {
    delete seen
    empty = 0
    for (k = 0; k < 6; k++) {
      part[k] = pixel(column * 2 + k % 2, row * 3 + int(k / 2))
      if (part[k] != ".") seen[part[k]]++
      else empty++
    }
    best = ""; best_count = 0; second = ""; second_count = 0
    for (letter in seen) {
      if (seen[letter] > best_count) { second = best; second_count = best_count; best = letter; best_count = seen[letter] }
      else if (seen[letter] > second_count) { second = letter; second_count = seen[letter] }
    }
    if (best == "") return ""
    mask = 0
    if (empty && !("K" in seen)) {
      for (k = 0; k < 6; k++) if (part[k] != ".") mask += 2 ^ k
      return styled("#[fg=" color[best] ",bg=default]", glyph[mask])
    }
    ink = (second != "" ? second : best)
    paper = (second != "" ? best : "")
    for (k = 0; k < 6; k++) if (part[k] == ink) mask += 2 ^ k
    return styled("#[fg=" color[ink] ",bg=" (paper != "" ? color[paper] : "default") "]", glyph[mask])
  }
  # The character, after its style when that differs from the one before.
  function styled(style, character) {
    if (style == last_style) return character
    last_style = style
    return style character
  }
  # <row> with <letters> laid over it from <column>.
  function put(row, column, letters) {
    sprite[row] = substr(sprite[row], 1, column) letters substr(sprite[row], column + length(letters) + 1)
  }
  # The face, moved <look> pixels aside (-1 left, 0 ahead, 1 right): <eyes>
  # open (two tall blocks), shut (their bottoms) or happy (their tops);
  # <mouth> a smile between them, or open.
  function face(eyes, look, mouth,    left, right) {
    left = 4 + look; right = 9 + look
    if (eyes != "shut") { put(2, left, "KK"); put(2, right, "KK") }
    if (eyes != "happy") { put(3, left, "KK"); put(3, right, "KK") }
    if (mouth == "open") put(5, 6 + look, "KKK")
    put(4, 6 + look, "KKK")
  }
  function frame(t,    row, column, line, drawn, sweep, rise, eyes, look, mouth) {
    note = ""
    sprite[0] = "...OOOOOOOOO...."
    sprite[1] = "..OOOOOOOOOOO..."
    sprite[2] = "..OOOOOOOOOOO..."
    sprite[3] = "OOOOOOOOOOOOOOO."
    sprite[4] = ".OOOOOOOOOOOOO.."
    sprite[5] = "...OOOOOOOOOO..."
    sprite[6] = ".....OOOOOOO...."
    sprite[7] = ".......OOOOOO.OO"
    sprite[8] = "..........DDOOO."
    eyes = "open"; look = 0; mouth = "smile"
    if (mood == "idle") {
      x = 4
      # Breathing: the head a little narrower every other second.
      if (int(t / 20) % 2) sprite[0] = ".....OOOOO......"
      eyes = "shut"
      rise = int(t / 10) % 4
      if (rise < 3) { note = (rise == 2 ? "Z" : "z"); note_row = 2 - rise; note_column = 11 }
    } else if (mood == "working") {
      sweep = int(t / 3) % 8
      x = (sweep <= 4 ? sweep : 8 - sweep) * 2
      # Looking where it goes: right on the way out, left on the way back.
      look = (sweep < 4 ? 1 : -1)
      if (t % 80 < 3) eyes = "shut"
    } else if (mood == "waiting") {
      x = 4 + (int(t / 3) % 2) * 2
      mouth = "open"
      if (int(t / 10) % 2 == 0) { note = "!"; note_row = 0; note_column = 11 }
    } else {
      # Happy for the first 3 s of every 15, then at rest: done lasts until
      # the next prompt, and every change is a redraw.
      eyes = "happy"
      note = "♥"; note_column = 11
      if (t % 300 < 60) {
        x = 2 + (int(t / 12) % 2) * 2
        note_row = int(t / 12) % 2
      } else {
        x = 2
        note_row = int(t / 20) % 2
      }
    }
    face(eyes, look, mouth)
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
    # The block for each pattern of the six pixels, bit k the pixel k
    # (left to right, top to bottom).
    split(" |🬀|🬁|🬂|🬃|🬄|🬅|🬆|🬇|🬈|🬉|🬊|🬋|🬌|🬍|🬎|🬏|🬐|🬑|🬒|🬓|▌|🬔|🬕|🬖|🬗|🬘|🬙|🬚|🬛|🬜|🬝|🬞|🬟|🬠|🬡|🬢|🬣|🬤|🬥|🬦|🬧|▐|🬨|🬩|🬪|🬫|🬬|🬭|🬮|🬯|🬰|🬱|🬲|🬳|🬴|🬵|🬶|🬷|🬸|🬹|🬺|🬻|█", list, "|")
    for (k = 0; k < 64; k++) glyph[k] = list[k + 1]
    width = 16
    color["O"] = body; color["D"] = edge; color["K"] = "colour232"
    for (t = first; t <= last; t++) frame(t)
  }
'
