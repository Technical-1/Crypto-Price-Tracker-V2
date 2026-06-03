//! Central application state and the recompute logic that keeps derived
//! reports in sync with the active method, tax year, and prices.

use std::collections::HashMap;

use coinbasis::{CapitalGainsReport, CostBasisMethod, IncomeReport, PortfolioReport};
use rust_decimal::Decimal;

use crate::config::Config;
use crate::error::AppError;
use crate::portfolio::{HoldingValue, PortfolioModel};
use crate::prices::PriceBook;
use crate::rebalance::{self, RebalanceAction, RebalanceSummary, Strategy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Prices,
    Holdings,
    Valuation,
    Tax,
    Rebalance,
    Performance,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Prices, View::Holdings, View::Valuation,
        View::Tax, View::Rebalance, View::Performance,
    ];

    pub fn title(self) -> &'static str {
        match self {
            View::Prices => "Prices",
            View::Holdings => "Holdings",
            View::Valuation => "Valuation",
            View::Tax => "Tax",
            View::Rebalance => "Rebalance",
            View::Performance => "Performance",
        }
    }

    fn index(self) -> usize {
        View::ALL.iter().position(|&v| v == self).unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Symbol,
    Price,
    Change24h,
    Value,
    Profit,
}

impl SortKey {
    pub const ALL: [SortKey; 5] =
        [SortKey::Symbol, SortKey::Price, SortKey::Change24h, SortKey::Value, SortKey::Profit];
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub message: String,
}

/// Reports recomputed whenever method / year / prices change.
#[derive(Default)]
pub struct Derived {
    pub valuation: Option<PortfolioReport>,
    pub holdings: Vec<HoldingValue>,
    pub capital_gains: Option<CapitalGainsReport>,
    pub income: Option<IncomeReport>,
    pub rebalance_actions: Vec<RebalanceAction>,
    pub rebalance_summary: Option<RebalanceSummary>,
}

pub struct App {
    pub config: Config,
    pub model: PortfolioModel,
    pub method: CostBasisMethod,
    pub view: View,
    pub tax_year: i32,
    pub prices: Option<PriceBook>,
    pub derived: Derived,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub selected: usize,
    pub group_by_wallet: bool,
    pub strategy: Strategy,
    pub status: Status,
    pub loading: bool,
    pub show_help: bool,
    pub should_quit: bool,
    pub history: Vec<crate::perf::Snapshot>,
}

impl App {
    pub fn new(config: Config, txs: &[coinbasis::Transaction]) -> Result<App, AppError> {
        let model = PortfolioModel::new(txs)?;
        let method = config.default_method;
        let strategy = config.rebalance.strategy;
        let mut app = App {
            method,
            view: View::Prices,
            tax_year: 2024,
            prices: None,
            derived: Derived::default(),
            sort: SortKey::Value,
            sort_desc: true,
            selected: 0,
            group_by_wallet: false,
            strategy,
            status: Status::default(),
            loading: false,
            show_help: false,
            should_quit: false,
            history: Vec::new(),
            config,
            model,
        };
        if matches!(app.method, CostBasisMethod::SpecificId) {
            app.method = CostBasisMethod::Fifo;
        }
        app.recompute();
        Ok(app)
    }

    fn price_map(&self) -> HashMap<String, Decimal> {
        self.prices.as_ref().map(|b| b.price_map()).unwrap_or_default()
    }

    /// Recompute all derived reports from current method / year / prices.
    pub fn recompute(&mut self) {
        let pm = self.price_map();

        self.derived.holdings =
            self.model.holdings_with_value(self.method, &pm).unwrap_or_default();
        self.derived.valuation = self.model.valuation(self.method, &pm).ok();
        self.derived.capital_gains = self.model.capital_gains(self.method, self.tax_year).ok();
        self.derived.income = Some(self.model.income(self.tax_year));

        if let Some(report) = &self.derived.valuation {
            let mut actions = rebalance::suggest(
                report,
                &self.config.targets,
                self.config.rebalance.band,
                self.config.rebalance.min_trade_usd,
                self.strategy,
            );
            let now = report_timestamp(self);
            for a in actions.iter_mut() {
                if a.side == crate::rebalance::RebalanceSide::Sell {
                    let price = report
                        .assets
                        .iter()
                        .find(|av| av.asset == a.asset)
                        .map(|av| av.price)
                        .unwrap_or(Decimal::ZERO);
                    a.est_realized_gain = crate::portfolio::estimate_sell_gain(
                        self.model.transactions(), &a.asset, a.amount_usd, price, now,
                    ).ok();
                }
            }
            self.derived.rebalance_summary = Some(rebalance::summarize(&actions));
            self.derived.rebalance_actions = actions;
        } else {
            self.derived.rebalance_actions.clear();
            self.derived.rebalance_summary = None;
        }
    }

    pub fn next_view(&mut self) {
        let i = (self.view.index() + 1) % View::ALL.len();
        self.view = View::ALL[i];
        self.selected = 0;
    }

    pub fn prev_view(&mut self) {
        let i = (self.view.index() + View::ALL.len() - 1) % View::ALL.len();
        self.view = View::ALL[i];
        self.selected = 0;
    }

    /// Cycle the four automatic methods (never `SpecificId`).
    pub fn cycle_method(&mut self) {
        self.method = match self.method {
            CostBasisMethod::Fifo => CostBasisMethod::Lifo,
            CostBasisMethod::Lifo => CostBasisMethod::Hifo,
            CostBasisMethod::Hifo => CostBasisMethod::Average,
            CostBasisMethod::Average | CostBasisMethod::SpecificId => CostBasisMethod::Fifo,
        };
        self.recompute();
    }

    pub fn set_year(&mut self, delta: i32) {
        self.tax_year += delta;
        self.recompute();
    }

    pub fn cycle_sort(&mut self) {
        let idx = SortKey::ALL.iter().position(|&s| s == self.sort).unwrap();
        self.sort = SortKey::ALL[(idx + 1) % SortKey::ALL.len()];
    }

    pub fn toggle_grouping(&mut self) {
        self.group_by_wallet = !self.group_by_wallet;
    }

    pub fn toggle_strategy(&mut self) {
        self.strategy = match self.strategy {
            Strategy::Band => Strategy::Full,
            Strategy::Full => Strategy::Band,
        };
        self.recompute();
    }

    pub fn select_next(&mut self) {
        self.selected = self.selected.saturating_add(1);
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn set_prices(&mut self, book: PriceBook) {
        self.prices = Some(book);
        self.loading = false;
        self.recompute();
    }

    pub fn method_label(&self) -> &'static str {
        match self.method {
            CostBasisMethod::Fifo => "FIFO",
            CostBasisMethod::Lifo => "LIFO",
            CostBasisMethod::Hifo => "HIFO",
            CostBasisMethod::Average => "AVG",
            CostBasisMethod::SpecificId => "SPEC",
        }
    }

    pub fn perf_metrics(&self) -> crate::perf::PerfMetrics {
        crate::perf::metrics(&self.history)
    }
}

fn report_timestamp(app: &App) -> chrono::DateTime<chrono::Utc> {
    app.prices.as_ref().map(|b| b.fetched_at).unwrap_or_else(chrono::Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use coinbasis::{CostBasisMethod, Transaction};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        App::new(Config::example(), &txs).unwrap()
    }

    #[test]
    fn starts_on_prices_view_with_default_method() {
        let a = app();
        assert_eq!(a.view, View::Prices);
        assert_eq!(a.method, CostBasisMethod::Fifo);
    }

    #[test]
    fn cycle_method_walks_the_four_automatic_methods() {
        let mut a = app();
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Lifo);
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Hifo);
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Average);
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Fifo); // wraps, never SpecificId
    }

    #[test]
    fn next_and_prev_view_cycle_six_views() {
        let mut a = app();
        a.next_view();
        assert_eq!(a.view, View::Holdings);
        a.prev_view();
        assert_eq!(a.view, View::Prices);
        a.prev_view();
        assert_eq!(a.view, View::Performance); // wraps backward
    }

    #[test]
    fn set_year_adjusts_tax_year() {
        let mut a = app();
        let y = a.tax_year;
        a.set_year(1);
        assert_eq!(a.tax_year, y + 1);
        a.set_year(-1);
        assert_eq!(a.tax_year, y);
    }
}
