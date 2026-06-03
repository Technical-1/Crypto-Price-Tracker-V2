//! View 5: target vs current allocation and suggested (tax-aware) trades.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::rebalance::{RebalanceSide, Strategy};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let strat = match app.strategy {
        Strategy::Band => "Band",
        Strategy::Full => "Full",
    };
    let banner = match &app.derived.rebalance_summary {
        Some(s) if s.in_balance => {
            Line::from(format!("✓ In balance — strategy {strat} (t to toggle)"))
        }
        Some(s) => Line::from(format!(
            "⚠ Out of balance — buys {:.2} / sells {:.2} — strategy {strat} (t to toggle)",
            s.total_buys, s.total_sells
        )),
        None => Line::from("No valuation yet — fetch prices with `r`."),
    };
    f.render_widget(
        Paragraph::new(banner).block(Block::default().borders(Borders::ALL).title(" Rebalance ")),
        chunks[0],
    );

    let header = Row::new(
        ["SIDE", "ASSET", "AMOUNT$", "DRIFT%", "EST. GAIN (HIFO)"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );
    let total = app
        .derived
        .valuation
        .as_ref()
        .map(|r| r.total_value)
        .unwrap_or(rust_decimal::Decimal::ONE);
    let rows: Vec<Row> = app
        .derived
        .rebalance_actions
        .iter()
        .map(|a| {
            let (side, style) = match a.side {
                RebalanceSide::Buy => ("BUY", Style::default().fg(Color::Green)),
                RebalanceSide::Sell => ("SELL", Style::default().fg(Color::Red)),
            };
            let drift_pct = if total.is_zero() {
                rust_decimal::Decimal::ZERO
            } else {
                a.drift / total * rust_decimal::Decimal::from(100)
            };
            let est = match a.side {
                RebalanceSide::Sell => a
                    .est_realized_gain
                    .map(|g| format!("{:+.2}", g))
                    .unwrap_or_else(|| "—".into()),
                RebalanceSide::Buy => "—".into(),
            };
            Row::new(vec![
                Cell::from(side).style(style),
                Cell::from(app.config.symbol(&a.asset)),
                Cell::from(format!("{:.2}", a.amount_usd)),
                Cell::from(format!("{:+.1}%", drift_pct)),
                Cell::from(est),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Length(14),
        Constraint::Length(9),
        Constraint::Length(18),
    ];
    f.render_widget(
        Table::new(rows, widths).header(header).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Suggested trades (estimates) "),
        ),
        chunks[1],
    );
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::prices::{PriceBook, Quote};
    use chrono::{TimeZone, Utc};
    use coinbasis::Transaction;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(30000),
            fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        let mut q = HashMap::new();
        q.insert(
            "bitcoin".into(),
            Quote {
                price: dec!(50000),
                change_24h: dec!(0),
                change_7d: None,
                market_cap: None,
                volume_24h: None,
                ath: None,
            },
        );
        a.set_prices(PriceBook {
            quotes: q,
            fetched_at: Utc::now(),
            sparklines: HashMap::new(),
            stale: false,
        });
        a
    }

    #[test]
    fn shows_action_columns_and_banner() {
        let mut t = Terminal::new(TestBackend::new(140, 26)).unwrap();
        t.draw(|f| crate::ui::rebalance::render(f, f.area(), &app()))
            .unwrap();
        let s: String = t
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(s.contains("SIDE"));
        assert!(s.contains("AMOUNT"));
        assert!(s.contains("DRIFT"));
        assert!(s.contains("balance")); // in/out of balance banner
    }
}
