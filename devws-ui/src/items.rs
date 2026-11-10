//! The lines a picker chooses from: what each shows (one field with
//! `--with-nth`, as fzf), what it is matched on, and the whole line it
//! answers with.

use crate::ansi;

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The input line as given: the answer.
    pub line: String,
    /// What is shown, colours kept.
    pub shown: String,
    /// What the query is matched against: `shown` without colours.
    pub plain: String,
}

/// Every non-empty line of `input`. With a delimiter and `with_nth` (from 1),
/// that field alone is shown; a line without it shows nothing.
pub fn parse(input: &str, delimiter: Option<&str>, with_nth: Option<usize>) -> Vec<Item> {
    input
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let shown = match (delimiter, with_nth) {
                (Some(delimiter), Some(field)) if field > 0 => line
                    .split(delimiter)
                    .nth(field - 1)
                    .unwrap_or("")
                    .to_string(),
                _ => line.to_string(),
            };
            Item {
                line: line.to_string(),
                plain: ansi::strip(&shown),
                shown,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_nth_shows_one_field_and_answers_with_the_line() {
        let items = parse("main (here)\t/repo\nfeat/x\t/repo-x\n", Some("\t"), Some(1));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].shown, "main (here)");
        assert_eq!(items[0].line, "main (here)\t/repo");
        assert_eq!(items[1].shown, "feat/x");
    }

    #[test]
    fn colours_are_shown_but_not_matched() {
        let items = parse("\x1b[2mdim\x1b[0m name\n\n", None, None);
        assert_eq!(items.len(), 1, "empty lines are not choices");
        assert_eq!(items[0].plain, "dim name");
        assert_eq!(items[0].shown, "\x1b[2mdim\x1b[0m name");
    }
}
