//! Price abstraction: a `PriceSource` produces a `PriceBook` of `Quote`s.

pub mod cache;
pub mod coingecko;
pub mod mock;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub price: Decimal,
    pub change_24h: Decimal,
    pub change_7d: Option<Decimal>,
    pub market_cap: Option<Decimal>,
    pub volume_24h: Option<Decimal>,
    pub ath: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceBook {
    pub quotes: HashMap<String, Quote>,
    pub fetched_at: DateTime<Utc>,
    pub sparklines: HashMap<String, Vec<f64>>,
    /// True when served from cache after a live fetch failed.
    #[serde(default)]
    pub stale: bool,
}

impl PriceBook {
    /// asset id -> price, for `coinbasis::Portfolio::valuation`.
    pub fn price_map(&self) -> HashMap<String, Decimal> {
        self.quotes
            .iter()
            .map(|(k, q)| (k.clone(), q.price))
            .collect()
    }
}

#[allow(async_fn_in_trait)]
pub trait PriceSource {
    /// Fetch quotes for `ids` priced in `vs` (e.g. "usd").
    async fn fetch(&self, ids: &[String], vs: &str) -> Result<PriceBook, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    #[test]
    fn price_map_extracts_decimal_prices() {
        let mut quotes = HashMap::new();
        quotes.insert(
            "bitcoin".to_string(),
            Quote {
                price: dec!(50000),
                change_24h: dec!(1.5),
                change_7d: Some(dec!(3.0)),
                market_cap: None,
                volume_24h: None,
                ath: None,
            },
        );
        let book = PriceBook {
            quotes,
            fetched_at: Utc::now(),
            sparklines: HashMap::new(),
            stale: false,
        };
        let pm = book.price_map();
        assert_eq!(pm.get("bitcoin"), Some(&dec!(50000)));
    }
}
