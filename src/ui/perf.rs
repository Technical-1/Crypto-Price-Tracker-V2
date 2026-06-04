//! View 6: value-history line chart, performance metrics, and a per-day
//! playback with a holdings breakdown.

use std::collections::HashMap;

use coinbasis::Portfolio;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::Line;
use ratatui::widgets::{
    Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table,
};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::app::{App, PerfMode};
use crate::perf::{self, Snapshot};

/// The snapshot series the Performance view renders: the ledger-replay
/// reconstruction when available, else the forward-recorded history.
fn series(app: &App) -> &[Snapshot] {
    if !app.derived.reconstructed.is_empty() {
        &app.derived.reconstructed
    } else {
        &app.history
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    match app.perf_mode {
        PerfMode::Chart => render_chart(f, area, app),
        PerfMode::Playback => render_playback(f, area, app),
    }
}

fn render_chart(f: &mut Frame, area: Rect, app: &App) {
    let snaps = series(app);
    if snaps.len() < 2 {
        f.render_widget(
            Paragraph::new(
                "Performance: not enough history yet — values are recorded as prices refresh. (p: playback)",
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Performance "),
            ),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(8)])
        .split(area);

    let value_points: Vec<(f64, f64)> = snaps
        .iter()
        .enumerate()
        .map(|(i, s)| (i as f64, s.total_value.to_f64().unwrap_or(0.0)))
        .collect();
    let pl_points: Vec<(f64, f64)> = snaps
        .iter()
        .enumerate()
        .map(|(i, s)| (i as f64, s.pl.to_f64().unwrap_or(0.0)))
        .collect();
    let all = value_points.iter().chain(pl_points.iter());
    let max_y = all.clone().map(|p| p.1).fold(f64::MIN, f64::max);
    let min_y = all.map(|p| p.1).fold(f64::MAX, f64::min);
    let last_x = (value_points.len() - 1) as f64;

    let datasets = vec![
        Dataset::default()
            .name("value")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&value_points),
        Dataset::default()
            .name("P&L")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Magenta))
            .data(&pl_points),
    ];
    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Value History (p: playback) "),
        )
        .x_axis(Axis::default().bounds([0.0, last_x]))
        .y_axis(
            Axis::default()
                .bounds([min_y, max_y])
                .labels(vec![format!("{:.0}", min_y), format!("{:.0}", max_y)]),
        );
    f.render_widget(chart, chunks[0]);

    let m = perf::metrics(snaps);
    let fmt = |o: Option<f64>| o.map(|v| format!("{:.4}", v)).unwrap_or_else(|| "—".into());
    let lines = vec![
        Line::from(format!("Volatility:        {}", fmt(m.volatility))),
        Line::from(format!("Sharpe:            {}", fmt(m.sharpe))),
        Line::from(format!("Max Drawdown:      {}", fmt(m.max_drawdown))),
        Line::from(format!("Cumulative Return: {}", fmt(m.cumulative_return))),
    ];
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Metrics ")),
        chunks[1],
    );
}

/// An 18-cell signed bar centered on zero: green to the right for gains, red to
/// the left for losses, scaled against `max_abs`.
fn pl_bar(pl: f64, max_abs: f64) -> String {
    const HALF: usize = 9;
    if max_abs <= 0.0 {
        return " ".repeat(HALF * 2);
    }
    let frac = (pl.abs() / max_abs).min(1.0);
    let cells = (frac * HALF as f64).round() as usize;
    if pl >= 0.0 {
        format!("{}{}", " ".repeat(HALF), "█".repeat(cells))
    } else {
        format!(
            "{}{}{}",
            " ".repeat(HALF - cells),
            "█".repeat(cells),
            " ".repeat(HALF)
        )
    }
}

fn render_playback(f: &mut Frame, area: Rect, app: &App) {
    let snaps = series(app);
    if snaps.is_empty() {
        f.render_widget(
            Paragraph::new(
                "Playback: no reconstructed history yet — refresh with a CoinGecko key to fetch price history. (p: chart)",
            )
            .block(Block::default().borders(Borders::ALL).title(" Playback ")),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(10)])
        .split(area);

    let selected = app.selected.min(snaps.len() - 1);
    let max_abs = snaps
        .iter()
        .map(|s| s.pl.to_f64().unwrap_or(0.0).abs())
        .fold(0.0_f64, f64::max);

    let rows: Vec<Row> = snaps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let pl = s.pl.to_f64().unwrap_or(0.0);
            let cells = vec![
                Cell::from(s.date.format("%Y-%m-%d").to_string()),
                Cell::from(format!("{:.2}", s.total_value)),
                Cell::from(format!("{:+.2}", s.pl)),
                Cell::from(pl_bar(pl, max_abs)),
            ];
            let row = Row::new(cells);
            if i == selected {
                row.style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                row
            }
        })
        .collect();
    let widths = [
        Constraint::Length(12),
        Constraint::Length(16),
        Constraint::Length(16),
        Constraint::Length(20),
    ];
    let header = Row::new(vec!["date", "value", "P/L", "P/L"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(
        Table::new(rows, widths).header(header).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Playback (↑/↓ day · p: chart) "),
        ),
        chunks[0],
    );

    // Per-coin breakdown for the selected day.
    let day = snaps[selected].date;
    let day_prices: HashMap<String, Decimal> = app
        .price_history
        .iter()
        .filter_map(|(c, s)| {
            s.iter()
                .find(|(dt, _)| *dt == day)
                .and_then(|(_, p)| Decimal::from_f64_retain(*p))
                .map(|px| (c.clone(), px))
        })
        .collect();
    let upto: Vec<coinbasis::Transaction> = app
        .model
        .transactions()
        .iter()
        .filter(|t| t.timestamp() <= day)
        .cloned()
        .collect();

    let mut detail: Vec<Row> = Vec::new();
    if let Ok(p) = Portfolio::from_transactions(&upto) {
        if let Ok(report) = p.valuation(app.method, &day_prices) {
            for av in &report.assets {
                detail.push(Row::new(vec![
                    Cell::from(av.asset.clone()),
                    Cell::from(format!("{:.6}", av.quantity)),
                    Cell::from(format!("{:.2}", av.price)),
                    Cell::from(format!("{:.2}", av.market_value)),
                ]));
            }
        }
    }
    let detail_header = Row::new(vec!["asset", "qty", "price", "value"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let detail_widths = [
        Constraint::Length(12),
        Constraint::Length(16),
        Constraint::Length(16),
        Constraint::Length(16),
    ];
    let title = format!(" Breakdown · {} ", day.format("%Y-%m-%d"));
    f.render_widget(
        Table::new(detail, detail_widths)
            .header(detail_header)
            .block(Block::default().borders(Borders::ALL).title(title)),
        chunks[1],
    );
}

#[cfg(test)]
mod tests {
    use crate::app::{App, PerfMode};
    use crate::config::Config;
    use crate::perf::Snapshot;
    use chrono::{TimeZone, Utc};
    use coinbasis::Transaction;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;

    fn app_with_history(points: usize) -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(30000),
            fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        a.history = (0..points)
            .map(|i| Snapshot {
                date: Utc
                    .with_ymd_and_hms(2026, 1, 1 + i as u32, 0, 0, 0)
                    .unwrap(),
                total_value: dec!(100) + rust_decimal::Decimal::from(i),
                cost: dec!(0),
                pl: dec!(0),
            })
            .collect();
        a
    }

    fn buffer_text(app: &App) -> String {
        let mut t = Terminal::new(TestBackend::new(120, 30)).unwrap();
        t.draw(|f| crate::ui::perf::render(f, f.area(), app))
            .unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn short_history_shows_need_more_message() {
        let s = buffer_text(&app_with_history(1));
        assert!(s.contains("not enough history"));
    }

    #[test]
    fn longer_history_shows_metrics_labels() {
        let s = buffer_text(&app_with_history(8));
        assert!(s.contains("Volatility"));
        assert!(s.contains("Max Drawdown"));
    }

    #[test]
    fn playback_shows_day_rows_and_breakdown() {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(100),
            fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        let mut hist = std::collections::HashMap::new();
        hist.insert(
            "bitcoin".to_string(),
            vec![
                (Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(), 100.0),
                (Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(), 150.0),
            ],
        );
        a.set_price_history(hist);
        a.perf_mode = PerfMode::Playback;
        let s = buffer_text(&a);
        assert!(s.contains("Playback"));
        assert!(s.contains("2024-01-02"));
        assert!(s.contains("Breakdown"));
    }
}
