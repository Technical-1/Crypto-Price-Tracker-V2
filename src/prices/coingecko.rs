//! CoinGecko-backed `PriceSource` using the one-shot `coins_markets` endpoint,
//! which returns price, 24h/7d change, market cap, volume, and a 7d sparkline
//! in a single call.

use std::collections::HashMap;

use chrono::Utc;
use coingecko::params::{MarketsOrder, PriceChangePercentage};
use coingecko::CoinGeckoClient;
use rust_decimal::Decimal;

use super::{PriceBook, PriceSource, Quote};
use crate::config::Plan;
use crate::error::AppError;

pub struct CoinGeckoSource {
    client: CoinGeckoClient,
}

impl CoinGeckoSource {
    /// Build a client for the given API key and plan. With no key, falls back
    /// to the public/demo endpoint.
    pub fn new(api_key: Option<&str>, plan: Plan) -> Self {
        let client = match (api_key, plan) {
            (Some(k), Plan::Pro) => CoinGeckoClient::new_with_pro_api_key(k),
            (Some(k), Plan::Demo) => CoinGeckoClient::new_with_demo_api_key(k),
            (None, _) => CoinGeckoClient::new(coingecko::COINGECKO_API_DEMO_URL),
        };
        Self { client }
    }
}

impl Default for CoinGeckoSource {
    fn default() -> Self {
        Self::new(None, Plan::Demo)
    }
}

pub(crate) fn to_decimal(v: Option<f64>) -> Decimal {
    v.and_then(Decimal::from_f64_retain)
        .unwrap_or(Decimal::ZERO)
}

pub(crate) fn to_opt_decimal(v: Option<f64>) -> Option<Decimal> {
    v.and_then(Decimal::from_f64_retain)
}

impl PriceSource for CoinGeckoSource {
    async fn fetch(&self, ids: &[String], vs: &str) -> Result<PriceBook, AppError> {
        if ids.is_empty() {
            return Ok(PriceBook {
                quotes: HashMap::new(),
                fetched_at: Utc::now(),
                sparklines: HashMap::new(),
                stale: false,
            });
        }
        let items = self
            .client
            .coins_markets(
                vs,
                ids,
                None,
                MarketsOrder::MarketCapDesc,
                ids.len() as i64,
                1,
                true,
                &[
                    PriceChangePercentage::TwentyFourHours,
                    PriceChangePercentage::SevenDays,
                ],
            )
            .await
            .map_err(|e| AppError::Price(e.to_string()))?;

        let mut quotes = HashMap::new();
        let mut sparklines = HashMap::new();
        for item in items {
            quotes.insert(
                item.id.clone(),
                Quote {
                    price: to_decimal(item.current_price),
                    change_24h: to_decimal(item.price_change_percentage24_h),
                    change_7d: to_opt_decimal(item.price_change_percentage7_d_in_currency),
                    market_cap: to_opt_decimal(item.market_cap),
                    volume_24h: to_opt_decimal(item.total_volume),
                    ath: to_opt_decimal(item.ath),
                },
            );
            if let Some(spark) = item.sparkline_in7_d {
                sparklines.insert(item.id, spark.price);
            }
        }

        Ok(PriceBook {
            quotes,
            fetched_at: Utc::now(),
            sparklines,
            stale: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn f64_opt_to_decimal_handles_none_and_value() {
        assert_eq!(to_decimal(Some(50000.5)), dec!(50000.5));
        assert_eq!(to_decimal(None), dec!(0));
    }

    #[test]
    fn opt_decimal_preserves_none() {
        assert_eq!(to_opt_decimal(None), None);
        assert_eq!(to_opt_decimal(Some(3.0)), Some(dec!(3)));
    }
}
