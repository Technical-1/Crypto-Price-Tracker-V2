//! View 1: live prices and profit/loss table, with a 7d sparkline column and a
//! detail pane for the selected asset.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;

use crate::app::{App, SortKey};

struct PriceRow {
    symbol: String,
    asset: String,
    price: Decimal,
    change_24h: Decimal,
    change_7d: Option<Decimal>,
    held: Decimal,
    cost: Decimal,
    value: Decimal,
    profit: Decimal,
    profit_pct: Decimal,
    alloc: Decimal,
    spark: String,
}

fn color_for(v: Decimal) -> Style {
    if v >= Decimal::ZERO {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

/// Render a compact block-bar sparkline from a price series (downsampled to ~10 cells).
fn sparkline(data: &[f64]) -> String {
    const BARS: [char; 8] = [
        '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
        '\u{2588}',
    ];
    if data.len() < 2 {
        return String::new();
    }
    let min = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = (max - min).max(f64::EPSILON);
    let step = (data.len() / 10).max(1);
    data.iter()
        .step_by(step)
        .map(|v| {
            let idx = (((v - min) / range) * 7.0).round() as usize;
            BARS[idx.min(7)]
        })
        .collect()
}

fn fmt_opt(v: Option<Decimal>) -> String {
    v.map(|x| format!("{:.0}", x))
        .unwrap_or_else(|| "\u{2014}".into())
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(4)])
        .split(area);

    let header = Row::new(
        [
            "SYMBOL", "PRICE", "24H%", "7D%", "HELD", "COST", "VALUE", "PROFIT", "PROFIT%",
            "ALLOC", "7D TREND",
        ]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );

    let mut rows: Vec<PriceRow> = Vec::new();
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
            let spark = app
                .prices
                .as_ref()
                .and_then(|b| b.sparklines.get(&av.asset))
                .map(|s| sparkline(s))
                .unwrap_or_default();
            rows.push(PriceRow {
                symbol: app.config.symbol(&av.asset),
                asset: av.asset.clone(),
                price: av.price,
                change_24h,
                change_7d,
                held: av.quantity,
                cost: av.cost_basis,
                value: av.market_value,
                profit,
                profit_pct,
                alloc: av.allocation * Decimal::from(100),
                spark,
            });
        }
    }

    rows.sort_by(|a, b| {
        let ord = match app.sort {
            SortKey::Symbol => a.symbol.cmp(&b.symbol),
            SortKey::Price => a.price.cmp(&b.price),
            SortKey::Change24h => a.change_24h.cmp(&b.change_24h),
            SortKey::Value => a.value.cmp(&b.value),
            SortKey::Profit => a.profit.cmp(&b.profit),
        };
        if app.sort_desc {
            ord.reverse()
        } else {
            ord
        }
    });

    let total_cost: Decimal = rows.iter().map(|r| r.cost).sum();
    let total_value: Decimal = rows.iter().map(|r| r.value).sum();
    let total_profit: Decimal = rows.iter().map(|r| r.profit).sum();
    let total_alloc: Decimal = rows.iter().map(|r| r.alloc).sum();

    let mut table_rows: Vec<Row> = rows
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.symbol.clone()),
                Cell::from(format!("{:.2}", r.price)),
                Cell::from(format!("{:+.2}", r.change_24h)).style(color_for(r.change_24h)),
                Cell::from(
                    r.change_7d
                        .map(|v| format!("{:+.2}", v))
                        .unwrap_or_else(|| "\u{2014}".into()),
                ),
                Cell::from(format!("{}", r.held)),
                Cell::from(format!("{:.2}", r.cost)),
                Cell::from(format!("{:.2}", r.value)),
                Cell::from(format!("{:+.2}", r.profit)).style(color_for(r.profit)),
                Cell::from(format!("{:+.2}%", r.profit_pct)).style(color_for(r.profit_pct)),
                Cell::from(format!("{:.1}%", r.alloc)),
                Cell::from(r.spark.clone()).style(Style::default().fg(Color::Cyan)),
            ])
        })
        .collect();

    table_rows.push(Row::new(vec![
        Cell::from("TOTAL").style(Style::default().fg(Color::Yellow)),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(format!("{:.2}", total_cost)),
        Cell::from(format!("{:.2}", total_value)),
        Cell::from(format!("{:+.2}", total_profit)).style(color_for(total_profit)),
        Cell::from(""),
        Cell::from(format!("{:.1}%", total_alloc)),
        Cell::from(""),
    ]));

    let widths = [
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(9),
        Constraint::Length(7),
        Constraint::Length(12),
    ];
    let table = Table::new(table_rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Prices / P&L "),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray));
    f.render_widget(table, chunks[0]);

    let detail = if rows.is_empty() {
        Line::from("No assets — fetch prices with `r`.")
    } else {
        let sel = app.selected.min(rows.len() - 1);
        let row = &rows[sel];
        let quote = app.prices.as_ref().and_then(|b| b.quotes.get(&row.asset));
        let (mc, vol, ath) = quote
            .map(|q| (q.market_cap, q.volume_24h, q.ath))
            .unwrap_or((None, None, None));
        Line::from(format!(
            "{}   MKT CAP: {}   24H VOL: {}   ATH: {}",
            row.symbol,
            fmt_opt(mc),
            fmt_opt(vol),
            fmt_opt(ath)
        ))
    };
    f.render_widget(
        Paragraph::new(detail).block(Block::default().borders(Borders::ALL).title(" Detail ")),
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

    fn app_with_prices() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(30000),
            fee: dec!(0),
        }];
        let mut app = App::new(Config::example(), &txs).unwrap();
        let mut quotes = HashMap::new();
        quotes.insert(
            "bitcoin".into(),
            Quote {
                price: dec!(50000),
                change_24h: dec!(2.5),
                change_7d: Some(dec!(5.0)),
                market_cap: Some(dec!(1000000000000)),
                volume_24h: Some(dec!(20000000000)),
                ath: Some(dec!(69000)),
            },
        );
        app.set_prices(PriceBook {
            quotes,
            fetched_at: Utc::now(),
            sparklines: {
                let mut sp = HashMap::new();
                sp.insert("bitcoin".to_string(), vec![100.0, 101.0, 102.0, 101.5, 103.0, 104.0, 103.5, 105.0]);
                sp
            },
            stale: false,
        });
        app
    }

    fn rendered(app: &App) -> String {
        let mut t = Terminal::new(TestBackend::new(140, 30)).unwrap();
        t.draw(|f| crate::ui::prices::render(f, f.area(), app))
            .unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn shows_symbol_price_and_header() {
        let s = rendered(&app_with_prices());
        assert!(s.contains("SYMBOL"));
        assert!(s.contains("PRICE"));
        assert!(s.contains("BTC"));
        assert!(s.contains("50000"));
        assert!(s.contains("7D TREND"));
        assert!(s.contains("MKT CAP"));
        assert!(s.contains("Detail"));
    }
}
