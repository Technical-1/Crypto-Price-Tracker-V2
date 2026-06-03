//! Frame composition: tab bar, active view, status bar, and help overlay.

pub mod holdings;
pub mod perf;
pub mod prices;
pub mod rebalance;
pub mod tax;
pub mod valuation;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs};
use ratatui::Frame;

use crate::app::{App, View};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_tab_bar(f, chunks[0], app);
    draw_active_view(f, chunks[1], app);
    draw_status_bar(f, chunks[2], app);

    if app.show_help {
        draw_help(f, f.area());
    }
}

fn draw_tab_bar(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = View::ALL.iter().map(|v| Line::from(v.title())).collect();
    let selected = View::ALL.iter().position(|&v| v == app.view).unwrap();
    let countdown = app
        .prices
        .as_ref()
        .map(|_| format!("{}s", app.config.refresh_seconds))
        .unwrap_or_else(|| "—".into());
    let title = format!(" {} · refresh {} ", app.method_label(), countdown);
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(title))
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}

fn draw_active_view(f: &mut Frame, area: Rect, app: &App) {
    match app.view {
        View::Prices => prices::render(f, area, app),
        View::Holdings => holdings::render(f, area, app),
        View::Valuation => valuation::render(f, area, app),
        View::Tax => tax::render(f, area, app),
        View::Rebalance => rebalance::render(f, area, app),
        View::Performance => perf::render(f, area, app),
    }
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![Span::styled(
        format!(" {} ", app.method_label()),
        Style::default().fg(Color::Black).bg(Color::Cyan),
    )];
    if app.loading {
        spans.push(Span::raw(" ⟳ loading "));
    }
    if let Some(b) = &app.prices {
        if b.stale {
            spans.push(Span::styled(" [stale] ", Style::default().fg(Color::Red)));
        }
    }
    if !app.status.message.is_empty() {
        spans.push(Span::raw(format!(" {} ", app.status.message)));
    }
    spans.push(Span::styled(
        " ? help ",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let w = area.width.min(60);
    let h = area.height.min(16);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    let lines = vec![
        Line::from("Keybindings"),
        Line::from("Tab/→  next view    Shift-Tab/←  prev view"),
        Line::from("m      cycle method  s  cycle sort"),
        Line::from("g      toggle grouping"),
        Line::from("[ / ]  change tax year"),
        Line::from("t      toggle rebalance strategy"),
        Line::from("r      refresh now    e  export (Tax/Holdings)"),
        Line::from("↑/↓    move selection"),
        Line::from("?      toggle help     q/Esc  quit"),
    ];
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Help ")),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use chrono::{TimeZone, Utc};
    use coinbasis::Transaction;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(30000),
            fee: dec!(0),
        }];
        App::new(Config::example(), &txs).unwrap()
    }

    fn render_to_string(app: &App) -> String {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn tab_bar_shows_all_view_titles_and_method() {
        let s = render_to_string(&app());
        assert!(s.contains("Prices"));
        assert!(s.contains("Holdings"));
        assert!(s.contains("Performance"));
        assert!(s.contains("FIFO"));
    }

    #[test]
    fn help_overlay_lists_keybindings_when_shown() {
        let mut a = app();
        a.show_help = true;
        let s = render_to_string(&a);
        assert!(s.contains("cycle method"));
        assert!(s.contains("quit"));
    }
}
