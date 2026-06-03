//! Thin wrapper over `coinbasis::Portfolio` that recomputes reports under a
//! chosen cost-basis method and enriches holdings with live price values.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use coinbasis::{
    CapitalGainsReport, CostBasisMethod, Holding, IncomeReport, Portfolio, PortfolioReport,
    RealizedGain, Transaction,
};
use rust_decimal::Decimal;

use crate::error::AppError;

pub struct PortfolioModel {
    portfolio: Portfolio,
    txs: Vec<Transaction>,
}

/// A holding enriched with current market value and unrealized P&L,
/// computed in V2 because `coinbasis::Holding` carries neither.
#[derive(Debug, Clone, PartialEq)]
pub struct HoldingValue {
    pub holding: Holding,
    pub price: Decimal,
    pub current_value: Decimal,
    pub unrealized: Decimal,
}

impl PortfolioModel {
    pub fn new(txs: &[Transaction]) -> Result<Self, AppError> {
        let portfolio = Portfolio::from_transactions(txs)?;
        Ok(Self {
            portfolio,
            txs: txs.to_vec(),
        })
    }

    pub fn holdings(&self, method: CostBasisMethod) -> Result<Vec<Holding>, AppError> {
        Ok(self.portfolio.holdings(method)?)
    }

    pub fn holdings_with_value(
        &self,
        method: CostBasisMethod,
        prices: &HashMap<String, Decimal>,
    ) -> Result<Vec<HoldingValue>, AppError> {
        let holdings = self.portfolio.holdings(method)?;
        Ok(holdings
            .into_iter()
            .map(|h| {
                let price = prices.get(&h.asset).copied().unwrap_or(Decimal::ZERO);
                let current_value = h.quantity * price;
                let unrealized = current_value - h.cost_basis;
                HoldingValue {
                    holding: h,
                    price,
                    current_value,
                    unrealized,
                }
            })
            .collect())
    }

    pub fn realized_gains(&self, method: CostBasisMethod) -> Result<Vec<RealizedGain>, AppError> {
        Ok(self.portfolio.realized_gains(method)?)
    }

    pub fn capital_gains(
        &self,
        method: CostBasisMethod,
        year: i32,
    ) -> Result<CapitalGainsReport, AppError> {
        Ok(self.portfolio.capital_gains_report(method, year)?)
    }

    pub fn income(&self, year: i32) -> IncomeReport {
        self.portfolio.income_report(year)
    }

    pub fn valuation(
        &self,
        method: CostBasisMethod,
        prices: &HashMap<String, Decimal>,
    ) -> Result<PortfolioReport, AppError> {
        Ok(self.portfolio.valuation(method, prices)?)
    }

    pub fn transactions(&self) -> &[Transaction] {
        &self.txs
    }
}

/// Estimate the realized gain of selling `sell_value_usd` of `asset` at
/// `price`, drawing lots under HIFO (the gain-minimizing method), by diffing
/// total realized gains with vs. without a hypothetical `Sell`.
pub fn estimate_sell_gain(
    txs: &[Transaction],
    asset: &str,
    sell_value_usd: Decimal,
    price: Decimal,
    now: DateTime<Utc>,
) -> Result<Decimal, AppError> {
    if price.is_zero() || sell_value_usd <= Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }
    let quantity = sell_value_usd / price;

    let base = Portfolio::from_transactions(txs)?;
    let base_gain: Decimal = base
        .realized_gains(CostBasisMethod::Hifo)?
        .iter()
        .map(|g| g.gain)
        .sum();

    let mut hypo = txs.to_vec();
    let wallet = txs
        .iter()
        .find_map(|t| match t {
            Transaction::Buy {
                asset: a, wallet, ..
            } if a == asset => Some(wallet.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "estimate".to_string());
    hypo.push(Transaction::Sell {
        timestamp: now,
        wallet,
        asset: asset.to_string(),
        quantity,
        unit_price: price,
        fee: Decimal::ZERO,
    });

    let with = Portfolio::from_transactions(&hypo)?;
    let with_gain: Decimal = with
        .realized_gains(CostBasisMethod::Hifo)?
        .iter()
        .map(|g| g.gain)
        .sum();

    Ok(with_gain - base_gain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use coinbasis::{CostBasisMethod, Transaction};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn ledger() -> Vec<Transaction> {
        vec![
            Transaction::Buy {
                timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(),
                asset: "bitcoin".into(),
                quantity: dec!(1),
                unit_price: dec!(30000),
                fee: dec!(0),
            },
            Transaction::Buy {
                timestamp: Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(),
                asset: "bitcoin".into(),
                quantity: dec!(1),
                unit_price: dec!(40000),
                fee: dec!(0),
            },
            Transaction::Sell {
                timestamp: Utc.with_ymd_and_hms(2023, 6, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(),
                asset: "bitcoin".into(),
                quantity: dec!(0.5),
                unit_price: dec!(50000),
                fee: dec!(0),
            },
        ]
    }

    #[test]
    fn builds_from_valid_ledger() {
        assert!(PortfolioModel::new(&ledger()).is_ok());
    }

    #[test]
    fn holdings_with_value_computes_current_and_unrealized() {
        let m = PortfolioModel::new(&ledger()).unwrap();
        let mut prices = HashMap::new();
        prices.insert("bitcoin".to_string(), dec!(50000));
        let hv = m
            .holdings_with_value(CostBasisMethod::Fifo, &prices)
            .unwrap();
        let total_qty: rust_decimal::Decimal = hv.iter().map(|h| h.holding.quantity).sum();
        assert_eq!(total_qty, dec!(1.5));
        let h = &hv[0];
        assert_eq!(h.current_value, h.holding.quantity * dec!(50000));
        assert_eq!(h.unrealized, h.current_value - h.holding.cost_basis);
    }

    #[test]
    fn capital_gains_for_year_filters_rows() {
        let m = PortfolioModel::new(&ledger()).unwrap();
        let rep = m.capital_gains(CostBasisMethod::Fifo, 2023).unwrap();
        assert_eq!(rep.tax_year, 2023);
        assert!(!rep.rows.is_empty());
    }

    #[test]
    fn estimate_sell_gain_is_positive_for_appreciated_asset() {
        let txs = ledger();
        let gain = estimate_sell_gain(
            &txs,
            "bitcoin",
            dec!(10000),
            dec!(50000),
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        )
        .unwrap();
        assert!(gain > dec!(0));
    }
}
