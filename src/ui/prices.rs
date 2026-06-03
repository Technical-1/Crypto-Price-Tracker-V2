//! View 1: live prices and profit/loss table.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;

use crate::app::{App, SortKey};

fn color_for(v: Decimal) -> Style {
    if v >= Decimal::ZERO {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(
        ["SYMBOL", "PRICE", "24H%", "7D%", "HELD", "COST", "VALUE", "PROFIT", "PROFIT%", "ALLOC"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );

    let mut rows: Vec<(String, Decimal, Decimal, Option<Decimal>, Decimal, Decimal, Decimal, Decimal, Decimal, Decimal)> = Vec::new();
    if let Some(report) = &app.derived.valuation {
        for av in &report.assets {
            let quote = app.prices.as_ref().and_then(|b| b.quotes.get(&av.asset));
            let change_24h = quote.map(|q| q.change_24h).unwrap_or(Decimal::ZERO);
            let change_7d = quote.and_then(|q| q.change_7d);
            let profit = av.unrealized;
            let profit_pct = if av.cost_basis.is_zero() {
                Decimal::ZERO
            } else {
                profit / av.cost_basis * Decimal::from(100)
            };
            rows.push((
                app.config.symbol(&av.asset),
                av.price, change_24h, change_7d, av.quantity, av.cost_basis,
                av.market_value, profit, profit_pct, av.allocation * Decimal::from(100),
            ));
        }
    }

    rows.sort_by(|a, b| {
        let ord = match app.sort {
            SortKey::Symbol => a.0.cmp(&b.0),
            SortKey::Price => a.1.cmp(&b.1),
            SortKey::Change24h => a.2.cmp(&b.2),
            SortKey::Value => a.6.cmp(&b.6),
            SortKey::Profit => a.7.cmp(&b.7),
        };
        if app.sort_desc { ord.reverse() } else { ord }
    });

    let table_rows: Vec<Row> = rows
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.0.clone()),
                Cell::from(format!("{:.2}", r.1)),
                Cell::from(format!("{:+.2}", r.2)).style(color_for(r.2)),
                Cell::from(r.3.map(|v| format!("{:+.2}", v)).unwrap_or_else(|| "\u{2014}".into())),
                Cell::from(format!("{}", r.4)),
                Cell::from(format!("{:.2}", r.5)),
                Cell::from(format!("{:.2}", r.6)),
                Cell::from(format!("{:+.2}", r.7)).style(color_for(r.7)),
                Cell::from(format!("{:+.2}%", r.8)).style(color_for(r.8)),
                Cell::from(format!("{:.1}%", r.9)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(8), Constraint::Length(12), Constraint::Length(8),
        Constraint::Length(8), Constraint::Length(10), Constraint::Length(12),
        Constraint::Length(12), Constraint::Length(12), Constraint::Length(9),
        Constraint::Length(7),
    ];
    let table = Table::new(table_rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Prices / P&L "))
        .row_highlight_style(Style::default().bg(Color::DarkGray));
    f.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::prices::{PriceBook, Quote};
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn app_with_prices() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut app = App::new(Config::example(), &txs).unwrap();
        let mut quotes = HashMap::new();
        quotes.insert("bitcoin".into(), Quote {
            price: dec!(50000), change_24h: dec!(2.5), change_7d: Some(dec!(5.0)),
            market_cap: Some(dec!(1000000000000)), volume_24h: Some(dec!(20000000000)),
            ath: Some(dec!(69000)),
        });
        app.set_prices(PriceBook {
            quotes, fetched_at: Utc::now(), sparklines: HashMap::new(), stale: false });
        app
    }

    fn rendered(app: &App) -> String {
        let mut t = Terminal::new(TestBackend::new(140, 30)).unwrap();
        t.draw(|f| crate::ui::prices::render(f, f.area(), app)).unwrap();
        t.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn shows_symbol_price_and_header() {
        let s = rendered(&app_with_prices());
        assert!(s.contains("SYMBOL"));
        assert!(s.contains("PRICE"));
        assert!(s.contains("BTC"));
        assert!(s.contains("50000"));
    }
}
