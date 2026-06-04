//! View 5: target vs current allocation and suggested (tax-aware) trades.

use std::collections::BTreeMap;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::app::App;
use crate::rebalance::{RebalanceSide, Strategy};

/// A block-bar gauge for a percentage (0..100), ~20 cells wide (each cell = 5%).
fn bar(pct: Decimal) -> String {
    let cells = ((pct.to_f64().unwrap_or(0.0) / 5.0).round() as i64).clamp(0, 20) as usize;
    "\u{2588}".repeat(cells)
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    // Build the current-vs-target comparison lines first (height depends on count).
    let targets = app.config.normalized_targets();
    let mut compare_lines: Vec<Line> = Vec::new();
    if let Some(report) = &app.derived.valuation {
        let mut cur: BTreeMap<String, Decimal> = BTreeMap::new();
        for av in &report.assets {
            cur.insert(av.asset.clone(), av.allocation);
        }
        let mut keys: Vec<String> = cur.keys().cloned().collect();
        for k in targets.keys() {
            if !cur.contains_key(k) {
                keys.push(k.clone());
            }
        }
        keys.sort();
        keys.dedup();
        for asset in keys {
            let cur_pct = cur.get(&asset).copied().unwrap_or(Decimal::ZERO) * Decimal::from(100);
            let tgt_pct =
                targets.get(&asset).copied().unwrap_or(Decimal::ZERO) * Decimal::from(100);
            compare_lines.push(Line::from(format!(
                "{:>6}  cur {:<20} {:>5.1}%   tgt {:<20} {:>5.1}%",
                app.config.symbol(&asset),
                bar(cur_pct),
                cur_pct,
                bar(tgt_pct),
                tgt_pct,
            )));
        }
    }
    let compare_h = (compare_lines.len() as u16 + 2).clamp(3, 12);

    // Analytics panel: strategy line, per-coin risk, portfolio vol, correlation,
    // and backtest comparison.
    let analytics_lines = analytics_lines(app);
    let analytics_h = (analytics_lines.len() as u16 + 2).clamp(3, 16);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(compare_h),
            Constraint::Length(analytics_h),
            Constraint::Min(1),
        ])
        .split(area);

    let strat = match app.strategy {
        Strategy::Band => "Band",
        Strategy::Full => "Full",
    };
    let banner = match &app.derived.rebalance_summary {
        Some(s) if s.in_balance => Line::from(format!(
            "\u{2713} In balance — strategy {strat} (t to toggle)"
        )),
        Some(s) => Line::from(format!(
            "\u{26a0} Out of balance — buys {:.2} / sells {:.2} — strategy {strat} (t to toggle)",
            s.total_buys, s.total_sells
        )),
        None => Line::from("No valuation yet — fetch prices with `r`."),
    };
    f.render_widget(
        Paragraph::new(banner).block(Block::default().borders(Borders::ALL).title(" Rebalance ")),
        chunks[0],
    );

    f.render_widget(
        Paragraph::new(compare_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Current vs Target "),
        ),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(analytics_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Strategy / Risk / Backtest "),
        ),
        chunks[2],
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
        .unwrap_or(Decimal::ONE);
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
                Decimal::ZERO
            } else {
                a.drift / total * Decimal::from(100)
            };
            let est = match a.side {
                RebalanceSide::Sell => a
                    .est_realized_gain
                    .map(|g| format!("{:+.2}", g))
                    .unwrap_or_else(|| "\u{2014}".into()),
                RebalanceSide::Buy => "\u{2014}".into(),
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
        chunks[3],
    );
}

/// Build the strategy/risk/correlation/backtest lines for the analytics panel.
fn analytics_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    let strat_kind = match app.strategy {
        Strategy::Band => "Band",
        Strategy::Full => "Full",
    };
    lines.push(Line::from(format!(
        "Strategy: {:?} \u{b7} {} (w cycles target, t toggles band/full)",
        app.target_strategy, strat_kind,
    )));

    // Per-coin daily/annualized volatility.
    if app.derived.vols_daily.is_empty() {
        lines.push(Line::from(
            "Risk: (no price history \u{2014} fetch with `r`)".to_string(),
        ));
    } else {
        lines.push(Line::from("Risk (volatility):".to_string()));
        for (coin, vd) in &app.derived.vols_daily {
            let va = app.derived.vols_annual.get(coin).copied().unwrap_or(0.0);
            lines.push(Line::from(format!(
                "  {:>6}  daily {:>6.2}%   annual {:>7.2}%",
                app.config.symbol(coin),
                vd * 100.0,
                va * 100.0,
            )));
        }
    }

    // Portfolio volatility.
    match app.derived.portfolio_vol {
        Some(pv) => lines.push(Line::from(format!(
            "Portfolio daily volatility: {:.2}%",
            pv * 100.0
        ))),
        None => lines.push(Line::from(
            "Portfolio daily volatility: (need \u{2265}2 coins with history)".to_string(),
        )),
    }

    // Correlation matrix (only when populated).
    if !app.derived.correlation.is_empty() {
        let mut coins: Vec<String> = app.derived.vols_daily.keys().cloned().collect();
        coins.sort();
        if !coins.is_empty() {
            let header: String = coins
                .iter()
                .map(|c| format!("{:>7}", app.config.symbol(c)))
                .collect();
            lines.push(Line::from(format!("Correlation:  {header}")));
            for a in &coins {
                let row: String = coins
                    .iter()
                    .map(|b| {
                        let v = app
                            .derived
                            .correlation
                            .get(&(a.clone(), b.clone()))
                            .copied()
                            .unwrap_or(0.0);
                        format!("{v:>7.2}")
                    })
                    .collect();
                lines.push(Line::from(format!(
                    "  {:>6}    {}",
                    app.config.symbol(a),
                    row
                )));
            }
        }
    }

    // Backtest comparison.
    match (app.derived.backtest_current, app.derived.backtest_target) {
        (Some(cur), Some(tgt)) => lines.push(Line::from(format!(
            "Backtest (buy&hold): current {:+.2}% / target {:+.2}%",
            cur * 100.0,
            tgt * 100.0,
        ))),
        _ => lines.push(Line::from("Backtest (buy&hold): (no history)".to_string())),
    }

    lines
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
        let mut hist = HashMap::new();
        hist.insert(
            "bitcoin".to_string(),
            vec![
                (Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(), 100.0),
                (Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(), 110.0),
                (Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap(), 105.0),
            ],
        );
        hist.insert(
            "ethereum".to_string(),
            vec![
                (Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(), 50.0),
                (Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(), 52.0),
                (Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap(), 48.0),
            ],
        );
        a.set_price_history(hist);
        a
    }

    #[test]
    fn shows_action_columns_and_banner() {
        let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
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
        assert!(s.contains("Current vs Target"));
        assert!(s.contains("tgt"));
        assert!(s.contains("Risk"));
        assert!(s.contains("Backtest"));
        assert!(s.contains("Strategy"));
    }
}
