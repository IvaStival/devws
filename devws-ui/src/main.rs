//! devws-ui: devws's lists, drawn without the terminal cursor so a tmux
//! popup does not flicker each time tmux redraws it.
//!
//!   devws-ui pick [options] < lines     one question (picker.sh): prints the
//!                                       chosen line(s); exits 0, 1 when
//!                                       nothing matched, 130 on Esc
//!   devws-ui menu [--tab <tab>] < rows  the devws menu, from
//!                                       "tab<TAB>action<TAB>label<TAB>shortcut"
//!                                       rows; prints "tab<TAB>action", or the
//!                                       tab alone and exits 130 on Esc

mod ansi;
mod filter;
mod input;
mod items;
mod menu;
mod pick;
mod tty;
mod view;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::io::{IsTerminal, Read, Write};

#[derive(Parser)]
#[command(version, about = "devws's menu and pickers")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// One question from the lines on stdin.
    Pick(PickArgs),
    /// The devws menu, from its rows on stdin.
    Menu(MenuArgs),
}

#[derive(Args)]
struct PickArgs {
    #[arg(long, default_value = "> ", allow_hyphen_values = true)]
    prompt: String,
    #[arg(long, default_value = "", allow_hyphen_values = true)]
    header: String,
    #[arg(long, default_value = "", allow_hyphen_values = true)]
    footer: String,
    /// Tab ticks several; Enter answers with every ticked line.
    #[arg(long)]
    multi: bool,
    /// With --multi, every line starts ticked.
    #[arg(long)]
    select_all: bool,
    #[arg(long, allow_hyphen_values = true)]
    delimiter: Option<String>,
    /// Shows only this field (from 1) of each line; the answer is the line.
    #[arg(long)]
    with_nth: Option<usize>,
    /// Answers with what was typed first, then the choice.
    #[arg(long)]
    print_query: bool,
}

#[derive(Args)]
struct MenuArgs {
    /// The tab to open on.
    #[arg(long)]
    tab: Option<String>,
    #[arg(
        long,
        default_value = "←/→ tab · type to search every tab · Enter run · Esc close",
        allow_hyphen_values = true
    )]
    footer: String,
}

fn stdin_text() -> Result<String> {
    let mut text = String::new();
    let mut stdin = std::io::stdin();
    if !stdin.is_terminal() {
        stdin
            .read_to_string(&mut text)
            .context("reading the choices from stdin")?;
    }
    Ok(text)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (lines, code) = match cli.command {
        Command::Pick(args) => pick(args)?,
        Command::Menu(args) => menu(args)?,
    };
    let mut stdout = std::io::stdout().lock();
    for line in lines {
        writeln!(stdout, "{line}")?;
    }
    stdout.flush()?;
    std::process::exit(code);
}

fn pick(args: PickArgs) -> Result<(Vec<String>, i32)> {
    let choices = items::parse(&stdin_text()?, args.delimiter.as_deref(), args.with_nth);
    let mut picker = pick::Picker::new(choices, args.multi, args.select_all);
    let text = view::PickText {
        prompt: &args.prompt,
        header: &args.header,
        footer: &args.footer,
    };
    let print_query = args.print_query;
    let answer = tty::run(
        &mut picker,
        false,
        |picker, area, buffer| view::draw_pick(picker, &text, area, buffer),
        |picker, key| match picker.press(key, print_query) {
            pick::Step::Done(answer) => Some(answer),
            pick::Step::Continue => None,
        },
    )?;
    Ok((answer.lines, answer.code))
}

fn menu(args: MenuArgs) -> Result<(Vec<String>, i32)> {
    let rows = menu::parse(&stdin_text()?);
    let mut menu = menu::Menu::new(rows, args.tab.as_deref());
    let footer = args.footer;
    let (tab, action) = tty::run(
        &mut menu,
        true,
        |menu, area, buffer| view::draw_menu(menu, &footer, area, buffer),
        |menu, key| match menu.press(key) {
            menu::Step::Done { tab, action } => Some((tab, action)),
            menu::Step::Continue => None,
        },
    )?;
    Ok(match action {
        Some(action) => (vec![format!("{tab}\t{action}")], 0),
        None => (vec![tab], 130),
    })
}
