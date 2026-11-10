//! Narrowing a list by what is typed: fuzzy, best match first (nucleo, the
//! matcher fzf-like tools use); nothing typed keeps the list's own order.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

pub struct Filter {
    matcher: Matcher,
}

impl Filter {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// The indexes of `texts` matching `query`, best first; ties keep their
    /// order.
    pub fn matches(&mut self, texts: &[&str], query: &str) -> Vec<usize> {
        if query.trim().is_empty() {
            return (0..texts.len()).collect();
        }
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut buffer = Vec::new();
        let mut scored: Vec<(u32, usize)> = texts
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                pattern
                    .score(Utf32Str::new(text, &mut buffer), &mut self.matcher)
                    .map(|score| (score, index))
            })
            .collect();
        scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        scored.into_iter().map(|(_, index)| index).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_typed_keeps_every_item_in_order() {
        assert_eq!(Filter::new().matches(&["b", "a", "c"], ""), vec![0, 1, 2]);
    }

    #[test]
    fn the_best_match_comes_first_and_others_are_dropped() {
        let texts = ["Run this branch", "Stop", "Switch running branch"];
        let found = Filter::new().matches(&texts, "srb");
        assert_eq!(found.first(), Some(&2), "found: {found:?}");
        assert!(!found.contains(&1), "Stop matched srb: {found:?}");
    }

    #[test]
    fn lower_case_ignores_case() {
        assert_eq!(
            Filter::new().matches(&["Push / PR", "pull"], "pu"),
            vec![0, 1]
        );
    }
}
