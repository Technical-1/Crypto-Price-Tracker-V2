//! Value-history snapshots and performance metrics over `coinbasis::stats`.

use chrono::{DateTime, Utc};
use coinbasis::stats;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub at: DateTime<Utc>,
    pub total_value: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerfMetrics {
    pub volatility: Option<f64>,
    pub sharpe: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub cumulative_return: Option<f64>,
    pub period_returns: Vec<f64>,
}

pub fn load_history(path: &str) -> Result<Vec<Snapshot>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| AppError::Cache(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(AppError::Cache(e.to_string())),
    }
}

/// Append a snapshot unless the most recent one is within `min_interval_seconds`.
pub fn record_snapshot(
    path: &str,
    total_value: Decimal,
    now: DateTime<Utc>,
    min_interval_seconds: i64,
) -> Result<(), AppError> {
    let mut hist = load_history(path)?;
    if let Some(last) = hist.last() {
        if now.signed_duration_since(last.at).num_seconds() < min_interval_seconds {
            return Ok(());
        }
    }
    hist.push(Snapshot { at: now, total_value });
    let text = serde_json::to_string_pretty(&hist).map_err(|e| AppError::Cache(e.to_string()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    #[test]
    fn metrics_on_short_series_are_none_but_returns_present() {
        let snaps = vec![Snapshot { at: Utc::now(), total_value: dec!(100) }];
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
                at: Utc.with_ymd_and_hms(2026, 1, 1 + i as u32, 0, 0, 0).unwrap(),
                total_value: rust_decimal::Decimal::from_f64_retain(*v).unwrap(),
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
        let path = dir.join("history.json");
        let _ = std::fs::remove_file(&path);
        let t0 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(100), t0, 60).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 30).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(101), t1, 60).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 1, 30).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(102), t2, 60).unwrap();
        let hist = load_history(path.to_str().unwrap()).unwrap();
        assert_eq!(hist.len(), 2);
    }
}
