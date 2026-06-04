//! Value-history snapshots and performance metrics over `coinbasis::stats`.

use chrono::{DateTime, Utc};
use coinbasis::stats;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(alias = "at")]
    pub date: DateTime<Utc>,
    pub total_value: Decimal,
    #[serde(default)]
    pub cost: Decimal,
    #[serde(default)]
    pub pl: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerfMetrics {
    pub volatility: Option<f64>,
    pub sharpe: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub cumulative_return: Option<f64>,
    pub period_returns: Vec<f64>,
}

/// Load snapshots from a JSONL file (one object per line). A legacy pretty
/// JSON-array file is tolerated by trying to parse the whole text as an array
/// first.
pub fn load_history(path: &str) -> Result<Vec<Snapshot>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            // Legacy: a single JSON array of snapshots.
            if let Ok(arr) = serde_json::from_str::<Vec<Snapshot>>(&text) {
                return Ok(arr);
            }
            let mut out = Vec::new();
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let snap: Snapshot =
                    serde_json::from_str(line).map_err(|e| AppError::Cache(e.to_string()))?;
                out.push(snap);
            }
            Ok(out)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(AppError::Cache(e.to_string())),
    }
}

/// Append a snapshot unless the most recent one is within `min_interval_seconds`.
/// Snapshots are stored one-per-line (JSONL) as `{date,total_value,cost,pl}`.
pub fn record_snapshot(
    path: &str,
    total_value: Decimal,
    cost: Decimal,
    pl: Decimal,
    now: DateTime<Utc>,
    min_interval_seconds: i64,
) -> Result<(), AppError> {
    let mut hist = load_history(path)?;
    if let Some(last) = hist.last() {
        if now.signed_duration_since(last.date).num_seconds() < min_interval_seconds {
            return Ok(());
        }
    }
    hist.push(Snapshot {
        date: now,
        total_value,
        cost,
        pl,
    });
    let mut text = String::new();
    for snap in &hist {
        let line = serde_json::to_string(snap).map_err(|e| AppError::Cache(e.to_string()))?;
        text.push_str(&line);
        text.push('\n');
    }
    std::fs::write(path, text).map_err(|e| AppError::Cache(e.to_string()))
}

pub fn metrics(snaps: &[Snapshot]) -> PerfMetrics {
    let values: Vec<f64> = snaps
        .iter()
        .map(|s| s.total_value.to_f64().unwrap_or(0.0))
        .collect();
    let returns = stats::returns_from_values(&values);
    PerfMetrics {
        volatility: stats::volatility(&returns),
        sharpe: stats::sharpe_ratio(&returns, 0.0),
        max_drawdown: stats::max_drawdown(&values),
        cumulative_return: stats::cumulative_return(&values),
        period_returns: returns,
    }
}

/// Reconstruct daily snapshots by replaying the ledger as-of each history date.
pub fn reconstruct_series(
    txs: &[coinbasis::Transaction],
    history: &crate::prices::HistoryData,
    method: coinbasis::CostBasisMethod,
) -> Vec<Snapshot> {
    use coinbasis::Portfolio;
    use std::collections::BTreeSet;
    // union of all dated points, by date
    let mut dates: BTreeSet<chrono::DateTime<Utc>> = BTreeSet::new();
    for series in history.values() {
        for (d, _) in series {
            dates.insert(*d);
        }
    }
    let mut out = Vec::new();
    for d in dates {
        let upto: Vec<coinbasis::Transaction> =
            txs.iter().filter(|t| t.timestamp() <= d).cloned().collect();
        let prices: std::collections::HashMap<String, Decimal> = history
            .iter()
            .filter_map(|(c, s)| {
                s.iter()
                    .find(|(dt, _)| *dt == d)
                    .and_then(|(_, p)| Decimal::from_f64_retain(*p))
                    .map(|px| (c.clone(), px))
            })
            .collect();
        if let Ok(p) = Portfolio::from_transactions(&upto) {
            if let Ok(r) = p.valuation(method, &prices) {
                out.push(Snapshot {
                    date: d,
                    total_value: r.total_value,
                    cost: r.total_cost,
                    pl: r.total_unrealized,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    #[test]
    fn metrics_on_short_series_are_none_but_returns_present() {
        let snaps = vec![Snapshot {
            date: Utc::now(),
            total_value: dec!(100),
            cost: dec!(0),
            pl: dec!(0),
        }];
        let m = metrics(&snaps);
        assert!(m.volatility.is_none());
        assert!(m.cumulative_return.is_none());
    }

    #[test]
    fn metrics_compute_on_longer_series() {
        let snaps: Vec<Snapshot> = [100.0, 110.0, 105.0, 120.0]
            .iter()
            .enumerate()
            .map(|(i, v)| Snapshot {
                date: Utc
                    .with_ymd_and_hms(2026, 1, 1 + i as u32, 0, 0, 0)
                    .unwrap(),
                total_value: rust_decimal::Decimal::from_f64_retain(*v).unwrap(),
                cost: dec!(0),
                pl: dec!(0),
            })
            .collect();
        let m = metrics(&snaps);
        assert!(m.cumulative_return.unwrap() > 0.0);
        assert!(!m.period_returns.is_empty());
        assert!(m.max_drawdown.is_some());
    }

    #[test]
    fn record_snapshot_dedupes_within_interval() {
        let dir = std::env::temp_dir().join("cpt2_perf");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.jsonl");
        let _ = std::fs::remove_file(&path);
        let t0 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(100), dec!(0), dec!(0), t0, 60).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 30).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(101), dec!(0), dec!(0), t1, 60).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 1, 30).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(102), dec!(0), dec!(0), t2, 60).unwrap();
        let hist = load_history(path.to_str().unwrap()).unwrap();
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn old_snapshot_loads_with_default_cost_pl() {
        let dir = std::env::temp_dir().join("cpt2_perf_mig");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("h.jsonl");
        std::fs::write(
            &path,
            "{\"date\":\"2026-06-01T12:00:00Z\",\"total_value\":\"100\"}\n",
        )
        .unwrap();
        let h = load_history(path.to_str().unwrap()).unwrap();
        assert_eq!(h[0].cost, rust_decimal_macros::dec!(0));
    }

    #[test]
    fn reconstruct_values_holdings_as_of_each_day() {
        use chrono::{TimeZone, Utc};
        use coinbasis::{CostBasisMethod, Transaction};
        use rust_decimal_macros::dec;
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            wallet: "w".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(100),
            fee: dec!(0),
        }];
        let mut hist = std::collections::HashMap::new();
        hist.insert(
            "bitcoin".to_string(),
            vec![
                (Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(), 100.0),
                (Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(), 150.0),
            ],
        );
        let series = reconstruct_series(&txs, &hist, CostBasisMethod::Fifo);
        assert_eq!(series.len(), 2);
        assert_eq!(series[1].total_value, dec!(150)); // 1 btc * 150
        assert_eq!(series[1].pl, dec!(50)); // 150 - 100 cost
    }
}
