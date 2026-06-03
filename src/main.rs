mod config;
mod error;
mod ledger;
mod perf;
mod portfolio;
mod prices;
mod rebalance;

use std::io::{self, Stdout};
use std::panic;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original(info);
    }));
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    install_panic_hook();
    let mut terminal = setup_terminal()?;

    loop {
        terminal.draw(|f| {
            let widget = Paragraph::new("Crypto-Price-Tracker-V2 — press q to quit")
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(widget, f.area());
        })?;

        if let Event::Key(key) = event::read()? {
            if matches!(key.code, KeyCode::Char('q')) {
                break;
            }
        }
    }

    restore_terminal()?;
    Ok(())
}
