//! The colours devws's scripts put in labels, headers and footers (SGR
//! escapes such as `\x1b[2m`), turned into styled spans, and stripped for
//! matching.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// `text` without its escape sequences.
pub fn strip(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\x1b' {
            plain.push(character);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // Parameters and intermediates, up to the final byte (@ to ~).
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        }
    }
    plain
}

/// One line of `text` as spans, starting from `base` and following its SGR
/// escapes; other escapes are dropped.
pub fn line(text: &str, base: Style) -> Line<'static> {
    let mut spans = Vec::new();
    let mut style = base;
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\x1b' {
            current.push(character);
            continue;
        }
        if chars.peek() != Some(&'[') {
            continue;
        }
        chars.next();
        let mut parameters = String::new();
        let mut last = ' ';
        for next in chars.by_ref() {
            if ('@'..='~').contains(&next) {
                last = next;
                break;
            }
            parameters.push(next);
        }
        if last != 'm' {
            continue;
        }
        if !current.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut current), style));
        }
        style = apply(style, base, &parameters);
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, style));
    }
    Line::from(spans)
}

/// `style` after one SGR sequence's parameters; a reset goes back to `base`.
fn apply(mut style: Style, base: Style, parameters: &str) -> Style {
    let codes: Vec<u16> = if parameters.is_empty() {
        vec![0]
    } else {
        parameters
            .split(';')
            .map(|code| code.parse().unwrap_or(0))
            .collect()
    };
    let mut index = 0;
    while index < codes.len() {
        let code = codes[index];
        match code {
            0 => style = base,
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            30..=37 => style = style.fg(Color::Indexed((code - 30) as u8)),
            39 => style = style.fg(base.fg.unwrap_or(Color::Reset)),
            40..=47 => style = style.bg(Color::Indexed((code - 40) as u8)),
            49 => style = style.bg(base.bg.unwrap_or(Color::Reset)),
            90..=97 => style = style.fg(Color::Indexed((code - 90 + 8) as u8)),
            100..=107 => style = style.bg(Color::Indexed((code - 100 + 8) as u8)),
            38 | 48 => {
                let (color, used) = extended_color(&codes[index + 1..]);
                if let Some(color) = color {
                    style = if code == 38 {
                        style.fg(color)
                    } else {
                        style.bg(color)
                    };
                }
                index += used;
            }
            _ => {}
        }
        index += 1;
    }
    style
}

/// A 256-colour (`5;n`) or true-colour (`2;r;g;b`) colour, and how many
/// parameters it took.
fn extended_color(rest: &[u16]) -> (Option<Color>, usize) {
    match rest {
        [5, index, ..] => (Some(Color::Indexed(*index as u8)), 2),
        [2, red, green, blue, ..] => (Some(Color::Rgb(*red as u8, *green as u8, *blue as u8)), 4),
        _ => (None, rest.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_colour_and_keeps_the_text() {
        assert_eq!(
            strip("\x1b[1;30;46m Workspace \x1b[0m x\x1b[2mC-b d\x1b[0m"),
            " Workspace  xC-b d"
        );
    }

    #[test]
    fn line_follows_sgr_and_resets_to_the_base() {
        let base = Style::default();
        let line = line("a\x1b[2mdim\x1b[0mb\x1b[38;2;1;2;3mrgb\x1b[38;5;24mx", base);
        let styles: Vec<(String, Style)> = line
            .spans
            .iter()
            .map(|span| (span.content.to_string(), span.style))
            .collect();
        assert_eq!(
            styles,
            vec![
                ("a".to_string(), base),
                ("dim".to_string(), base.add_modifier(Modifier::DIM)),
                ("b".to_string(), base),
                ("rgb".to_string(), base.fg(Color::Rgb(1, 2, 3))),
                ("x".to_string(), base.fg(Color::Indexed(24))),
            ]
        );
    }
}
