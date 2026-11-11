//! A question from a list (what picker.sh asks): type to narrow it, move,
//! tick several with `--multi`, Enter answers, Esc goes back. Exit codes are
//! fzf's, which devws's scripts already read: 0 answered, 1 nothing matched,
//! 130 Esc.

use crate::filter::Filter;
use crate::input::Key;
use crate::items::Item;
use std::collections::BTreeSet;

pub struct Picker {
    pub items: Vec<Item>,
    pub query: String,
    /// Indexes into `items`, in the order shown.
    pub matches: Vec<usize>,
    /// Position in `matches`.
    pub cursor: usize,
    /// Indexes into `items`.
    pub ticked: BTreeSet<usize>,
    pub multi: bool,
    filter: Filter,
}

#[derive(Debug, PartialEq)]
pub enum Step {
    Continue,
    Done(Answer),
}

#[derive(Debug, PartialEq)]
pub struct Answer {
    pub lines: Vec<String>,
    pub code: i32,
}

impl Picker {
    pub fn new(items: Vec<Item>, multi: bool, select_all: bool) -> Self {
        let ticked = if multi && select_all {
            (0..items.len()).collect()
        } else {
            BTreeSet::new()
        };
        let mut picker = Self {
            items,
            query: String::new(),
            matches: Vec::new(),
            cursor: 0,
            ticked,
            multi,
            filter: Filter::new(),
        };
        picker.refilter();
        picker
    }

    fn refilter(&mut self) {
        let texts: Vec<&str> = self.items.iter().map(|item| item.plain.as_str()).collect();
        self.matches = self.filter.matches(&texts, &self.query);
        self.cursor = 0;
    }

    /// The item under the cursor, if anything matches.
    pub fn current(&self) -> Option<usize> {
        self.matches.get(self.cursor).copied()
    }

    pub fn press(&mut self, key: Key, print_query: bool) -> Step {
        match key {
            Key::Cancel => return Step::Done(self.answer(130, Vec::new(), print_query)),
            Key::Accept => return Step::Done(self.accept(print_query)),
            Key::Up => self.move_by(-1),
            Key::Down => self.move_by(1),
            Key::PageUp => self.move_by(-10),
            Key::PageDown => self.move_by(10),
            Key::Toggle if self.multi => {
                if let Some(index) = self.current() {
                    if !self.ticked.remove(&index) {
                        self.ticked.insert(index);
                    }
                    self.move_by(1);
                }
            }
            Key::Edit(edit) => {
                if edit.apply(&mut self.query) {
                    self.refilter();
                }
            }
            // One click picks the row, as in the menu; a multi-select ticks
            // it instead, Enter answering with what is ticked.
            Key::Click(row) if row < self.matches.len() => {
                self.cursor = row;
                if !self.multi {
                    return Step::Done(self.accept(print_query));
                }
                let index = self.matches[row];
                if !self.ticked.remove(&index) {
                    self.ticked.insert(index);
                }
            }
            _ => {}
        }
        Step::Continue
    }

    fn move_by(&mut self, delta: isize) {
        let count = self.matches.len();
        if count == 0 {
            return;
        }
        // One step wraps around, as the menu's tabs do; a page stops at the end.
        self.cursor = if delta.abs() == 1 {
            (self.cursor as isize + delta).rem_euclid(count as isize) as usize
        } else {
            (self.cursor as isize + delta).clamp(0, count as isize - 1) as usize
        };
    }

    /// The ticked items in their input order, or the one under the cursor.
    fn accept(&self, print_query: bool) -> Answer {
        let chosen: Vec<usize> = if self.multi && !self.ticked.is_empty() {
            self.ticked.iter().copied().collect()
        } else {
            self.current().into_iter().collect()
        };
        let code = if chosen.is_empty() { 1 } else { 0 };
        self.answer(code, chosen, print_query)
    }

    fn answer(&self, code: i32, chosen: Vec<usize>, print_query: bool) -> Answer {
        let mut lines = Vec::new();
        if print_query && code != 130 {
            lines.push(self.query.clone());
        }
        lines.extend(
            chosen
                .into_iter()
                .map(|index| self.items[index].line.clone()),
        );
        Answer { lines, code }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::typed;
    use crate::items::parse;

    fn picker(input: &str, multi: bool, select_all: bool) -> Picker {
        Picker::new(parse(input, Some("\t"), Some(1)), multi, select_all)
    }

    fn press_all(picker: &mut Picker, keys: Vec<Key>) {
        for key in keys {
            assert_eq!(picker.press(key, false), Step::Continue);
        }
    }

    #[test]
    fn enter_answers_with_the_whole_line_under_the_cursor() {
        let mut picker = picker("main\t/a\nfeat/x\t/b\n", false, false);
        press_all(&mut picker, vec![Key::Down]);
        assert_eq!(
            picker.press(Key::Accept, false),
            Step::Done(Answer {
                lines: vec!["feat/x\t/b".into()],
                code: 0
            })
        );
    }

    #[test]
    fn a_click_answers_with_its_row_and_in_a_multi_select_ticks_it() {
        let mut one = picker("a\tA\nb\tB\nc\tC\n", false, false);
        match one.press(Key::Click(1), false) {
            Step::Done(answer) => assert_eq!(answer.lines, ["b\tB"]),
            Step::Continue => panic!("one click picks"),
        }
        assert!(
            matches!(one.press(Key::Click(9), false), Step::Continue),
            "below the rows"
        );

        let mut many = picker("a\tA\nb\tB\nc\tC\n", true, false);
        assert!(matches!(many.press(Key::Click(2), false), Step::Continue));
        assert!(matches!(many.press(Key::Click(0), false), Step::Continue));
        assert!(
            matches!(many.press(Key::Click(2), false), Step::Continue),
            "unticks"
        );
        match many.press(Key::Accept, false) {
            Step::Done(answer) => assert_eq!(answer.lines, ["a\tA"]),
            Step::Continue => panic!("Enter answers"),
        }
    }

    #[test]
    fn one_step_wraps_around() {
        let mut picker = picker("a\nb\nc\n", false, false);
        press_all(&mut picker, vec![Key::Up]);
        assert_eq!(picker.current(), Some(2));
        press_all(&mut picker, vec![Key::Down]);
        assert_eq!(picker.current(), Some(0));
    }

    #[test]
    fn typing_narrows_and_a_miss_answers_one() {
        let mut picker = picker("alpha\nbeta\n", false, false);
        press_all(&mut picker, typed("bet"));
        assert_eq!(picker.matches, vec![1]);
        press_all(&mut picker, typed("zzz"));
        assert_eq!(
            picker.press(Key::Accept, false),
            Step::Done(Answer {
                lines: vec![],
                code: 1
            })
        );
    }

    #[test]
    fn esc_answers_130_and_nothing() {
        let mut picker = picker("a\n", false, false);
        assert_eq!(
            picker.press(Key::Cancel, true),
            Step::Done(Answer {
                lines: vec![],
                code: 130
            })
        );
    }

    #[test]
    fn multi_starts_all_ticked_and_tab_unticks_in_input_order() {
        let mut picker = picker("a\nb\nc\n", true, true);
        press_all(&mut picker, vec![Key::Down, Key::Toggle]);
        assert_eq!(
            picker.press(Key::Accept, false),
            Step::Done(Answer {
                lines: vec!["a".into(), "c".into()],
                code: 0
            })
        );
    }

    #[test]
    fn print_query_puts_what_was_typed_first() {
        let mut picker = picker("~/code\n", false, false);
        press_all(&mut picker, typed("/tmp/new"));
        assert_eq!(
            picker.press(Key::Accept, true),
            Step::Done(Answer {
                lines: vec!["/tmp/new".into()],
                code: 1
            })
        );
        let mut picker = picker_with("~/code\n");
        assert_eq!(
            picker.press(Key::Accept, true),
            Step::Done(Answer {
                lines: vec!["".into(), "~/code".into()],
                code: 0
            })
        );
    }

    fn picker_with(input: &str) -> Picker {
        Picker::new(parse(input, None, None), false, false)
    }
}
