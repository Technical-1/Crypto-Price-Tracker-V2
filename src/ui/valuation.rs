//! View 3: headline portfolio valuation and a per-asset allocation bar chart.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{BarChart, Block, Borders, Paragraph};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(1)])
        .split(area);

    let Some(report) = &app.derived.valuation else {
        f.render_widget(
            Paragraph::new("No valuation yet — fetch prices with `r`.")
                .block(Block::default().borders(Borders::ALL).title(" Valuation ")),
            area,
        );
        return;
    };

    let mut lines = vec![
        Line::from(format!("Total Value:   {:.2} USD", report.total_value)),
        Line::from(format!("Total Cost:    {:.2} USD", report.total_cost)),
        Line::from(format!("Unrealized:    {:+.2} USD", report.total_unrealized)),
        Line::from(format!("Total Return:  {:+.2}%", report.total_return * rust_decimal::Decimal::from(100))),
    ];
    if !report.missing_prices.is_empty() {
        lines.push(Line::from(format!("⚠ missing prices: {}", report.missing_prices.join(", "))));
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Valuation ")),
        chunks[0],
    );

    let bars: Vec<(String, u64)> = report
        .assets
        .iter()
        .map(|av| {
            let pct = (av.allocation * rust_decimal::Decimal::from(100))
                .to_u64()
                .unwrap_or(0);
            (app.config.symbol(&av.asset), pct)
        })
        .collect();
    let bar_refs: Vec<(&str, u64)> = bars.iter().map(|(s, v)| (s.as_str(), *v)).collect();
    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(" Allocation (%) "))
        .data(&bar_refs)
        .bar_width(7)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    f.render_widget(chart, chunks[1]);
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

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        let mut q = HashMap::new();
        q.insert("bitcoin".into(), Quote { price: dec!(50000), change_24h: dec!(0),
            change_7d: None, market_cap: None, volume_24h: None, ath: None });
        a.set_prices(PriceBook { quotes: q, fetched_at: Utc::now(),
            sparklines: HashMap::new(), stale: false });
        a
    }

    #[test]
    fn shows_headline_totals() {
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| crate::ui::valuation::render(f, f.area(), &app())).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("Total Value"));
        assert!(s.contains("Unrealized"));
        assert!(s.contains("Allocation"));
    }
}
