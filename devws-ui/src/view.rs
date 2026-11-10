//! Drawing a picker and the menu. The caret is a reversed cell of our own:
//! the terminal's cursor stays hidden, so tmux has no cursor to show again
//! (and restart blinking) each time it redraws a popup — the flicker fzf had.

use crate::ansi;
use crate::menu::{Menu, title};
use crate::pick::Picker;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

/// fzf's highlighted row, as devws's pickers had it.
const CURRENT_ROW: Style = Style::new().bg(Color::Indexed(236));
const POINTER: Style = Style::new()
    .fg(Color::Indexed(161))
    .add_modifier(Modifier::BOLD);
const PROMPT: Style = Style::new()
    .fg(Color::Indexed(110))
    .add_modifier(Modifier::BOLD);
const DIM: Style = Style::new().add_modifier(Modifier::DIM);
const CURRENT_TAB: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Cyan)
    .add_modifier(Modifier::BOLD);

/// Where the list was drawn, to turn a click into its row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geometry {
    pub list_top: u16,
    pub list_height: u16,
    /// The first shown row's position in the list.
    pub offset: usize,
}

impl Geometry {
    /// The list position at screen `row`, if it is on the list.
    pub fn row_at(&self, row: u16) -> Option<usize> {
        (row >= self.list_top && row < self.list_top + self.list_height)
            .then(|| self.offset + (row - self.list_top) as usize)
    }
}

pub struct PickText<'a> {
    pub prompt: &'a str,
    pub header: &'a str,
    pub footer: &'a str,
}

pub fn draw_pick(picker: &Picker, text: &PickText, area: Rect, buffer: &mut Buffer) -> Geometry {
    let mut y = area.y;
    let bottom = area.y + area.height;
    let count = format!("{}/{}", picker.matches.len(), picker.items.len());
    prompt_line(
        text.prompt,
        &picker.query,
        &count,
        Rect::new(area.x, y, area.width, 1),
        buffer,
    );
    y += 1;
    for header in text.header.lines().filter(|line| !line.is_empty()) {
        if y >= bottom {
            break;
        }
        ansi::line(header, Style::default()).render(Rect::new(area.x, y, area.width, 1), buffer);
        y += 1;
    }
    let footer_rows = text.footer.lines().filter(|line| !line.is_empty()).count() as u16;
    let list_height = bottom.saturating_sub(y + footer_rows);
    let offset = scroll_offset(picker.cursor, list_height as usize);
    for (row, &index) in picker
        .matches
        .iter()
        .skip(offset)
        .take(list_height as usize)
        .enumerate()
    {
        let item = &picker.items[index];
        let is_current = offset + row == picker.cursor;
        let mut spans = vec![pointer(is_current)];
        if picker.multi {
            let mark = if picker.ticked.contains(&index) {
                "✓ "
            } else {
                "  "
            };
            spans.push(Span::styled(mark, POINTER));
        }
        spans.extend(ansi::line(&item.shown, Style::default()).spans);
        row_line(
            spans,
            is_current,
            Rect::new(area.x, y + row as u16, area.width, 1),
            buffer,
        );
    }
    footer(text.footer, area, buffer);
    Geometry {
        list_top: y,
        list_height,
        offset,
    }
}

pub fn draw_menu(menu: &Menu, footer_text: &str, area: Rect, buffer: &mut Buffer) -> Geometry {
    let bottom = area.y + area.height;
    tab_bar(menu, Rect::new(area.x, area.y, area.width, 1), buffer);
    prompt_line(
        "Search › ",
        &menu.query,
        "",
        Rect::new(
            area.x,
            area.y + 1,
            area.width,
            1.min(area.height.saturating_sub(1)),
        ),
        buffer,
    );
    let list_top = area.y + 2;
    let footer_rows = footer_text.lines().filter(|line| !line.is_empty()).count() as u16;
    let list_height = bottom.saturating_sub(list_top + footer_rows);
    let offset = scroll_offset(menu.cursor, list_height as usize);
    for (row, &index) in menu
        .shown
        .iter()
        .skip(offset)
        .take(list_height as usize)
        .enumerate()
    {
        let is_current = offset + row == menu.cursor;
        let label = menu.label_of(index);
        let padding = 40usize.saturating_sub(ansi::strip(&label).chars().count());
        let mut spans = vec![pointer(is_current)];
        spans.extend(ansi::line(&label, Style::default()).spans);
        spans.push(Span::raw(" ".repeat(padding + 1)));
        spans.push(Span::styled(menu.rows[index].shortcut.clone(), DIM));
        row_line(
            spans,
            is_current,
            Rect::new(area.x, list_top + row as u16, area.width, 1),
            buffer,
        );
    }
    footer(footer_text, area, buffer);
    Geometry {
        list_top,
        list_height,
        offset,
    }
}

/// " Title " per tab, one space apart (Menu::tab_at reads the same layout).
fn tab_bar(menu: &Menu, area: Rect, buffer: &mut Buffer) {
    let mut spans = Vec::new();
    for (index, tab) in menu.tabs.iter().enumerate() {
        let style = if index == menu.current_tab {
            CURRENT_TAB
        } else {
            DIM
        };
        spans.push(Span::styled(format!(" {} ", title(tab)), style));
        spans.push(Span::raw(" "));
    }
    Line::from(spans).render(area, buffer);
}

/// The prompt, what was typed and the caret; `info` (a count) on the right.
fn prompt_line(prompt: &str, query: &str, info: &str, area: Rect, buffer: &mut Buffer) {
    if area.height == 0 {
        return;
    }
    Line::from(vec![
        Span::styled(prompt.to_string(), PROMPT),
        Span::raw(query.to_string()),
        Span::styled(" ", Style::new().add_modifier(Modifier::REVERSED)),
    ])
    .render(area, buffer);
    let width = info.chars().count() as u16;
    if !info.is_empty() && width < area.width {
        Span::styled(info.to_string(), DIM).render(
            Rect::new(area.x + area.width - width, area.y, width, 1),
            buffer,
        );
    }
}

fn pointer(is_current: bool) -> Span<'static> {
    if is_current {
        Span::styled("▌ ", POINTER)
    } else {
        Span::raw("  ")
    }
}

fn row_line(spans: Vec<Span<'static>>, is_current: bool, area: Rect, buffer: &mut Buffer) {
    if is_current {
        buffer.set_style(area, CURRENT_ROW);
    }
    let line = Line::from(spans);
    let line = if is_current {
        line.patch_style(CURRENT_ROW)
    } else {
        line
    };
    line.render(area, buffer);
}

/// The footer's lines on the area's last rows.
fn footer(text: &str, area: Rect, buffer: &mut Buffer) {
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let rows = (lines.len() as u16).min(area.height);
    for (index, line) in lines.iter().rev().take(rows as usize).enumerate() {
        let y = area.y + area.height - 1 - index as u16;
        ansi::line(line, DIM).render(Rect::new(area.x, y, area.width, 1), buffer);
    }
}

/// The first row to show so the cursor's row is on screen.
fn scroll_offset(cursor: usize, height: usize) -> usize {
    if height == 0 {
        0
    } else {
        cursor.saturating_sub(height - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::parse;
    use crate::menu;

    fn text(buffer: &Buffer, row: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, row)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn a_picker_shows_prompt_header_list_and_footer() {
        let picker = Picker::new(
            parse("main\t/a\nfeat/x\t/b\n", Some("\t"), Some(1)),
            false,
            false,
        );
        let area = Rect::new(0, 0, 40, 6);
        let mut buffer = Buffer::empty(area);
        let geometry = draw_pick(
            &picker,
            &PickText {
                prompt: "Run › ",
                header: "Branch to run",
                footer: "\x1b[2mEsc back\x1b[0m",
            },
            area,
            &mut buffer,
        );
        assert_eq!(
            text(&buffer, 0),
            format!("Run ›  {}2/2", " ".repeat(40 - 6 - 1 - 3))
        );
        assert_eq!(text(&buffer, 1), "Branch to run");
        assert_eq!(text(&buffer, 2), "▌ main");
        assert_eq!(text(&buffer, 3), "  feat/x");
        assert_eq!(text(&buffer, 5), "Esc back");
        assert_eq!(
            geometry,
            Geometry {
                list_top: 2,
                list_height: 3,
                offset: 0
            }
        );
        assert_eq!(geometry.row_at(3), Some(1));
        assert_eq!(geometry.row_at(5), None, "the footer is not the list");
    }

    #[test]
    fn the_caret_is_a_reversed_cell_after_the_query() {
        let mut picker = Picker::new(parse("a\n", None, None), false, false);
        picker.query = "ab".into();
        let area = Rect::new(0, 0, 20, 3);
        let mut buffer = Buffer::empty(area);
        draw_pick(
            &picker,
            &PickText {
                prompt: "X › ",
                header: "",
                footer: "",
            },
            area,
            &mut buffer,
        );
        let caret = &buffer[(6, 0)];
        assert!(
            caret.modifier.contains(Modifier::REVERSED),
            "no caret after the query: {:?}",
            caret
        );
    }

    #[test]
    fn the_menu_marks_its_tab_and_lines_up_shortcuts() {
        let menu = Menu::new(
            menu::parse("workspace\tdetach\t⏏ Detach\tC-b d\ngit\tpush\t↑ Push\tC-b G\n"),
            None,
        );
        let area = Rect::new(0, 0, 60, 5);
        let mut buffer = Buffer::empty(area);
        let geometry = draw_menu(&menu, "←/→ tab", area, &mut buffer);
        assert_eq!(text(&buffer, 0), " Workspace   Git");
        assert_eq!(
            buffer[(1, 0)].style().bg,
            Some(Color::Cyan),
            "the current tab is not marked"
        );
        assert_eq!(
            text(&buffer, 2),
            format!("▌ ⏏ Detach{}C-b d", " ".repeat(40 - 8 + 1))
        );
        assert_eq!(text(&buffer, 4), "←/→ tab");
        assert_eq!(geometry.list_top, 2);
    }

    #[test]
    fn the_list_scrolls_to_keep_the_cursor_on_screen() {
        let mut picker = Picker::new(parse("a\nb\nc\nd\ne\n", None, None), false, false);
        picker.cursor = 4;
        let area = Rect::new(0, 0, 20, 3);
        let mut buffer = Buffer::empty(area);
        let geometry = draw_pick(
            &picker,
            &PickText {
                prompt: "> ",
                header: "",
                footer: "",
            },
            area,
            &mut buffer,
        );
        assert_eq!(geometry.offset, 3);
        assert_eq!(text(&buffer, 2), "▌ e");
    }
}
