//! Application configuration loaded from `config.json`.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use coinbasis::tax::TaxConfig;
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
    #[serde(default)]
    pub coingecko: CoinGeckoConfig,
    #[serde(default = "default_history_days")]
    pub history_days: u32,
}

fn default_history_days() -> u32 {
    90
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Plan {
    #[default]
    Demo,
    Pro,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoinGeckoConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub plan: Plan,
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
                jurisdiction: "US".into(),
                ..TaxConfig::default()
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
            coingecko: CoinGeckoConfig::default(),
            history_days: 90,
        }
    }

    /// CoinGecko API key, preferring the `COINGECKO_API_KEY` env var over config.
    pub fn coingecko_key(&self) -> Option<String> {
        std::env::var("COINGECKO_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| self.coingecko.api_key.clone())
            .filter(|s| !s.is_empty())
    }
}

/// A loaded config plus the directory it came from (used to resolve relative
/// paths such as `ledger_path`) and, on first run, the starter file we wrote.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub config: Config,
    pub dir: PathBuf,
    pub created_starter: Option<PathBuf>,
}

/// The directory containing `path`, or `.` when it has no parent component.
fn dir_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Resolve a possibly-relative path against `base_dir`. Absolute paths are
/// returned unchanged.
pub fn resolve_relative(base_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    }
}

/// Compute the global config path from explicit XDG/HOME values. Pure; the
/// env-reading wrapper is [`global_config_path`].
fn global_config_path_from(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let base = xdg
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|s| !s.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("crypto-price-tracker-v2").join("config.json"))
}

/// Global config path: `$XDG_CONFIG_HOME/crypto-price-tracker-v2/config.json`,
/// falling back to `$HOME/.config/...`. `None` if neither env var is set.
pub fn global_config_path() -> Option<PathBuf> {
    global_config_path_from(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Resolve which config to load. Precedence: explicit `--config` (must exist) >
/// `cwd_config` > `global_config` > write a starter to the global path and use
/// it. With no global path available, falls back to built-in defaults without
/// writing anything.
pub fn resolve(
    explicit: Option<&str>,
    cwd_config: &Path,
    global_config: Option<&Path>,
) -> Result<ResolvedConfig, AppError> {
    if let Some(p) = explicit {
        let config = Config::load(p)?;
        return Ok(ResolvedConfig {
            config,
            dir: dir_of(Path::new(p)),
            created_starter: None,
        });
    }
    if cwd_config.exists() {
        let config = Config::load(&cwd_config.to_string_lossy())?;
        return Ok(ResolvedConfig {
            config,
            dir: dir_of(cwd_config),
            created_starter: None,
        });
    }
    if let Some(g) = global_config {
        if g.exists() {
            let config = Config::load(&g.to_string_lossy())?;
            return Ok(ResolvedConfig {
                config,
                dir: dir_of(g),
                created_starter: None,
            });
        }
        // First run: write a starter config to the global path, then use it.
        let config = Config::example();
        let path = g.to_string_lossy().into_owned();
        if let Some(parent) = g.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Config {
                path: path.clone(),
                reason: e.to_string(),
            })?;
        }
        let text = serde_json::to_string_pretty(&config).map_err(|e| AppError::Config {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        std::fs::write(g, text).map_err(|e| AppError::Config {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        return Ok(ResolvedConfig {
            config,
            dir: dir_of(g),
            created_starter: Some(g.to_path_buf()),
        });
    }
    // No global path available: built-in defaults, write nothing.
    Ok(ResolvedConfig {
        config: Config::example(),
        dir: PathBuf::from("."),
        created_starter: None,
    })
}

/// Convenience wrapper over [`resolve`] using the real CWD and global paths.
pub fn resolve_default(explicit: Option<&str>) -> Result<ResolvedConfig, AppError> {
    let cwd = PathBuf::from("config.json");
    let global = global_config_path();
    resolve(explicit, &cwd, global.as_deref())
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
          "tax": { "jurisdiction": "US", "long_term_threshold_days": 365,
                   "short_term_rate": "0.35",
                   "long_term_brackets": [ {"up_to": "47025", "rate": "0.0"},
                                           {"up_to": null, "rate": "0.20"} ] },
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
        assert_eq!(c.tax.short_term_rate, rust_decimal_macros::dec!(0.35));
        assert_eq!(c.tax.long_term_threshold_days, 365);
        assert_eq!(c.tax.long_term_brackets.len(), 2);
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
    fn parses_coingecko_key_and_plan() {
        let json = r#"{ "api_key": "abc", "plan": "Pro" }"#;
        let c: CoinGeckoConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.api_key.as_deref(), Some("abc"));
        assert_eq!(c.plan, Plan::Pro);
    }

    #[test]
    fn example_is_serializable() {
        let c = Config::example();
        let s = serde_json::to_string_pretty(&c).unwrap();
        let round: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(round.default_method, c.default_method);
    }

    use std::path::Path;

    fn write_file(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn resolve_prefers_explicit_config() {
        let dir = std::env::temp_dir().join("cpt2_cfg_explicit");
        let _ = std::fs::remove_dir_all(&dir);
        let explicit = dir.join("custom.json");
        let cwd = dir.join("config.json");
        write_file(&explicit, sample_json());
        write_file(&cwd, sample_json());
        let r = resolve(Some(explicit.to_str().unwrap()), &cwd, None).unwrap();
        assert_eq!(r.dir, dir);
        assert!(r.created_starter.is_none());
    }

    #[test]
    fn resolve_uses_cwd_over_global() {
        let dir = std::env::temp_dir().join("cpt2_cfg_cwd");
        let _ = std::fs::remove_dir_all(&dir);
        let cwd = dir.join("config.json");
        let global = dir.join("g").join("config.json");
        write_file(&cwd, sample_json());
        write_file(&global, sample_json());
        let r = resolve(None, &cwd, Some(&global)).unwrap();
        assert_eq!(r.dir, dir); // cwd's parent wins
        assert!(r.created_starter.is_none());
    }

    #[test]
    fn resolve_uses_global_when_no_cwd() {
        let dir = std::env::temp_dir().join("cpt2_cfg_global");
        let _ = std::fs::remove_dir_all(&dir);
        let cwd = dir.join("nope").join("config.json"); // does not exist
        let global = dir.join("g").join("config.json");
        write_file(&global, sample_json());
        let r = resolve(None, &cwd, Some(&global)).unwrap();
        assert_eq!(r.dir, dir.join("g"));
        assert!(r.created_starter.is_none());
    }

    #[test]
    fn resolve_creates_starter_when_none_exists() {
        let dir = std::env::temp_dir().join("cpt2_cfg_starter");
        let _ = std::fs::remove_dir_all(&dir);
        let cwd = dir.join("nope").join("config.json");
        let global = dir.join("g").join("config.json");
        let r = resolve(None, &cwd, Some(&global)).unwrap();
        assert_eq!(r.created_starter.as_deref(), Some(global.as_path()));
        assert!(global.exists(), "starter file should be written");
        let reloaded = Config::load(global.to_str().unwrap()).unwrap();
        assert_eq!(reloaded.default_method, r.config.default_method);
    }

    #[test]
    fn resolve_relative_joins_base_keeps_absolute() {
        let base = Path::new("/tmp/cfgbase");
        assert_eq!(
            resolve_relative(base, "ledger.json"),
            base.join("ledger.json")
        );
        let abs = "/var/data/ledger.json";
        assert_eq!(resolve_relative(base, abs), PathBuf::from(abs));
    }

    #[test]
    fn global_config_path_prefers_xdg_then_home() {
        use std::ffi::OsStr;
        let p = global_config_path_from(Some(OsStr::new("/x/cfg")), Some(OsStr::new("/home/u")))
            .unwrap();
        assert_eq!(
            p,
            PathBuf::from("/x/cfg/crypto-price-tracker-v2/config.json")
        );
        let p2 = global_config_path_from(None, Some(OsStr::new("/home/u"))).unwrap();
        assert_eq!(
            p2,
            PathBuf::from("/home/u/.config/crypto-price-tracker-v2/config.json")
        );
        assert!(global_config_path_from(None, None).is_none());
    }
}
