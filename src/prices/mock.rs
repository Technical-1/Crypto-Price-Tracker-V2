//! Deterministic in-memory price source for tests.

use std::collections::HashMap;

use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;

use super::{HistoryData, PriceBook, PriceSource, Quote};
use crate::error::AppError;

#[derive(Default)]
pub struct MockSource {
    quotes: HashMap<String, Quote>,
    history: HashMap<String, Vec<(DateTime<Utc>, f64)>>,
}

impl MockSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, asset: &str, price: Decimal, change_24h: Decimal) {
        self.quotes.insert(
            asset.to_string(),
            Quote {
                price,
                change_24h,
                change_7d: None,
                market_cap: None,
                volume_24h: None,
                ath: None,
            },
        );
    }

    pub fn set_history(&mut self, id: &str, series: Vec<(DateTime<Utc>, f64)>) {
        self.history.insert(id.to_string(), series);
    }
}

impl PriceSource for MockSource {
    async fn fetch(&self, ids: &[String], _vs: &str) -> Result<PriceBook, AppError> {
        let quotes = ids
            .iter()
            .filter_map(|id| self.quotes.get(id).map(|q| (id.clone(), q.clone())))
            .collect();
        Ok(PriceBook {
            quotes,
            fetched_at: Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap(),
            sparklines: HashMap::new(),
            stale: false,
        })
    }

    async fn fetch_history(
        &self,
        ids: &[String],
        _vs: &str,
        _days: u32,
    ) -> Result<HistoryData, AppError> {
        Ok(ids
            .iter()
            .filter_map(|id| self.history.get(id).map(|h| (id.clone(), h.clone())))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prices::PriceSource;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn returns_configured_quotes_for_requested_ids() {
        let mut src = MockSource::new();
        src.set("bitcoin", dec!(50000), dec!(2.0));
        src.set("ethereum", dec!(3000), dec!(-1.0));
        let book = src
            .fetch(&["bitcoin".to_string(), "ethereum".to_string()], "usd")
            .await
            .unwrap();
        assert_eq!(book.quotes["bitcoin"].price, dec!(50000));
        assert_eq!(book.quotes["ethereum"].change_24h, dec!(-1.0));
        assert_eq!(book.quotes.len(), 2);
    }

    #[tokio::test]
    async fn mock_history_returns_configured_series() {
        use chrono::{TimeZone, Utc};
        let mut s = MockSource::new();
        s.set_history(
            "bitcoin",
            vec![
                (Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(), 100.0),
                (Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(), 110.0),
            ],
        );
        let h = s
            .fetch_history(&["bitcoin".to_string()], "usd", 2)
            .await
            .unwrap();
        assert_eq!(h["bitcoin"].len(), 2);
        assert_eq!(h["bitcoin"][1].1, 110.0);
    }

    #[tokio::test]
    async fn omits_unconfigured_ids() {
        let src = MockSource::new();
        let book = src.fetch(&["dogecoin".to_string()], "usd").await.unwrap();
        assert!(book.quotes.is_empty());
    }
}
