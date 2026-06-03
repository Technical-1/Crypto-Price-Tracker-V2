//! View 2: open lots per wallet, enriched with current value and unrealized P&L.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;

use crate::app::App;

fn color_for(v: Decimal) -> Style {
    if v >= Decimal::ZERO {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(
        [
            "ASSET",
            "WALLET",
            "QTY",
            "COST BASIS",
            "AVG COST",
            "CURRENT VALUE",
            "UNREALIZED",
        ]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );

    let mut rows: Vec<Row> = app
        .derived
        .holdings
        .iter()
        .map(|hv| {
            Row::new(vec![
                Cell::from(app.config.symbol(&hv.holding.asset)),
                Cell::from(hv.holding.wallet.clone()),
                Cell::from(format!("{}", hv.holding.quantity)),
                Cell::from(format!("{:.2}", hv.holding.cost_basis)),
                Cell::from(format!("{:.2}", hv.holding.average_cost)),
                Cell::from(format!("{:.2}", hv.current_value)),
                Cell::from(format!("{:+.2}", hv.unrealized)).style(color_for(hv.unrealized)),
            ])
        })
        .collect();

    let total_value: Decimal = app.derived.holdings.iter().map(|h| h.current_value).sum();
    let total_unrealized: Decimal = app.derived.holdings.iter().map(|h| h.unrealized).sum();
    rows.push(Row::new(vec![
        Cell::from("TOTAL").style(Style::default().fg(Color::Yellow)),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(format!("{:.2}", total_value)),
        Cell::from(format!("{:+.2}", total_unrealized)).style(color_for(total_unrealized)),
    ]));

    let widths = [
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Length(14),
        Constraint::Length(14),
    ];
    let group_note = if app.group_by_wallet {
        " (by wallet) "
    } else {
        " (by asset) "
    };
    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Holdings{} ", group_note)),
    );
    f.render_widget(table, area);
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
    fn shows_holdings_columns_and_wallet() {
        let mut t = Terminal::new(TestBackend::new(140, 20)).unwrap();
        t.draw(|f| crate::ui::holdings::render(f, f.area(), &app()))
            .unwrap();
        let s: String = t
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(s.contains("ASSET"));
        assert!(s.contains("WALLET"));
        assert!(s.contains("UNREALIZED"));
        assert!(s.contains("coinbase"));
    }
}
