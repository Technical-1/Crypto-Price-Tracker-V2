//! Rebalancing: compute per-asset drift vs. target weights and suggest trades.
//! Pure and deterministic. Tax-aware sell-gain estimates are filled in by the
//! caller via `crate::portfolio::estimate_sell_gain`.

use std::collections::BTreeMap;

use coinbasis::PortfolioReport;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    Band,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebalanceSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceAction {
    pub asset: String,
    pub current_value: Decimal,
    pub target_value: Decimal,
    pub drift: Decimal,
    pub side: RebalanceSide,
    pub amount_usd: Decimal,
    /// Estimated realized gain if this sell executes (HIFO). Filled by caller.
    pub est_realized_gain: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceSummary {
    pub total_buys: Decimal,
    pub total_sells: Decimal,
    pub in_balance: bool,
}

/// Suggest rebalancing trades. `targets` are weights (need not be normalized;
/// they are normalized internally).
pub fn suggest(
    report: &PortfolioReport,
    targets: &BTreeMap<String, Decimal>,
    band: Decimal,
    min_trade_usd: Decimal,
    strategy: Strategy,
) -> Vec<RebalanceAction> {
    let total = report.total_value;
    if total.is_zero() {
        return Vec::new();
    }
    let weight_sum: Decimal = targets.values().copied().sum();
    if weight_sum.is_zero() {
        return Vec::new();
    }

    let mut current: BTreeMap<String, Decimal> = BTreeMap::new();
    for av in &report.assets {
        current.insert(av.asset.clone(), av.market_value);
    }

    let mut keys: Vec<String> = current.keys().cloned().collect();
    for k in targets.keys() {
        if !current.contains_key(k) {
            keys.push(k.clone());
        }
    }
    keys.sort();
    keys.dedup();

    let mut actions = Vec::new();
    for asset in keys {
        let current_value = current.get(&asset).copied().unwrap_or(Decimal::ZERO);
        let weight = targets.get(&asset).copied().unwrap_or(Decimal::ZERO) / weight_sum;
        let target_value = total * weight;
        let drift = current_value - target_value; // + => overweight (sell)
        let drift_fraction = (drift / total).abs();

        match strategy {
            Strategy::Band if drift_fraction <= band => continue,
            _ => {}
        }

        let amount = drift.abs();
        if amount < min_trade_usd {
            continue;
        }

        let (side, amount_usd) = if drift > Decimal::ZERO {
            (RebalanceSide::Sell, amount.min(current_value))
        } else {
            (RebalanceSide::Buy, amount)
        };
        if amount_usd < min_trade_usd {
            continue;
        }

        actions.push(RebalanceAction {
            asset,
            current_value,
            target_value,
            drift,
            side,
            amount_usd,
            est_realized_gain: None,
        });
    }
    actions
}

pub fn summarize(actions: &[RebalanceAction]) -> RebalanceSummary {
    let total_buys = actions
        .iter()
        .filter(|a| a.side == RebalanceSide::Buy)
        .map(|a| a.amount_usd)
        .sum();
    let total_sells = actions
        .iter()
        .filter(|a| a.side == RebalanceSide::Sell)
        .map(|a| a.amount_usd)
        .sum();
    RebalanceSummary { total_buys, total_sells, in_balance: actions.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coinbasis::{AssetValuation, PortfolioReport};
    use rust_decimal_macros::dec;
    use std::collections::BTreeMap;

    fn report() -> PortfolioReport {
        PortfolioReport {
            assets: vec![
                AssetValuation { asset: "bitcoin".into(), quantity: dec!(1), cost_basis: dec!(5000),
                    price: dec!(7000), market_value: dec!(7000), unrealized: dec!(2000), allocation: dec!(0.7) },
                AssetValuation { asset: "ethereum".into(), quantity: dec!(1), cost_basis: dec!(2000),
                    price: dec!(3000), market_value: dec!(3000), unrealized: dec!(1000), allocation: dec!(0.3) },
            ],
            total_cost: dec!(7000), total_value: dec!(10000),
            total_unrealized: dec!(3000), total_return: dec!(0.428),
            missing_prices: vec![],
        }
    }

    fn targets() -> BTreeMap<String, rust_decimal::Decimal> {
        let mut t = BTreeMap::new();
        t.insert("bitcoin".into(), dec!(0.6));
        t.insert("ethereum".into(), dec!(0.4));
        t
    }

    #[test]
    fn band_strategy_suggests_sell_btc_buy_eth() {
        let actions = suggest(&report(), &targets(), dec!(0.05), dec!(25), Strategy::Band);
        let btc = actions.iter().find(|a| a.asset == "bitcoin").unwrap();
        assert_eq!(btc.side, RebalanceSide::Sell);
        assert_eq!(btc.amount_usd, dec!(1000));
        let eth = actions.iter().find(|a| a.asset == "ethereum").unwrap();
        assert_eq!(eth.side, RebalanceSide::Buy);
    }

    #[test]
    fn band_strategy_skips_within_band() {
        let mut t = BTreeMap::new();
        t.insert("bitcoin".into(), dec!(0.7));
        t.insert("ethereum".into(), dec!(0.3));
        let actions = suggest(&report(), &t, dec!(0.05), dec!(25), Strategy::Band);
        assert!(actions.is_empty());
    }

    #[test]
    fn min_trade_filters_tiny_drifts() {
        let actions = suggest(&report(), &targets(), dec!(0.0), dec!(2000), Strategy::Full);
        assert!(actions.is_empty());
    }

    #[test]
    fn summary_reports_in_balance_when_no_actions() {
        let mut t = BTreeMap::new();
        t.insert("bitcoin".into(), dec!(0.7));
        t.insert("ethereum".into(), dec!(0.3));
        let actions = suggest(&report(), &t, dec!(0.05), dec!(25), Strategy::Band);
        let summary = summarize(&actions);
        assert!(summary.in_balance);
    }
}
