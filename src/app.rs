//! Central application state and the recompute logic that keeps derived
//! reports in sync with the active method, tax year, and prices.

use std::collections::{BTreeMap, HashMap};

use coinbasis::{CapitalGainsReport, CostBasisMethod, IncomeReport, PortfolioReport};
use cryptolytics::allocation::TargetStrategy;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::config::Config;
use crate::error::AppError;
use crate::portfolio::{HoldingValue, PortfolioModel};
use crate::prices::{HistoryData, PriceBook};
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
        View::Prices,
        View::Holdings,
        View::Valuation,
        View::Tax,
        View::Rebalance,
        View::Performance,
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

/// The Performance view has two presentations: a value/P&L chart over time,
/// and a per-day scrollable playback with a holdings breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PerfMode {
    #[default]
    Chart,
    Playback,
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
    pub const ALL: [SortKey; 5] = [
        SortKey::Symbol,
        SortKey::Price,
        SortKey::Change24h,
        SortKey::Value,
        SortKey::Profit,
    ];
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
    /// Per-coin daily volatility (std-dev of daily returns).
    pub vols_daily: BTreeMap<String, f64>,
    /// Per-coin annualized volatility.
    pub vols_annual: BTreeMap<String, f64>,
    /// Pairwise return correlations (coins with history only).
    pub correlation: BTreeMap<(String, String), f64>,
    /// Value-weighted portfolio daily volatility (needs >=2 coins with history).
    pub portfolio_vol: Option<f64>,
    /// Buy-and-hold return over the history window at current weights.
    pub backtest_current: Option<f64>,
    /// Buy-and-hold return over the history window at target weights.
    pub backtest_target: Option<f64>,
    /// Ledger-replay snapshots over the price-history window (empty when no history).
    pub reconstructed: Vec<crate::perf::Snapshot>,
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
    /// Seconds remaining until the next automatic price refresh (driven by the run loop).
    pub seconds_to_refresh: u64,
    /// Per-coin daily price history feeding the rebalance analytics.
    pub price_history: HistoryData,
    /// How rebalance target weights are derived.
    pub target_strategy: TargetStrategy,
    /// Presentation mode for the Performance view.
    pub perf_mode: PerfMode,
}

impl App {
    pub fn new(config: Config, txs: &[coinbasis::Transaction]) -> Result<App, AppError> {
        let model = PortfolioModel::new(txs)?;
        let method = config.default_method;
        let strategy = config.rebalance.strategy;
        let refresh = config.refresh_seconds;
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
            seconds_to_refresh: refresh,
            price_history: HashMap::new(),
            target_strategy: TargetStrategy::Custom,
            perf_mode: PerfMode::Chart,
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
        self.prices
            .as_ref()
            .map(|b| b.price_map())
            .unwrap_or_default()
    }

    /// Recompute all derived reports from current method / year / prices.
    pub fn recompute(&mut self) {
        let pm = self.price_map();

        self.derived.holdings = self
            .model
            .holdings_with_value(self.method, &pm)
            .unwrap_or_default();
        self.derived.valuation = self.model.valuation(self.method, &pm).ok();
        self.derived.capital_gains = self.model.capital_gains(self.method, self.tax_year).ok();
        self.derived.income = Some(self.model.income(self.tax_year));

        // Target weights honor the selected strategy (falls back to config targets).
        let targets_dec = self.target_weights_decimal();

        if let Some(report) = &self.derived.valuation {
            let mut actions = rebalance::suggest(
                report,
                &targets_dec,
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
                        self.model.transactions(),
                        &a.asset,
                        a.amount_usd,
                        price,
                        now,
                    )
                    .ok();
                }
            }
            self.derived.rebalance_summary = Some(rebalance::summarize(&actions));
            self.derived.rebalance_actions = actions;
        } else {
            self.derived.rebalance_actions.clear();
            self.derived.rebalance_summary = None;
        }

        self.recompute_analytics();
    }

    /// Compute volatility, correlation, portfolio vol, and backtests from the
    /// per-coin price history. Cleared fields when there is insufficient data.
    fn recompute_analytics(&mut self) {
        // Per-coin return series (f64) from price history.
        let mut returns_by_coin: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for (coin, series) in &self.price_history {
            let prices: Vec<f64> = series.iter().map(|(_, p)| *p).collect();
            let r = cryptolytics::returns::daily_returns(&prices);
            if !r.is_empty() {
                returns_by_coin.insert(coin.clone(), r);
            }
        }
        self.derived.vols_daily = returns_by_coin
            .iter()
            .filter_map(|(c, r)| cryptolytics::volatility::volatility(r).map(|v| (c.clone(), v)))
            .collect();
        self.derived.vols_annual = self
            .derived
            .vols_daily
            .iter()
            .map(|(c, v)| (c.clone(), cryptolytics::volatility::annualize(*v, 365.0)))
            .collect();
        self.derived.correlation = if returns_by_coin.len() >= 2 {
            cryptolytics::correlation::correlation_matrix(&returns_by_coin)
        } else {
            BTreeMap::new()
        };

        if let Some(report) = &self.derived.valuation {
            let total = report.total_value.to_f64().unwrap_or(0.0);
            // value-weighted portfolio vol over coins with history
            if total > 0.0 && self.derived.vols_daily.len() >= 2 {
                let weights: BTreeMap<String, f64> = report
                    .assets
                    .iter()
                    .filter(|a| self.derived.vols_daily.contains_key(&a.asset))
                    .map(|a| {
                        (
                            a.asset.clone(),
                            a.market_value.to_f64().unwrap_or(0.0) / total,
                        )
                    })
                    .collect();
                self.derived.portfolio_vol = Some(cryptolytics::portfolio::portfolio_volatility(
                    &weights,
                    &self.derived.vols_daily,
                    &self.derived.correlation,
                ));
            } else {
                self.derived.portfolio_vol = None;
            }
            // backtest current vs target weights
            let hist_prices: BTreeMap<String, Vec<f64>> = self
                .price_history
                .iter()
                .map(|(c, s)| (c.clone(), s.iter().map(|(_, p)| *p).collect()))
                .collect();
            let cur_w: BTreeMap<String, f64> = report
                .assets
                .iter()
                .map(|a| {
                    (
                        a.asset.clone(),
                        if total > 0.0 {
                            a.market_value.to_f64().unwrap_or(0.0) / total
                        } else {
                            0.0
                        },
                    )
                })
                .collect();
            self.derived.backtest_current = Some(cryptolytics::backtest::buy_and_hold_return(
                &hist_prices,
                &cur_w,
            ));
            let tgt_w = self.target_weights_f64();
            self.derived.backtest_target = Some(cryptolytics::backtest::buy_and_hold_return(
                &hist_prices,
                &tgt_w,
            ));
        } else {
            self.derived.portfolio_vol = None;
            self.derived.backtest_current = None;
            self.derived.backtest_target = None;
        }

        // Ledger-replay reconstruction over the history window.
        self.derived.reconstructed = if self.price_history.is_empty() {
            Vec::new()
        } else {
            crate::perf::reconstruct_series(
                self.model.transactions(),
                &self.price_history,
                self.method,
            )
        };
    }

    /// Flip between chart and playback presentation in the Performance view.
    pub fn toggle_playback(&mut self) {
        self.perf_mode = match self.perf_mode {
            PerfMode::Chart => PerfMode::Playback,
            PerfMode::Playback => PerfMode::Chart,
        };
    }

    /// Target weights (f64) under the active [`TargetStrategy`]; on error falls
    /// back to the normalized config targets.
    fn target_weights_f64(&self) -> BTreeMap<String, f64> {
        let assets: Vec<String> = self.assets_for_targets();
        let custom: BTreeMap<String, f64> = self
            .config
            .normalized_targets()
            .into_iter()
            .map(|(k, v)| (k, v.to_f64().unwrap_or(0.0)))
            .collect();
        let market_caps: BTreeMap<String, f64> = self
            .prices
            .as_ref()
            .map(|b| {
                b.quotes
                    .iter()
                    .filter_map(|(k, q)| {
                        q.market_cap
                            .and_then(|m| m.to_f64())
                            .map(|c| (k.clone(), c))
                    })
                    .collect()
            })
            .unwrap_or_default();
        cryptolytics::allocation::target_weights(
            self.target_strategy,
            &assets,
            Some(&market_caps),
            Some(&custom),
        )
        .unwrap_or(custom)
    }

    /// Target weights as `Decimal`, for feeding `rebalance::suggest`.
    fn target_weights_decimal(&self) -> BTreeMap<String, Decimal> {
        let f = self.target_weights_f64();
        if f.is_empty() {
            return self.config.targets.clone();
        }
        f.into_iter()
            .filter_map(|(k, v)| Decimal::from_f64_retain(v).map(|d| (k, d)))
            .collect()
    }

    /// Asset universe for target-weight strategies: the union of held assets
    /// (from the valuation) and configured target keys.
    fn assets_for_targets(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> =
            self.config.targets.keys().cloned().collect();
        if let Some(report) = &self.derived.valuation {
            for a in &report.assets {
                set.insert(a.asset.clone());
            }
        }
        set.into_iter().collect()
    }

    /// Replace the price history and recompute derived analytics.
    pub fn set_price_history(&mut self, h: HistoryData) {
        self.price_history = h;
        self.recompute();
    }

    /// Cycle the target-weight strategy: Custom -> Equal -> MarketCap -> Custom.
    pub fn cycle_target_strategy(&mut self) {
        self.target_strategy = match self.target_strategy {
            TargetStrategy::Custom => TargetStrategy::Equal,
            TargetStrategy::Equal => TargetStrategy::MarketCap,
            TargetStrategy::MarketCap => TargetStrategy::Custom,
        };
        self.recompute();
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
    app.prices
        .as_ref()
        .map(|b| b.fetched_at)
        .unwrap_or_else(chrono::Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use chrono::{TimeZone, Utc};
    use coinbasis::{CostBasisMethod, Transaction};
    use rust_decimal_macros::dec;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(30000),
            fee: dec!(0),
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
    fn recompute_populates_rebalance_analytics() {
        use chrono::{TimeZone, Utc};
        let mut a = app();
        let mut hist = std::collections::HashMap::new();
        hist.insert(
            "bitcoin".to_string(),
            vec![
                (Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(), 100.0),
                (Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(), 110.0),
                (Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap(), 105.0),
            ],
        );
        a.set_price_history(hist);
        assert!(a.derived.vols_daily.contains_key("bitcoin"));
        assert_eq!(
            a.target_strategy,
            cryptolytics::allocation::TargetStrategy::Custom
        );
        a.cycle_target_strategy();
        assert_eq!(
            a.target_strategy,
            cryptolytics::allocation::TargetStrategy::Equal
        );
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
