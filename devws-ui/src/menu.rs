//! The devws menu: its actions in tabs. ←/→ (or Tab) change tab, typing
//! searches every tab (each row tagged with its tab), Enter runs, Esc
//! closes. The answer names the tab it was on, for the next menu to open on.

use crate::ansi;
use crate::filter::Filter;
use crate::input::Key;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub tab: String,
    pub action: String,
    pub label: String,
    pub shortcut: String,
}

/// Rows from "tab<TAB>action<TAB>label<TAB>shortcut" lines; a line missing
/// its action is not a row.
pub fn parse(input: &str) -> Vec<Row> {
    input
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let tab = fields.next()?.to_string();
            let action = fields
                .next()
                .filter(|action| !action.is_empty())?
                .to_string();
            Some(Row {
                tab,
                action,
                label: fields.next().unwrap_or("").to_string(),
                shortcut: fields.next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// "workspace" → "Workspace", as the tab bar shows it.
pub fn title(tab: &str) -> String {
    let mut characters = tab.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

pub struct Menu {
    pub rows: Vec<Row>,
    /// In the order they first appear.
    pub tabs: Vec<String>,
    pub current_tab: usize,
    pub query: String,
    /// Indexes into `rows`, as shown.
    pub shown: Vec<usize>,
    pub cursor: usize,
    filter: Filter,
}

#[derive(Debug, PartialEq)]
pub enum Step {
    Continue,
    /// The tab it was on, and the action chosen (none for Esc).
    Done {
        tab: String,
        action: Option<String>,
    },
}

impl Menu {
    pub fn new(rows: Vec<Row>, start_tab: Option<&str>) -> Self {
        let mut tabs: Vec<String> = Vec::new();
        for row in &rows {
            if !tabs.contains(&row.tab) {
                tabs.push(row.tab.clone());
            }
        }
        let current_tab = start_tab
            .and_then(|tab| tabs.iter().position(|known| known == tab))
            .unwrap_or(0);
        let mut menu = Self {
            rows,
            tabs,
            current_tab,
            query: String::new(),
            shown: Vec::new(),
            cursor: 0,
            filter: Filter::new(),
        };
        menu.refresh();
        menu
    }

    pub fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// What a row shows: its label, tagged with its tab while searching.
    pub fn label_of(&self, index: usize) -> String {
        let row = &self.rows[index];
        if self.searching() {
            format!("{} · {}", title(&row.tab), row.label)
        } else {
            row.label.clone()
        }
    }

    fn refresh(&mut self) {
        if self.searching() {
            let texts: Vec<String> = (0..self.rows.len())
                .map(|index| ansi::strip(&self.label_of(index)))
                .collect();
            let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
            self.shown = self.filter.matches(&texts, &self.query);
        } else {
            let tab = self.tabs.get(self.current_tab);
            self.shown = (0..self.rows.len())
                .filter(|&index| Some(&self.rows[index].tab) == tab)
                .collect();
        }
        self.cursor = 0;
    }

    fn tab_name(&self) -> String {
        self.tabs.get(self.current_tab).cloned().unwrap_or_default()
    }

    pub fn press(&mut self, key: Key) -> Step {
        match key {
            Key::Cancel => {
                return Step::Done {
                    tab: self.tab_name(),
                    action: None,
                };
            }
            Key::Accept => {
                if let Some(&index) = self.shown.get(self.cursor) {
                    return Step::Done {
                        tab: self.tab_name(),
                        action: Some(self.rows[index].action.clone()),
                    };
                }
            }
            Key::Right | Key::Toggle => self.switch_tab(1),
            Key::Left | Key::BackToggle => self.switch_tab(-1),
            Key::Up => self.move_by(-1),
            Key::Down => self.move_by(1),
            Key::PageUp => self.move_by(-10),
            Key::PageDown => self.move_by(10),
            Key::Edit(edit) => {
                if edit.apply(&mut self.query) {
                    self.refresh();
                }
            }
            // A click runs the row, as the menu always did.
            Key::Click(row) => {
                if row < self.shown.len() {
                    self.cursor = row;
                    return self.press(Key::Accept);
                }
            }
            Key::ClickTab(column) => {
                if let Some(tab) = self.tab_at(column) {
                    self.current_tab = tab;
                    self.refresh();
                }
            }
            Key::None => {}
        }
        Step::Continue
    }

    /// The tab bar is " Title " per tab, one space apart.
    pub fn tab_at(&self, column: u16) -> Option<usize> {
        let mut start = 0u16;
        for (index, tab) in self.tabs.iter().enumerate() {
            let width = title(tab).chars().count() as u16 + 2;
            if column >= start && column < start + width {
                return Some(index);
            }
            start += width + 1;
        }
        None
    }

    fn switch_tab(&mut self, delta: isize) {
        let count = self.tabs.len();
        if count == 0 {
            return;
        }
        self.current_tab = (self.current_tab as isize + delta).rem_euclid(count as isize) as usize;
        // With something typed, the search across every tab stays.
        if !self.searching() {
            self.refresh();
        }
    }

    fn move_by(&mut self, delta: isize) {
        let count = self.shown.len();
        if count == 0 {
            return;
        }
        self.cursor = if delta.abs() == 1 {
            (self.cursor as isize + delta).rem_euclid(count as isize) as usize
        } else {
            (self.cursor as isize + delta).clamp(0, count as isize - 1) as usize
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::typed;

    fn menu(start: Option<&str>) -> Menu {
        Menu::new(
            parse(
                "workspace\tgithub\t⎇ Open a workspace\t\n\
                 workspace\tdetach\t⏏ Detach\tC-b d\n\
                 git\tcommit\t✓ Commit (agent)\tC-b g\n\
                 git\tpush\t↑ Push / PR\tC-b G\n\
                 help\tguide\t? Start here\t\n",
            ),
            start,
        )
    }

    fn actions(menu: &Menu) -> Vec<&str> {
        menu.shown
            .iter()
            .map(|&index| menu.rows[index].action.as_str())
            .collect()
    }

    #[test]
    fn tabs_keep_their_order_and_the_start_tab_is_chosen() {
        let menu = menu(Some("git"));
        assert_eq!(menu.tabs, vec!["workspace", "git", "help"]);
        assert_eq!(actions(&menu), vec!["commit", "push"]);
        assert_eq!(
            Menu::new(menu.rows.clone(), Some("gone")).current_tab,
            0,
            "an unknown tab starts at the first"
        );
    }

    #[test]
    fn left_and_right_wrap_across_tabs() {
        let mut menu = menu(None);
        menu.press(Key::Left);
        assert_eq!(actions(&menu), vec!["guide"]);
        menu.press(Key::Right);
        menu.press(Key::Toggle);
        assert_eq!(actions(&menu), vec!["commit", "push"]);
    }

    #[test]
    fn typing_searches_every_tab_tagged_with_its_own() {
        let mut menu = menu(Some("help"));
        for key in typed("push") {
            menu.press(key);
        }
        assert_eq!(actions(&menu), vec!["push"]);
        assert_eq!(menu.label_of(menu.shown[0]), "Git · ↑ Push / PR");
        assert_eq!(
            menu.press(Key::Accept),
            Step::Done {
                tab: "help".into(),
                action: Some("push".into())
            }
        );
    }

    #[test]
    fn esc_answers_the_tab_it_was_on() {
        let mut menu = menu(None);
        menu.press(Key::Right);
        assert_eq!(
            menu.press(Key::Cancel),
            Step::Done {
                tab: "git".into(),
                action: None
            }
        );
    }

    #[test]
    fn clicks_choose_a_tab_or_run_a_row() {
        let mut menu = menu(None);
        // " Workspace " is columns 0–10, then a space, then " Git " at 12–16.
        menu.press(Key::ClickTab(13));
        assert_eq!(menu.tabs[menu.current_tab], "git");
        assert_eq!(
            menu.press(Key::ClickTab(11)),
            Step::Continue,
            "the gap is no tab"
        );
        assert_eq!(
            menu.press(Key::Click(1)),
            Step::Done {
                tab: "git".into(),
                action: Some("push".into())
            }
        );
    }
}
