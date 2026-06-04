//! View 4: capital gains + income for the selected tax year, with an estimate.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;

use coinbasis::Term;

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(7)])
        .split(area);

    let title = format!(" Tax — {} ([ / ] change year · e export) ", app.tax_year);
    let header = Row::new(
        [
            "ASSET", "ACQUIRED", "DISPOSED", "QTY", "PROCEEDS", "BASIS", "GAIN", "TERM",
        ]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );

    let rows: Vec<Row> = app
        .derived
        .capital_gains
        .as_ref()
        .map(|rep| {
            rep.rows
                .iter()
                .map(|r| {
                    let term = match r.term {
                        Some(Term::Short) => "Short",
                        Some(Term::Long) => "Long",
                        None => "—",
                    };
                    let gain_style = if r.gain >= Decimal::ZERO {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Red)
                    };
                    Row::new(vec![
                        Cell::from(app.config.symbol(&r.asset)),
                        Cell::from(
                            r.acquired_at
                                .map(|d| d.date_naive().to_string())
                                .unwrap_or_default(),
                        ),
                        Cell::from(r.disposed_at.date_naive().to_string()),
                        Cell::from(format!("{}", r.quantity)),
                        Cell::from(format!("{:.2}", r.proceeds)),
                        Cell::from(format!("{:.2}", r.cost_basis)),
                        Cell::from(format!("{:+.2}", r.gain)).style(gain_style),
                        Cell::from(term),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(7),
    ];
    f.render_widget(
        Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(title)),
        chunks[0],
    );

    let mut lines = Vec::new();
    if let Some(cg) = &app.derived.capital_gains {
        let est = coinbasis::tax::estimate(cg, &app.config.tax);
        lines.push(Line::from(format!(
            "Short-term gain: {:+.2}    tax: {:.2}",
            est.short_term_gain, est.short_term_tax
        )));
        lines.push(Line::from(format!(
            "Long-term gain:  {:+.2}    tax: {:.2}",
            est.long_term_gain, est.long_term_tax
        )));
        lines.push(Line::from(format!(
            "Total gain:      {:+.2}",
            cg.total_gain
        )));
        lines.push(Line::from(format!(
            "Estimated Tax:   {:.2}  ({}, brackets)",
            est.total_tax, app.config.tax.jurisdiction
        )));
    }
    if let Some(inc) = &app.derived.income {
        lines.push(Line::from(format!(
            "Income:          {:.2}",
            inc.total_income
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Summary ")),
        chunks[1],
    );
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use chrono::{TimeZone, Utc};
    use coinbasis::Transaction;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;

    fn app() -> App {
        let txs = vec![
            Transaction::Buy {
                timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(),
                asset: "bitcoin".into(),
                quantity: dec!(1),
                unit_price: dec!(30000),
                fee: dec!(0),
            },
            Transaction::Sell {
                timestamp: Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(),
                asset: "bitcoin".into(),
                quantity: dec!(0.5),
                unit_price: dec!(50000),
                fee: dec!(0),
            },
        ];
        let mut a = App::new(Config::example(), &txs).unwrap();
        a.tax_year = 2024;
        a.recompute();
        a
    }

    #[test]
    fn shows_tax_columns_subtotals_and_estimate() {
        let mut t = Terminal::new(TestBackend::new(140, 28)).unwrap();
        t.draw(|f| crate::ui::tax::render(f, f.area(), &app()))
            .unwrap();
        let s: String = t
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(s.contains("PROCEEDS"));
        assert!(s.contains("Short-term"));
        assert!(s.contains("Estimated Tax"));
        assert!(s.contains("2024"));
        assert!(s.contains("US"));
    }
}
