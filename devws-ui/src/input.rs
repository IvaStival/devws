//! What a key press means to a list: the same keys fzf gives devws today
//! (arrows, Tab, Enter, Esc, Ctrl-U…), and editing the query.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Accept,
    Cancel,
    Up,
    Down,
    PageUp,
    PageDown,
    Left,
    Right,
    /// Tab: ticks in a multi-select, next tab in the menu.
    Toggle,
    /// Shift-Tab: the previous tab in the menu.
    BackToggle,
    Edit(Edit),
    /// A click on the list's row (from its top).
    Click(usize),
    /// A click on the menu's tab bar, at that column.
    ClickTab(u16),
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    Insert(char),
    Backspace,
    DeleteWord,
    Clear,
}

impl Edit {
    /// Changes `query`; whether it changed.
    pub fn apply(self, query: &mut String) -> bool {
        let before = query.len();
        match self {
            Edit::Insert(character) => query.push(character),
            Edit::Backspace => {
                query.pop();
            }
            Edit::DeleteWord => {
                let kept = query
                    .trim_end()
                    .rfind(' ')
                    .map(|space| space + 1)
                    .unwrap_or(0);
                query.truncate(kept);
            }
            Edit::Clear => query.clear(),
        }
        query.len() != before || matches!(self, Edit::Insert(_))
    }
}

/// A key press as a `Key`; releases and repeats of nothing are `None`.
pub fn from_key(event: KeyEvent) -> Key {
    if event.kind == KeyEventKind::Release {
        return Key::None;
    }
    let control = event.modifiers.contains(KeyModifiers::CONTROL);
    match event.code {
        KeyCode::Enter => Key::Accept,
        KeyCode::Esc => Key::Cancel,
        KeyCode::Char('c' | 'g' | 'q') if control => Key::Cancel,
        KeyCode::Up => Key::Up,
        KeyCode::Char('p' | 'k') if control => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Char('n' | 'j') if control => Key::Down,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Tab => Key::Toggle,
        KeyCode::BackTab => Key::BackToggle,
        KeyCode::Backspace => Key::Edit(Edit::Backspace),
        KeyCode::Char('h') if control => Key::Edit(Edit::Backspace),
        KeyCode::Char('w') if control => Key::Edit(Edit::DeleteWord),
        KeyCode::Char('u') if control => Key::Edit(Edit::Clear),
        KeyCode::Char(character) if !control => Key::Edit(Edit::Insert(character)),
        _ => Key::None,
    }
}

/// The keys typing `text` sends, for tests.
#[cfg(test)]
pub fn typed(text: &str) -> Vec<Key> {
    text.chars()
        .map(|character| Key::Edit(Edit::Insert(character)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Key {
        from_key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn fzf_keys_mean_the_same() {
        assert_eq!(key(KeyCode::Char('c'), KeyModifiers::CONTROL), Key::Cancel);
        assert_eq!(key(KeyCode::Char('n'), KeyModifiers::CONTROL), Key::Down);
        assert_eq!(
            key(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Key::Edit(Edit::Clear)
        );
        assert_eq!(
            key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            Key::Edit(Edit::Insert('A'))
        );
        assert_eq!(key(KeyCode::BackTab, KeyModifiers::SHIFT), Key::BackToggle);
    }

    #[test]
    fn editing_the_query() {
        let mut query = String::from("run this br");
        assert!(Edit::DeleteWord.apply(&mut query));
        assert_eq!(query, "run this ");
        assert!(Edit::Backspace.apply(&mut query));
        assert_eq!(query, "run this");
        assert!(Edit::Clear.apply(&mut query));
        assert!(!Edit::Backspace.apply(&mut query), "nothing left to delete");
    }
}
