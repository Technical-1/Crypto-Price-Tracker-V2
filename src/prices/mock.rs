//! Deterministic in-memory price source for tests.

use std::collections::HashMap;

use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;

use super::{PriceBook, PriceSource, Quote};
use crate::error::AppError;

#[derive(Default)]
pub struct MockSource {
    quotes: HashMap<String, Quote>,
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
    async fn omits_unconfigured_ids() {
        let src = MockSource::new();
        let book = src.fetch(&["dogecoin".to_string()], "usd").await.unwrap();
        assert!(book.quotes.is_empty());
    }
}
