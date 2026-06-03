//! Application configuration loaded from `config.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use coinbasis::CostBasisMethod;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::rebalance::Strategy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub ledger_path: String,
    pub default_method: CostBasisMethod,
    pub display_currency: String,
    pub refresh_seconds: u64,
    pub tax: TaxConfig,
    pub targets: BTreeMap<String, Decimal>,
    pub rebalance: RebalanceConfig,
    #[serde(default)]
    pub symbols: BTreeMap<String, String>,
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxConfig {
    pub short_term_rate: f64,
    pub long_term_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceConfig {
    pub band: Decimal,
    pub min_trade_usd: Decimal,
    pub strategy: Strategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub ttl_seconds: u64,
    pub dir: String,
}

impl CacheConfig {
    /// Expand a leading `~` to `$HOME`.
    pub fn expanded_dir(&self) -> PathBuf {
        if let Some(rest) = self.dir.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(rest);
            }
        }
        PathBuf::from(&self.dir)
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Config, AppError> {
        let text = std::fs::read_to_string(path).map_err(|e| AppError::Config {
            path: path.to_string(),
            reason: e.to_string(),
        })?;
        serde_json::from_str(&text).map_err(|e| AppError::Config {
            path: path.to_string(),
            reason: e.to_string(),
        })
    }

    /// Display symbol for an asset id; falls back to the uppercased asset id.
    pub fn symbol(&self, asset: &str) -> String {
        self.symbols
            .get(asset)
            .cloned()
            .unwrap_or_else(|| asset.to_uppercase())
    }

    /// Configured target weight for an asset, or zero if unset.
    pub fn target_weight(&self, asset: &str) -> Decimal {
        self.targets.get(asset).copied().unwrap_or(Decimal::ZERO)
    }

    /// Targets normalized to sum to 1.0. Empty map returns empty.
    pub fn normalized_targets(&self) -> BTreeMap<String, Decimal> {
        let sum: Decimal = self.targets.values().copied().sum();
        if sum.is_zero() {
            return self.targets.clone();
        }
        self.targets
            .iter()
            .map(|(k, v)| (k.clone(), v / sum))
            .collect()
    }

    pub fn example() -> Config {
        use rust_decimal_macros::dec;
        let mut targets = BTreeMap::new();
        targets.insert("bitcoin".into(), dec!(0.6));
        targets.insert("ethereum".into(), dec!(0.3));
        targets.insert("solana".into(), dec!(0.1));
        let mut symbols = BTreeMap::new();
        symbols.insert("bitcoin".into(), "BTC".into());
        symbols.insert("ethereum".into(), "ETH".into());
        symbols.insert("solana".into(), "SOL".into());
        Config {
            ledger_path: "ledger.json".into(),
            default_method: CostBasisMethod::Fifo,
            display_currency: "usd".into(),
            refresh_seconds: 60,
            tax: TaxConfig {
                short_term_rate: 0.24,
                long_term_rate: 0.15,
            },
            targets,
            rebalance: RebalanceConfig {
                band: dec!(0.05),
                min_trade_usd: dec!(25),
                strategy: Strategy::Band,
            },
            symbols,
            cache: CacheConfig {
                ttl_seconds: 30,
                dir: "~/.cache/crypto-price-tracker-v2".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_json() -> &'static str {
        r#"{
          "ledger_path": "ledger.json",
          "default_method": "Fifo",
          "display_currency": "usd",
          "refresh_seconds": 60,
          "tax": { "short_term_rate": 0.24, "long_term_rate": 0.15 },
          "targets": { "bitcoin": 0.6, "ethereum": 0.3, "solana": 0.1 },
          "rebalance": { "band": 0.05, "min_trade_usd": 25, "strategy": "Band" },
          "symbols": { "bitcoin": "BTC", "ethereum": "ETH" },
          "cache": { "ttl_seconds": 30, "dir": "~/.cache/cpt2" }
        }"#
    }

    #[test]
    fn parses_full_config() {
        let c: Config = serde_json::from_str(sample_json()).unwrap();
        assert_eq!(c.ledger_path, "ledger.json");
        assert_eq!(c.default_method, coinbasis::CostBasisMethod::Fifo);
        assert_eq!(c.refresh_seconds, 60);
        assert_eq!(c.tax.short_term_rate, 0.24);
        assert_eq!(c.rebalance.strategy, crate::rebalance::Strategy::Band);
    }

    #[test]
    fn symbol_falls_back_to_uppercased_asset() {
        let c: Config = serde_json::from_str(sample_json()).unwrap();
        assert_eq!(c.symbol("bitcoin"), "BTC");
        assert_eq!(c.symbol("solana"), "SOLANA"); // not in map -> upper(asset)
    }

    #[test]
    fn target_weight_reads_map() {
        let c: Config = serde_json::from_str(sample_json()).unwrap();
        assert_eq!(c.target_weight("bitcoin"), dec!(0.6));
        assert_eq!(c.target_weight("dogecoin"), dec!(0));
    }

    #[test]
    fn normalized_targets_sum_to_one() {
        let mut c: Config = serde_json::from_str(sample_json()).unwrap();
        c.targets.insert("bitcoin".into(), dec!(1.2)); // now sums to 1.6
        let norm = c.normalized_targets();
        let sum: rust_decimal::Decimal = norm.values().copied().sum();
        assert!((sum - dec!(1)).abs() < dec!(0.0001));
    }

    #[test]
    fn tilde_in_cache_dir_expands_to_home() {
        let c: Config = serde_json::from_str(sample_json()).unwrap();
        let dir = c.cache.expanded_dir();
        assert!(!dir.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn example_is_serializable() {
        let c = Config::example();
        let s = serde_json::to_string_pretty(&c).unwrap();
        let round: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(round.default_method, c.default_method);
    }
}
