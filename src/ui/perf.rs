//! View 6: value-history line chart and performance metrics.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols;
use ratatui::text::Line;
use ratatui::widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    if app.history.len() < 2 {
        f.render_widget(
            Paragraph::new("Performance: not enough history yet — values are recorded as prices refresh.")
                .block(Block::default().borders(Borders::ALL).title(" Performance ")),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(8)])
        .split(area);

    let points: Vec<(f64, f64)> = app
        .history
        .iter()
        .enumerate()
        .map(|(i, s)| (i as f64, s.total_value.to_f64().unwrap_or(0.0)))
        .collect();
    let max_y = points.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let min_y = points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let last_x = (points.len() - 1) as f64;

    let datasets = vec![Dataset::default()
        .name("value")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan))
        .data(&points)];
    let chart = Chart::new(datasets)
        .block(Block::default().borders(Borders::ALL).title(" Value History "))
        .x_axis(Axis::default().bounds([0.0, last_x]))
        .y_axis(
            Axis::default()
                .bounds([min_y, max_y])
                .labels(vec![format!("{:.0}", min_y), format!("{:.0}", max_y)]),
        );
    f.render_widget(chart, chunks[0]);

    let m = app.perf_metrics();
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

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::perf::Snapshot;
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;

    fn app_with_history(points: usize) -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        a.history = (0..points)
            .map(|i| Snapshot {
                at: Utc.with_ymd_and_hms(2026, 1, 1 + i as u32, 0, 0, 0).unwrap(),
                total_value: dec!(100) + rust_decimal::Decimal::from(i),
            })
            .collect();
        a
    }

    #[test]
    fn short_history_shows_need_more_message() {
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| crate::ui::perf::render(f, f.area(), &app_with_history(1))).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("not enough history"));
    }

    #[test]
    fn longer_history_shows_metrics_labels() {
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| crate::ui::perf::render(f, f.area(), &app_with_history(8))).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("Volatility"));
        assert!(s.contains("Max Drawdown"));
    }
}
