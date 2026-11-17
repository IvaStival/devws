//! The board's details text, formatted for reading: `Label: value` lines with
//! the label in bold, JSON laid out over lines and tinted, bullets and
//! numbered items each on a line of their own, `code` and **bold** marked.
//! The words stay the leader's and devAgentics's own; only the layout changes.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use serde_json::Value;

const LABEL: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const KEY: Style = Style::new().fg(Color::Indexed(110));
const STRING: Style = Style::new().fg(Color::Green);
const NUMBER: Style = Style::new().fg(Color::Yellow);
const PUNCTUATION: Style = Style::new().fg(Color::DarkGray);
const CODE: Style = Style::new().fg(Color::Cyan);
const BOLD: Style = Style::new().add_modifier(Modifier::BOLD);

/// A line longer than this reads better one sentence per line.
const LONG_LINE: usize = 160;

pub fn text(content: &str) -> Text<'static> {
    let mut lines = Vec::new();
    let source: Vec<&str> = content.lines().collect();
    let mut index = 0;
    while index < source.len() {
        let line = source[index];
        index += 1;
        if let Some((value, used)) = json_block(&source[index - 1..]) {
            json_lines(&value, 1, &mut lines);
            index += used - 1;
            continue;
        }
        for part in split_items(line) {
            if part.chars().count() > LONG_LINE {
                for sentence in sentences(&part) {
                    format_line(&sentence, &mut lines);
                }
            } else {
                format_line(&part, &mut lines);
            }
        }
    }
    Text::from(lines)
}

fn format_line(line: &str, lines: &mut Vec<Line<'static>>) {
    if let Some(value) = json(line.trim()) {
        json_lines(&value, 1, lines);
        return;
    }
    if let Some((label, value)) = label(line) {
        if let Some(json) = json(value.trim()) {
            lines.push(Line::from(Span::styled(format!("{label}:"), LABEL)));
            json_lines(&json, 1, lines);
            return;
        }
        let mut spans = vec![Span::styled(format!("{label}:"), LABEL), Span::raw(" ")];
        spans.extend(inline(value));
        lines.push(Line::from(spans));
        return;
    }
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let indent = &line[..line.len() - trimmed.len()];
        let mut spans = vec![Span::raw(format!("{indent}• "))];
        spans.extend(inline(rest));
        lines.push(Line::from(spans));
        return;
    }
    if let Some((number, rest)) = numbered(trimmed) {
        let mut spans = vec![Span::styled(format!("{number} "), BOLD)];
        spans.extend(inline(rest));
        lines.push(Line::from(spans));
        return;
    }
    lines.push(Line::from(inline(line)));
}

/// `Tool: x` → ("Tool", "x"): a short leading name, a capital first.
fn label(line: &str) -> Option<(&str, &str)> {
    let (label, value) = line.split_once(": ")?;
    let short = label.len() <= 28 && label.split_whitespace().count() <= 4;
    let named = label.chars().next().is_some_and(char::is_uppercase)
        && label
            .chars()
            .all(|character| character.is_alphanumeric() || " _-".contains(character));
    (short && named).then_some((label, value))
}

/// `1) x`, `(2) x`, `3. x` → the number as written, and the rest.
fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.trim_start_matches('(');
    let count = digits.chars().take_while(char::is_ascii_digit).count();
    if count == 0 || count > 2 {
        return None;
    }
    let after = &digits[count..];
    let closer = after
        .chars()
        .next()
        .filter(|closer| ").".contains(*closer))?;
    if line.starts_with('(') && closer != ')' {
        return None;
    }
    let end = line.len() - after.len() + 1;
    let rest = line[end..].strip_prefix(' ')?;
    Some((&line[..end], rest))
}

/// JSON already laid out over lines: a line that is only `{` or `[`, up to
/// the first line at its indent that closes it. The value and the lines used.
fn json_block(lines: &[&str]) -> Option<(Value, usize)> {
    let first = lines.first()?;
    let opener = first.trim();
    let closer = match opener {
        "{" => '}',
        "[" => ']',
        _ => return None,
    };
    let indent = &first[..first.len() - first.trim_start().len()];
    let end = lines.iter().position(|line| {
        line.strip_prefix(indent)
            .is_some_and(|rest| rest.trim_end().trim_end_matches(',') == closer.to_string())
    })?;
    let value = serde_json::from_str(&lines[..=end].join("\n")).ok()?;
    Some((value, end + 1))
}

/// An object or array, as JSON: a bare number or word is text.
fn json(text: &str) -> Option<Value> {
    if !(text.starts_with('{') || text.starts_with('[')) {
        return None;
    }
    serde_json::from_str(text).ok()
}

fn json_lines(value: &Value, depth: usize, lines: &mut Vec<Line<'static>>) {
    let mut spans = Vec::new();
    json_value(value, depth, &mut spans, lines);
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
}

/// Lays `value` out from the current line (`spans`), one entry per line,
/// pushing each finished line to `lines`.
fn json_value(
    value: &Value,
    depth: usize,
    spans: &mut Vec<Span<'static>>,
    lines: &mut Vec<Line<'static>>,
) {
    let indent = |depth: usize| Span::raw("  ".repeat(depth));
    let entries: Vec<(Option<&String>, &Value)> = match value {
        Value::Object(map) if !map.is_empty() => {
            map.iter().map(|(key, value)| (Some(key), value)).collect()
        }
        Value::Array(items) if !items.is_empty() && !items.iter().all(is_short) => {
            items.iter().map(|item| (None, item)).collect()
        }
        _ => {
            spans.extend(scalar(value));
            return;
        }
    };
    let (open, close) = if value.is_object() {
        ("{", "}")
    } else {
        ("[", "]")
    };
    spans.push(Span::styled(open, PUNCTUATION));
    lines.push(Line::from(std::mem::take(spans)));
    let last = entries.len() - 1;
    for (index, (key, item)) in entries.into_iter().enumerate() {
        spans.push(indent(depth));
        if let Some(key) = key {
            spans.push(Span::styled(format!("\"{key}\""), KEY));
            spans.push(Span::styled(": ", PUNCTUATION));
        }
        json_value(item, depth + 1, spans, lines);
        if index != last {
            spans.push(Span::styled(",", PUNCTUATION));
        }
        lines.push(Line::from(std::mem::take(spans)));
    }
    spans.push(indent(depth - 1));
    spans.push(Span::styled(close, PUNCTUATION));
}

/// Short lists of plain values stay on one line: `["lint", "typecheck"]`.
fn is_short(value: &Value) -> bool {
    !value.is_object() && !value.is_array() && value.to_string().len() <= 60
}

fn scalar(value: &Value) -> Vec<Span<'static>> {
    match value {
        Value::String(text) => vec![Span::styled(format!("\"{text}\""), STRING)],
        Value::Number(_) | Value::Bool(_) | Value::Null => {
            vec![Span::styled(value.to_string(), NUMBER)]
        }
        Value::Array(items) => {
            let mut spans = vec![Span::styled("[", PUNCTUATION)];
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::styled(", ", PUNCTUATION));
                }
                spans.extend(scalar(item));
            }
            spans.push(Span::styled("]", PUNCTUATION));
            spans
        }
        Value::Object(_) => vec![Span::styled("{}", PUNCTUATION)],
    }
}

/// `code` and **bold** in a line of text.
fn inline(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let code = rest.find('`');
        let bold = rest.find("**");
        let (start, marker, style) = match (code, bold) {
            (Some(code), Some(bold)) if bold < code => (bold, "**", BOLD),
            (Some(code), _) => (code, "`", CODE),
            (None, Some(bold)) => (bold, "**", BOLD),
            (None, None) => break,
        };
        let inner = &rest[start + marker.len()..];
        let Some(end) = inner.find(marker).filter(|end| *end > 0) else {
            break;
        };
        if start > 0 {
            spans.push(Span::raw(rest[..start].to_string()));
        }
        spans.push(Span::styled(inner[..end].to_string(), style));
        rest = &inner[end + marker.len()..];
    }
    if !rest.is_empty() {
        spans.push(Span::raw(rest.to_string()));
    }
    spans
}

/// `… 1) a 2) b` → `…`, `1) a`, `2) b`: numbered items written inline, each
/// on a line of its own, when the list starts at 1 and counts up.
fn split_items(line: &str) -> Vec<String> {
    let mut starts = Vec::new();
    let mut from = 0;
    for number in 1.. {
        let found = [format!("({number}) "), format!("{number}) ")]
            .iter()
            .filter_map(|marker| {
                line[from..]
                    .match_indices(marker.as_str())
                    .map(|(index, _)| from + index)
                    .find(|index| *index == 0 || line[..*index].ends_with(' '))
            })
            .min();
        match found {
            Some(index) => {
                starts.push(index);
                from = index + 1;
            }
            None => break,
        }
    }
    if starts.len() < 2 {
        return vec![line.to_string()];
    }
    let mut parts = Vec::new();
    let mut previous = 0;
    for start in starts {
        let part = line[previous..start].trim_end();
        if !part.is_empty() {
            parts.push(part.to_string());
        }
        previous = start;
    }
    parts.push(line[previous..].to_string());
    parts
}

/// A long paragraph, one sentence per line. A sentence ends at `. `, `? ` or
/// `! ` before a capital or a digit, so `e.g. x` and `1.2.0` stay whole.
fn sentences(line: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let characters: Vec<(usize, char)> = line.char_indices().collect();
    for window in characters.windows(3) {
        let [(_, end), (_, space), (next_index, next)] = window else {
            continue;
        };
        if ".?!".contains(*end) && *space == ' ' && (next.is_uppercase() || next.is_ascii_digit()) {
            sentences.push(line[start..*next_index].trim_end().to_string());
            start = *next_index;
        }
    }
    sentences.push(line[start..].to_string());
    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &Text) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    fn style_of(text: &Text, word: &str) -> Style {
        text.lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content.contains(word))
            .map(|span| span.style)
            .unwrap_or_else(|| panic!("no {word}"))
    }

    #[test]
    fn a_labelled_json_value_is_laid_out_and_tinted() {
        let formatted = text(
            r#"Tool: application_workflow
Arguments: {"action":"runner_configure","input":{"configuration":{"cpus":2,"source_paths":["web/**","docs/**"]}}}"#,
        );
        assert_eq!(
            plain(&formatted),
            [
                "Tool: application_workflow",
                "Arguments:",
                "{",
                "  \"action\": \"runner_configure\",",
                "  \"input\": {",
                "    \"configuration\": {",
                "      \"cpus\": 2,",
                "      \"source_paths\": [\"web/**\", \"docs/**\"]",
                "    }",
                "  }",
                "}",
            ]
        );
        assert_eq!(style_of(&formatted, "Tool:"), LABEL);
        assert_eq!(style_of(&formatted, "\"action\""), KEY);
        assert_eq!(style_of(&formatted, "\"runner_configure\""), STRING);
        assert_eq!(style_of(&formatted, "2"), NUMBER);
    }

    #[test]
    fn json_already_over_lines_is_tinted_too() {
        let formatted = text("Status:\n{\n  \"runner\": {\n    \"cpus\": 2\n  }\n}\nafter");
        assert_eq!(
            plain(&formatted),
            ["Status:", "{", "  \"runner\": {", "    \"cpus\": 2", "  }", "}", "after"]
        );
        assert_eq!(style_of(&formatted, "\"runner\""), KEY);
        assert_eq!(plain(&text("{\nnot json")), ["{", "not json"], "an unclosed brace is text");
    }

    #[test]
    fn inline_numbered_items_get_a_line_each() {
        let formatted = text("Two answers needed. 1) Approve p157? 2) Revision 4: A or B?");
        assert_eq!(
            plain(&formatted),
            [
                "Two answers needed.",
                "1) Approve p157?",
                "2) Revision 4: A or B?"
            ]
        );
        assert_eq!(style_of(&formatted, "1)"), BOLD);
        let not_a_list = text("Only step 2) was left.");
        assert_eq!(plain(&not_a_list), ["Only step 2) was left."]);
    }

    #[test]
    fn a_long_paragraph_reads_one_sentence_per_line() {
        let long = "The runner keeps the pinned image and adds six files for read-only evidence, e.g. the request classes. Milestone 1.2.0 stays as it is, and no backend code runs at all here. After consent I will apply it.";
        assert_eq!(
            plain(&text(long)),
            [
                "The runner keeps the pinned image and adds six files for read-only evidence, e.g. the request classes.",
                "Milestone 1.2.0 stays as it is, and no backend code runs at all here.",
                "After consent I will apply it.",
            ]
        );
        let short = "One. Two.";
        assert_eq!(
            plain(&text(short)),
            ["One. Two."],
            "a short line stays whole"
        );
    }

    #[test]
    fn bullets_code_and_bold_are_marked() {
        let formatted = text("- run `cargo test` first\n  * then **approve** it");
        assert_eq!(
            plain(&formatted),
            ["• run cargo test first", "  • then approve it"]
        );
        assert_eq!(style_of(&formatted, "cargo test"), CODE);
        assert_eq!(style_of(&formatted, "approve"), BOLD);
        assert_eq!(plain(&text("a `lone backtick")), ["a `lone backtick"]);
    }

    #[test]
    fn plain_text_and_sentences_with_colons_stay_as_they_are() {
        let formatted = text("Waiting: 2h 5m\nthe leader said: go on\n\nend");
        assert_eq!(
            plain(&formatted),
            ["Waiting: 2h 5m", "the leader said: go on", "", "end"]
        );
        assert_eq!(style_of(&formatted, "Waiting:"), LABEL);
        assert_ne!(style_of(&formatted, "the leader said"), LABEL);
    }
}
