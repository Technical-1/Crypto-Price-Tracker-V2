//! On-disk cache of the last fetched `PriceBook`, with TTL freshness and a
//! last-good fallback for offline / failed fetches.

use std::path::PathBuf;

use chrono::Utc;

use super::PriceBook;
use crate::error::AppError;

pub struct PriceCache {
    dir: PathBuf,
    ttl_seconds: i64,
}

impl PriceCache {
    pub fn new(dir: PathBuf, ttl_seconds: u64) -> Self {
        Self { dir, ttl_seconds: ttl_seconds as i64 }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join("pricebook.json")
    }

    pub fn store(&self, book: &PriceBook) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| AppError::Cache(e.to_string()))?;
        let text = serde_json::to_string(book).map_err(|e| AppError::Cache(e.to_string()))?;
        std::fs::write(self.path(), text).map_err(|e| AppError::Cache(e.to_string()))
    }

    fn read(&self) -> Result<Option<PriceBook>, AppError> {
        match std::fs::read_to_string(self.path()) {
            Ok(text) => {
                let book = serde_json::from_str(&text)
                    .map_err(|e| AppError::Cache(e.to_string()))?;
                Ok(Some(book))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AppError::Cache(e.to_string())),
        }
    }

    /// Returns the cached book only if within TTL.
    pub fn load_fresh(&self) -> Result<Option<PriceBook>, AppError> {
        let Some(book) = self.read()? else { return Ok(None) };
        let age = Utc::now().signed_duration_since(book.fetched_at).num_seconds();
        if age < self.ttl_seconds {
            Ok(Some(book))
        } else {
            Ok(None)
        }
    }

    /// Returns the cached book regardless of age, marked `stale`.
    pub fn load_last_good(&self) -> Result<Option<PriceBook>, AppError> {
        let Some(mut book) = self.read()? else { return Ok(None) };
        book.stale = true;
        Ok(Some(book))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prices::{PriceBook, Quote};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn book() -> PriceBook {
        let mut quotes = HashMap::new();
        quotes.insert("bitcoin".into(), Quote {
            price: dec!(50000), change_24h: dec!(1.0), change_7d: None,
            market_cap: None, volume_24h: None, ath: None });
        PriceBook { quotes, fetched_at: Utc::now(), sparklines: HashMap::new(), stale: false }
    }

    #[test]
    fn store_then_load_within_ttl_hits() {
        let dir = std::env::temp_dir().join("cpt2_cache_hit");
        let cache = PriceCache::new(dir.clone(), 3600);
        let _ = std::fs::remove_file(cache.path());
        cache.store(&book()).unwrap();
        let loaded = cache.load_fresh().unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().quotes["bitcoin"].price, dec!(50000));
    }

    #[test]
    fn load_fresh_misses_when_expired() {
        let dir = std::env::temp_dir().join("cpt2_cache_expired");
        let cache = PriceCache::new(dir.clone(), 0); // ttl 0 => always stale
        cache.store(&book()).unwrap();
        assert!(cache.load_fresh().unwrap().is_none());
    }

    #[test]
    fn load_last_good_returns_stale_marked_book() {
        let dir = std::env::temp_dir().join("cpt2_cache_lastgood");
        let cache = PriceCache::new(dir.clone(), 0);
        cache.store(&book()).unwrap();
        let lg = cache.load_last_good().unwrap().unwrap();
        assert!(lg.stale);
        assert_eq!(lg.quotes["bitcoin"].price, dec!(50000));
    }

    #[test]
    fn load_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("cpt2_cache_missing");
        let cache = PriceCache::new(dir.clone(), 3600);
        let _ = std::fs::remove_file(cache.path());
        assert!(cache.load_fresh().unwrap().is_none());
        assert!(cache.load_last_good().unwrap().is_none());
    }
}
