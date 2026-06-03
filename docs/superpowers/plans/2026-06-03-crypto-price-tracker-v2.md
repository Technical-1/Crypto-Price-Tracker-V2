# Crypto-Price-Tracker-V2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a fast Rust terminal UI for tracking a crypto portfolio — live prices/P&L, holdings, valuation, capital-gains tax, tax-aware rebalancing, and performance stats — as a thin presentation layer over the `coinbasis` cost-basis engine and the `coingecko` price client.

**Architecture:** Pure synchronous domain core (config, ledger, prices abstraction, portfolio wrapper, rebalance, perf, export) with zero terminal dependencies, plus a thin `ratatui`/`crossterm` TUI shell. One async `tokio` task fetches prices and hands `PriceBook` snapshots to app state; rendering reads a consistent state snapshot each frame. All views recompute from cached state when the cost-basis method, tax year, or prices change.

**Tech Stack:** Rust 2021, `ratatui` + `crossterm`, `tokio`, `coingecko` 1.1, `coinbasis` 0.1 (`serde` feature), `rust_decimal`, `serde`/`serde_json`, `chrono`, `anyhow` + `thiserror`, `clap`. Tests: built-in `#[test]`, `ratatui::TestBackend`, and a `MockSource` price source.

---

## Verified external APIs (pinned — do not guess these)

These were confirmed against docs.rs source on 2026-06-03. The implementer MUST use these exact spellings.

### `coinbasis = { version = "0.1", features = ["serde"] }`

`Transaction` is an externally-tagged enum (JSON `{"Buy": {…}}`) with 8 variants. Fields used here:
- `Buy/Sell { timestamp: DateTime<Utc>, wallet: String, asset: String, quantity: Decimal, unit_price: Decimal, fee: Decimal }`
- `Income { timestamp, wallet, asset, quantity: Decimal, value: Decimal, source: IncomeSource }`
- (also `Trade`, `Spend`, `Transfer`, `GiftSent`, `GiftReceived` — deserialized but not specially handled by V2)

`IncomeSource` (Copy): `Staking | Mining | Airdrop | Interest | Other`.

`CostBasisMethod` (Copy, serde): `Fifo | Lifo | Hifo | Average | SpecificId`.

`Portfolio` (all return `Result<_, PortfolioError>` unless noted):
- `Portfolio::from_transactions(txs: &[Transaction]) -> Result<Portfolio, PortfolioError>`
- `.holdings(method: CostBasisMethod) -> Result<Vec<Holding>, PortfolioError>`
- `.realized_gains(method) -> Result<Vec<RealizedGain>, PortfolioError>` (Err `SelectionRequired` if `SpecificId`)
- `.valuation(method, prices: &HashMap<String, Decimal>) -> Result<PortfolioReport, PortfolioError>`
- `.capital_gains_report(method, tax_year: i32) -> Result<CapitalGainsReport, PortfolioError>`
- `.income_report(tax_year: i32) -> IncomeReport` — **infallible, not a Result**

Report types (`coinbasis::` root re-exports), all `Clone + Debug + PartialEq`:
- `Holding { asset: String, wallet: String, quantity: Decimal, cost_basis: Decimal, average_cost: Decimal }` — **no value/unrealized fields**
- `RealizedGain { asset, wallet, disposed_at: DateTime<Utc>, acquired_at: Option<DateTime<Utc>>, quantity, proceeds, cost_basis, gain: Decimal, term: Option<Term> }`
- `Term { Short, Long }`
- `AssetValuation { asset, quantity, cost_basis, price, market_value, unrealized, allocation: Decimal }`
- `PortfolioReport { assets: Vec<AssetValuation>, total_cost, total_value, total_unrealized, total_return: Decimal, missing_prices: Vec<String> }`
- `CapitalGainsReport { tax_year: i32, rows: Vec<RealizedGain>, short_term_gain, long_term_gain, total_gain: Decimal }`
- `IncomeReport { tax_year: i32, events: Vec<IncomeEvent>, total_income: Decimal }`
- `IncomeEvent { asset, wallet, received_at: DateTime<Utc>, quantity, value, source: IncomeSource }`

`coinbasis::stats` (operate on `&[f64]`, **not** re-exported at root — `use coinbasis::stats;`):
- `stats::returns_from_values(values: &[f64]) -> Vec<f64>` (empty if < 2 values)
- `stats::volatility(returns: &[f64]) -> Option<f64>`
- `stats::sharpe_ratio(returns: &[f64], risk_free: f64) -> Option<f64>`
- `stats::max_drawdown(values: &[f64]) -> Option<f64>`
- `stats::cumulative_return(values: &[f64]) -> Option<f64>`

Error: `coinbasis::PortfolioError` (thiserror).

### `coingecko = "1.1"`

- Client `coingecko::CoinGeckoClient`; construct via `CoinGeckoClient::new(coingecko::COINGECKO_API_DEMO_URL)` (no key) or `CoinGeckoClient::new_with_demo_api_key(&key)`.
- Primary call (one request, everything we need):
  ```rust
  pub async fn coins_markets<Id: AsRef<str>>(
      &self, vs_currency: &str, ids: &[Id], category: Option<&str>,
      order: coingecko::params::MarketsOrder, per_page: i64, page: i64,
      sparkline: bool, price_change_percentage: &[coingecko::params::PriceChangePercentage],
  ) -> Result<Vec<coingecko::response::coins::CoinsMarketItem>, reqwest::Error>
  ```
- `CoinsMarketItem` fields used (all `Option<f64>` unless noted): `id: String`, `current_price`, `market_cap`, `total_volume`, `price_change_percentage24_h`, `price_change_percentage7_d_in_currency`, `ath`, `sparkline_in7_d: Option<SparklineIn7D>` where `SparklineIn7D { price: Vec<f64> }`.
- Param enums: `MarketsOrder::MarketCapDesc`; `PriceChangePercentage::{TwentyFourHours, SevenDays}`.
- Error type is `reqwest::Error`. Numbers are `f64`.
- **Field-name quirks** (compile errors if guessed wrong): `usd24_h_change`, `price_change_percentage24_h`, `price_change_percentage7_d_in_currency`, `sparkline_in7_d`.

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Crate metadata, dependencies, `Cargo.lock` committed (binary) |
| `src/main.rs` | tokio runtime, clap args, terminal setup/teardown, panic hook, async run loop |
| `src/error.rs` | `AppError` (thiserror) — domain failures the app branches on |
| `src/config.rs` | `Config` + nested types, `load`, `example`, `symbol`, `target_weight`, target normalization |
| `src/ledger.rs` | `load_ledger`, `save_ledger`, `assets` |
| `src/prices/mod.rs` | `Quote`, `PriceBook`, `PriceSource` trait, `price_map()` |
| `src/prices/mock.rs` | `MockSource` (deterministic, for tests) |
| `src/prices/cache.rs` | on-disk `PriceBook` cache with TTL + last-good fallback |
| `src/prices/coingecko.rs` | `CoinGeckoSource` (async, `coins_markets`) |
| `src/portfolio.rs` | `PortfolioModel` wrapper, `HoldingValue`, `holdings_with_value`, `estimate_sell_gain` |
| `src/perf.rs` | `Snapshot`, `PerfMetrics`, `record_snapshot`, `load_history`, `metrics` |
| `src/rebalance.rs` | `RebalanceAction`, `RebalanceSide`, `RebalanceSummary`, `Strategy`, `suggest` |
| `src/export.rs` | CSV/JSON export of capital-gains + holdings |
| `src/app.rs` | `App` state, `View`, `SortKey`, `Status`, recompute logic |
| `src/event.rs` | `Action` enum, `keymap`, crossterm→action mapping |
| `src/ui/mod.rs` | frame layout, tab bar, status bar, help overlay |
| `src/ui/{prices,holdings,valuation,tax,rebalance,perf}.rs` | one pure `render(frame, area, &App)` per view |
| `config.example.json`, `ledger.example.json` | shipped examples |
| `tests/integration.rs` | end-to-end App + MockSource exercise |

Build order follows spec §15. Each task is TDD: write failing test → run (fail) → implement → run (pass) → commit.

**Author guard reminder:** the repo's `git config user.email` is already `51518860+Technical-1@users.noreply.github.com` (used for the spec commit). Do **not** add any Co-Authored-By or "Generated with" trailer to commits. Verify `git config user.email` once before the first commit.

---

## Task 1: Scaffold — Cargo.toml, blank TUI, panic hook

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `config.example.json` (placeholder, finalized in Task 3)

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "crypto-price-tracker-v2"
version = "0.1.0"
edition = "2021"
description = "A terminal UI for tracking a crypto portfolio: prices, cost basis, tax, rebalancing, and performance."
license = "MIT OR Apache-2.0"

[dependencies]
coinbasis = { version = "0.1", features = ["serde"] }
coingecko = "1.1"
ratatui = "0.29"
crossterm = { version = "0.28", features = ["event-stream"] }
tokio = { version = "1.36", features = ["rt-multi-thread", "macros", "time", "sync"] }
rust_decimal = { version = "1", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
thiserror = "1"
clap = { version = "4", features = ["derive"] }
reqwest = "0.12"

[dev-dependencies]
rust_decimal_macros = "1"

[profile.release]
lto = true
```

- [ ] **Step 2: Write `src/main.rs` — blank TUI that opens, waits for `q`, closes cleanly**

```rust
mod error;

use std::io::{self, Stdout};
use std::panic;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original(info);
    }));
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    install_panic_hook();
    let mut terminal = setup_terminal()?;

    loop {
        terminal.draw(|f| {
            let widget = Paragraph::new("Crypto-Price-Tracker-V2 — press q to quit")
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(widget, f.area());
        })?;

        if let Event::Key(key) = event::read()? {
            if matches!(key.code, KeyCode::Char('q')) {
                break;
            }
        }
    }

    restore_terminal()?;
    Ok(())
}
```

- [ ] **Step 3: Create a temporary `src/error.rs` stub so it compiles**

```rust
//! Domain error type. Expanded in Task 2.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("placeholder")]
    Placeholder,
}
```

- [ ] **Step 4: Build and verify it compiles**

Run: `cargo build`
Expected: compiles with no errors (warnings about unused `AppError` are fine).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/error.rs
git commit -m "Scaffold binary crate, blank TUI, and panic hook"
```

---

## Task 2: `error.rs` — AppError

**Files:**
- Modify: `src/error.rs`
- Test: inline `#[cfg(test)]` in `src/error.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_displays_path() {
        let e = AppError::Config {
            path: "config.json".into(),
            reason: "missing field `ledger_path`".into(),
        };
        let msg = e.to_string();
        assert!(msg.contains("config.json"));
        assert!(msg.contains("ledger_path"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib error`
Expected: FAIL — `AppError::Config` variant does not exist.

- [ ] **Step 3: Replace `src/error.rs` with the real type**

```rust
//! Domain error type for failures the application branches on.
//! Top-level orchestration uses `anyhow::Result`; these are the typed kinds.

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("failed to read config `{path}`: {reason}")]
    Config { path: String, reason: String },

    #[error("failed to read ledger `{path}`: {reason}")]
    Ledger { path: String, reason: String },

    #[error("portfolio error: {0}")]
    Portfolio(#[from] coinbasis::PortfolioError),

    #[error("price source error: {0}")]
    Price(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("export error: {0}")]
    Export(String),
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib error`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "Add AppError domain error type"
```

---

## Task 3: `config.rs` — Config load, example, helpers, target normalization

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (add `mod config;`)
- Create: `config.example.json`
- Test: inline `#[cfg(test)]` in `src/config.rs`

- [ ] **Step 1: Write the failing tests**

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib config`
Expected: FAIL — `config` module / `rebalance::Strategy` not found. (`rebalance::Strategy` is defined in Task 11; for now this task DEFINES `Strategy` in `rebalance.rs` early — see Step 3 note.)

- [ ] **Step 3: Create `src/config.rs`**

> Note: `Strategy` lives in `rebalance.rs`. To keep this task self-contained, also create a minimal `src/rebalance.rs` now containing only the `Strategy` enum and `mod rebalance;`; Task 11 expands it. Add `mod rebalance;` and `mod config;` to `main.rs`.

`src/rebalance.rs` (minimal for now):
```rust
//! Rebalancing. `Strategy` defined here now; logic added in Task 11.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    Band,
    Full,
}
```

`src/config.rs`:
```rust
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
            tax: TaxConfig { short_term_rate: 0.24, long_term_rate: 0.15 },
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
```

> `Config::example` uses `rust_decimal_macros`; add `rust_decimal_macros = "1"` to `[dependencies]` (not just dev) since `example()` is non-test code.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib config`
Expected: PASS (all 6 tests).

- [ ] **Step 5: Generate `config.example.json` from `Config::example`**

Add to `main.rs` a hidden behavior is overkill; instead write the file by hand to match `example()`:
```json
{
  "ledger_path": "ledger.json",
  "default_method": "Fifo",
  "display_currency": "usd",
  "refresh_seconds": 60,
  "tax": { "short_term_rate": 0.24, "long_term_rate": 0.15 },
  "targets": { "bitcoin": "0.6", "ethereum": "0.3", "solana": "0.1" },
  "rebalance": { "band": "0.05", "min_trade_usd": "25", "strategy": "Band" },
  "symbols": { "bitcoin": "BTC", "ethereum": "ETH", "solana": "SOL" },
  "cache": { "ttl_seconds": 30, "dir": "~/.cache/crypto-price-tracker-v2" }
}
```
> Note: `rust_decimal` with serde accepts both JSON numbers and strings; example uses strings to preserve exactness.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/rebalance.rs src/main.rs Cargo.toml Cargo.lock config.example.json
git commit -m "Add Config loading, helpers, target normalization, and Strategy enum"
```

---

## Task 4: `ledger.rs` — load/save/assets

**Files:**
- Create: `src/ledger.rs`, `ledger.example.json`
- Modify: `src/main.rs` (`mod ledger;`)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        r#"[
          { "Buy": { "timestamp": "2021-01-01T00:00:00Z", "wallet": "coinbase",
                     "asset": "bitcoin", "quantity": "0.5", "unit_price": "30000", "fee": "5" } },
          { "Income": { "timestamp": "2021-06-01T00:00:00Z", "wallet": "kraken",
                        "asset": "ethereum", "quantity": "1.2", "value": "2400", "source": "Staking" } }
        ]"#
    }

    #[test]
    fn loads_transactions() {
        let dir = std::env::temp_dir().join("cpt2_ledger_load");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.json");
        std::fs::write(&path, sample()).unwrap();
        let txs = load_ledger(path.to_str().unwrap()).unwrap();
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn derives_distinct_assets_sorted() {
        let txs: Vec<coinbasis::Transaction> = serde_json::from_str(sample()).unwrap();
        let assets = assets(&txs);
        let v: Vec<_> = assets.into_iter().collect();
        assert_eq!(v, vec!["bitcoin".to_string(), "ethereum".to_string()]);
    }

    #[test]
    fn malformed_ledger_surfaces_clear_error() {
        let dir = std::env::temp_dir().join("cpt2_ledger_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{ not an array }").unwrap();
        let err = load_ledger(path.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("bad.json"));
    }

    #[test]
    fn save_then_load_roundtrips() {
        let txs: Vec<coinbasis::Transaction> = serde_json::from_str(sample()).unwrap();
        let dir = std::env::temp_dir().join("cpt2_ledger_rt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.json");
        save_ledger(path.to_str().unwrap(), &txs).unwrap();
        let back = load_ledger(path.to_str().unwrap()).unwrap();
        assert_eq!(back, txs);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ledger`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/ledger.rs`**

```rust
//! Ledger persistence: a JSON array of `coinbasis::Transaction`.

use std::collections::BTreeSet;

use coinbasis::Transaction;

use crate::error::AppError;

pub fn load_ledger(path: &str) -> Result<Vec<Transaction>, AppError> {
    let text = std::fs::read_to_string(path).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })?;
    serde_json::from_str(&text).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })
}

pub fn save_ledger(path: &str, txs: &[Transaction]) -> Result<(), AppError> {
    let text = serde_json::to_string_pretty(txs).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(path, text).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })
}

/// Distinct asset ids referenced by any transaction, sorted.
pub fn assets(txs: &[Transaction]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for tx in txs {
        match tx {
            Transaction::Buy { asset, .. }
            | Transaction::Sell { asset, .. }
            | Transaction::Income { asset, .. }
            | Transaction::Spend { asset, .. }
            | Transaction::Transfer { asset, .. }
            | Transaction::GiftSent { asset, .. }
            | Transaction::GiftReceived { asset, .. } => {
                set.insert(asset.clone());
            }
            Transaction::Trade { from_asset, to_asset, .. } => {
                set.insert(from_asset.clone());
                set.insert(to_asset.clone());
            }
        }
    }
    set
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib ledger`
Expected: PASS (4 tests).

- [ ] **Step 5: Create `ledger.example.json`** (same content as the test `sample()` array).

- [ ] **Step 6: Commit**

```bash
git add src/ledger.rs src/main.rs ledger.example.json
git commit -m "Add ledger load/save and asset derivation"
```

---

## Task 5: `prices/mod.rs` — Quote, PriceBook, PriceSource trait

**Files:**
- Create: `src/prices/mod.rs`
- Modify: `src/main.rs` (`mod prices;`)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
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
            Quote { price: dec!(50000), change_24h: dec!(1.5), change_7d: Some(dec!(3.0)),
                    market_cap: None, volume_24h: None, ath: None },
        );
        let book = PriceBook { quotes, fetched_at: Utc::now(), sparklines: HashMap::new(), stale: false };
        let pm = book.price_map();
        assert_eq!(pm.get("bitcoin"), Some(&dec!(50000)));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib prices`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/prices/mod.rs`**

```rust
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
        self.quotes.iter().map(|(k, q)| (k.clone(), q.price)).collect()
    }
}

#[allow(async_fn_in_trait)]
pub trait PriceSource {
    /// Fetch quotes for `ids` priced in `vs` (e.g. "usd").
    async fn fetch(&self, ids: &[String], vs: &str) -> Result<PriceBook, AppError>;
}
```

> Note: `async fn in trait` is stable on Rust 1.75+. `#[allow(async_fn_in_trait)]` silences the dyn-safety lint; we only ever use `PriceSource` generically or as concrete types, never as `dyn PriceSource`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib prices`
Expected: FAIL to **compile** — `cache`, `coingecko`, `mock` submodules don't exist yet. Create empty stubs so the tree compiles:

`src/prices/cache.rs`: `//! Disk cache. Implemented in Task 7.`
`src/prices/mock.rs`: `//! Mock source. Implemented in Task 6.`
`src/prices/coingecko.rs`: `//! CoinGecko source. Implemented in Task 8.`

Re-run: `cargo test --lib prices` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/prices/ src/main.rs
git commit -m "Add price abstraction: Quote, PriceBook, PriceSource trait"
```

---

## Task 6: `prices/mock.rs` — MockSource

**Files:**
- Modify: `src/prices/mock.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib prices::mock`
Expected: FAIL — `MockSource` not defined.

- [ ] **Step 3: Implement `src/prices/mock.rs`**

```rust
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
            Quote { price, change_24h, change_7d: None, market_cap: None, volume_24h: None, ath: None },
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib prices::mock`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/prices/mock.rs
git commit -m "Add MockSource price source for tests"
```

---

## Task 7: `prices/cache.rs` — disk cache with TTL + last-good

**Files:**
- Modify: `src/prices/cache.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib prices::cache`
Expected: FAIL — `PriceCache` not defined.

- [ ] **Step 3: Implement `src/prices/cache.rs`**

```rust
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
        if age <= self.ttl_seconds {
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib prices::cache`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/prices/cache.rs
git commit -m "Add on-disk price cache with TTL and last-good fallback"
```

---

## Task 8: `prices/coingecko.rs` — CoinGeckoSource

**Files:**
- Modify: `src/prices/coingecko.rs`

> No unit test here (it performs network I/O). Verified by `cargo build` and exercised manually in Task 22. The `f64 → Decimal` conversion helper IS unit-tested.

- [ ] **Step 1: Write the failing test for the conversion helper**

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib prices::coingecko`
Expected: FAIL — `to_decimal` not defined.

- [ ] **Step 3: Implement `src/prices/coingecko.rs`**

```rust
//! CoinGecko-backed `PriceSource` using the one-shot `coins_markets` endpoint,
//! which returns price, 24h/7d change, market cap, volume, and a 7d sparkline
//! in a single call.

use std::collections::HashMap;

use chrono::Utc;
use coingecko::params::{MarketsOrder, PriceChangePercentage};
use coingecko::CoinGeckoClient;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

use super::{PriceBook, PriceSource, Quote};
use crate::error::AppError;

pub struct CoinGeckoSource {
    client: CoinGeckoClient,
}

impl CoinGeckoSource {
    /// No API key (public/demo endpoint). For a demo key, swap in
    /// `CoinGeckoClient::new_with_demo_api_key(&key)`.
    pub fn new() -> Self {
        Self { client: CoinGeckoClient::new(coingecko::COINGECKO_API_DEMO_URL) }
    }
}

impl Default for CoinGeckoSource {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn to_decimal(v: Option<f64>) -> Decimal {
    v.and_then(Decimal::from_f64_retain).unwrap_or(Decimal::ZERO)
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
                &[PriceChangePercentage::TwentyFourHours, PriceChangePercentage::SevenDays],
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

        Ok(PriceBook { quotes, fetched_at: Utc::now(), sparklines, stale: false })
    }
}
```

- [ ] **Step 4: Run to verify pass + build**

Run: `cargo test --lib prices::coingecko` then `cargo build`
Expected: tests PASS; build succeeds.

> If `coins_markets`'s exact signature differs from what's pinned above (e.g. `category` arg type), adjust to the docs.rs signature for the installed version — `PriceSource` isolates this so only this file changes.

- [ ] **Step 5: Commit**

```bash
git add src/prices/coingecko.rs Cargo.lock
git commit -m "Add CoinGeckoSource using coins_markets endpoint"
```

---

## Task 9: `portfolio.rs` — PortfolioModel wrapper

**Files:**
- Modify: `src/portfolio.rs` (currently nonexistent — create), `src/main.rs` (`mod portfolio;`)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use coinbasis::{CostBasisMethod, Transaction};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn ledger() -> Vec<Transaction> {
        vec![
            Transaction::Buy {
                timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(), asset: "bitcoin".into(),
                quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
            },
            Transaction::Buy {
                timestamp: Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(), asset: "bitcoin".into(),
                quantity: dec!(1), unit_price: dec!(40000), fee: dec!(0),
            },
            Transaction::Sell {
                timestamp: Utc.with_ymd_and_hms(2023, 6, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(), asset: "bitcoin".into(),
                quantity: dec!(0.5), unit_price: dec!(50000), fee: dec!(0),
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
        let hv = m.holdings_with_value(CostBasisMethod::Fifo, &prices).unwrap();
        // 1.5 BTC remaining
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
        // sell $10000 of bitcoin at $50000 => 0.2 BTC; under Hifo draws the $40k lot
        let gain = estimate_sell_gain(
            &txs, "bitcoin", dec!(10000), dec!(50000),
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        ).unwrap();
        assert!(gain > dec!(0));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib portfolio`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/portfolio.rs`**

```rust
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
        Ok(Self { portfolio, txs: txs.to_vec() })
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
                HoldingValue { holding: h, price, current_value, unrealized }
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
    // wallet is required by the Sell variant; HIFO ignores wallet for lot choice
    // within an asset across wallets in this estimate, so pick a placeholder that
    // matches an existing wallet for that asset if possible.
    let wallet = txs
        .iter()
        .find_map(|t| match t {
            Transaction::Buy { asset: a, wallet, .. } if a == asset => Some(wallet.clone()),
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib portfolio`
Expected: PASS (4 tests). If `estimate_sell_gain` errors with `InsufficientLots` for the test (selling more than held), reduce the test's sell value; 0.2 BTC against 1.5 BTC held is safe.

- [ ] **Step 5: Commit**

```bash
git add src/portfolio.rs src/main.rs
git commit -m "Add PortfolioModel wrapper, holding valuation, and sell-gain estimator"
```

---

## Task 10: `perf.rs` — snapshots and metrics

**Files:**
- Create: `src/perf.rs`, `src/main.rs` (`mod perf;`)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
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
        // 30s later, within 60s interval => skipped
        let t1 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 30).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(101), t1, 60).unwrap();
        // 90s later => appended
        let t2 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 1, 30).unwrap();
        record_snapshot(path.to_str().unwrap(), dec!(102), t2, 60).unwrap();
        let hist = load_history(path.to_str().unwrap()).unwrap();
        assert_eq!(hist.len(), 2);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib perf`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/perf.rs`**

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib perf`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/perf.rs src/main.rs
git commit -m "Add value-history snapshots and performance metrics"
```

---

## Task 11: `rebalance.rs` — drift, actions, tax-aware estimate

**Files:**
- Modify: `src/rebalance.rs` (expand the Task 3 stub)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use coinbasis::{AssetValuation, PortfolioReport};
    use rust_decimal_macros::dec;
    use std::collections::BTreeMap;

    fn report() -> PortfolioReport {
        // total 10000: btc 7000 (70%), eth 3000 (30%)
        PortfolioReport {
            assets: vec![
                AssetValuation { asset: "bitcoin".into(), quantity: dec!(1), cost_basis: dec!(5000),
                    price: dec!(7000), market_value: dec!(7000), unrealized: dec!(2000), allocation: dec!(0.7) },
                AssetValuation { asset: "ethereum".into(), quantity: dec!(1), cost_basis: dec!(2000),
                    price: dec!(3000), market_value: dec!(3000), unrealized: dec!(1000), allocation: dec!(0.3) },
            ],
            total_cost: dec!(7000), total_value: dec!(10000),
            total_unrealized: dec!(3000), total_return: dec!(0.428),
            missing_prices: vec![],
        }
    }

    fn targets() -> BTreeMap<String, rust_decimal::Decimal> {
        let mut t = BTreeMap::new();
        t.insert("bitcoin".into(), dec!(0.6));
        t.insert("ethereum".into(), dec!(0.4));
        t
    }

    #[test]
    fn band_strategy_suggests_sell_btc_buy_eth() {
        // targets: btc 6000, eth 4000. drift btc +1000 (10% > band 5%), eth -1000
        let actions = suggest(&report(), &targets(), dec!(0.05), dec!(25), Strategy::Band);
        let btc = actions.iter().find(|a| a.asset == "bitcoin").unwrap();
        assert_eq!(btc.side, RebalanceSide::Sell);
        assert_eq!(btc.amount_usd, dec!(1000));
        let eth = actions.iter().find(|a| a.asset == "ethereum").unwrap();
        assert_eq!(eth.side, RebalanceSide::Buy);
    }

    #[test]
    fn band_strategy_skips_within_band() {
        // tighten targets to match current: no drift beyond band
        let mut t = BTreeMap::new();
        t.insert("bitcoin".into(), dec!(0.7));
        t.insert("ethereum".into(), dec!(0.3));
        let actions = suggest(&report(), &t, dec!(0.05), dec!(25), Strategy::Band);
        assert!(actions.is_empty());
    }

    #[test]
    fn min_trade_filters_tiny_drifts() {
        let actions = suggest(&report(), &targets(), dec!(0.0), dec!(2000), Strategy::Full);
        // drift is 1000 each, below 2000 min => filtered
        assert!(actions.is_empty());
    }

    #[test]
    fn summary_reports_in_balance_when_no_actions() {
        let mut t = BTreeMap::new();
        t.insert("bitcoin".into(), dec!(0.7));
        t.insert("ethereum".into(), dec!(0.3));
        let actions = suggest(&report(), &t, dec!(0.05), dec!(25), Strategy::Band);
        let summary = summarize(&actions);
        assert!(summary.in_balance);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib rebalance`
Expected: FAIL — `suggest`, `RebalanceAction`, etc. not defined.

- [ ] **Step 3: Expand `src/rebalance.rs`**

```rust
//! Rebalancing: compute per-asset drift vs. target weights and suggest trades.
//! Pure and deterministic. Tax-aware sell-gain estimates are filled in by the
//! caller via `crate::portfolio::estimate_sell_gain`.

use std::collections::BTreeMap;

use coinbasis::PortfolioReport;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    Band,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebalanceSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceAction {
    pub asset: String,
    pub current_value: Decimal,
    pub target_value: Decimal,
    pub drift: Decimal,
    pub side: RebalanceSide,
    pub amount_usd: Decimal,
    /// Estimated realized gain if this sell executes (HIFO). Filled by caller.
    pub est_realized_gain: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceSummary {
    pub total_buys: Decimal,
    pub total_sells: Decimal,
    pub in_balance: bool,
}

/// Suggest rebalancing trades. `targets` are weights (need not be normalized;
/// they are normalized internally).
pub fn suggest(
    report: &PortfolioReport,
    targets: &BTreeMap<String, Decimal>,
    band: Decimal,
    min_trade_usd: Decimal,
    strategy: Strategy,
) -> Vec<RebalanceAction> {
    let total = report.total_value;
    if total.is_zero() {
        return Vec::new();
    }
    let weight_sum: Decimal = targets.values().copied().sum();
    if weight_sum.is_zero() {
        return Vec::new();
    }

    // current market value per asset (from the report)
    let mut current: BTreeMap<String, Decimal> = BTreeMap::new();
    for av in &report.assets {
        current.insert(av.asset.clone(), av.market_value);
    }

    // union of held assets and targeted assets
    let mut keys: Vec<String> = current.keys().cloned().collect();
    for k in targets.keys() {
        if !current.contains_key(k) {
            keys.push(k.clone());
        }
    }
    keys.sort();
    keys.dedup();

    let mut actions = Vec::new();
    for asset in keys {
        let current_value = current.get(&asset).copied().unwrap_or(Decimal::ZERO);
        let weight = targets.get(&asset).copied().unwrap_or(Decimal::ZERO) / weight_sum;
        let target_value = total * weight;
        let drift = current_value - target_value; // + => overweight (sell)
        let drift_fraction = (drift / total).abs();

        match strategy {
            Strategy::Band if drift_fraction <= band => continue,
            _ => {}
        }

        let amount = drift.abs();
        if amount < min_trade_usd {
            continue;
        }

        let (side, amount_usd) = if drift > Decimal::ZERO {
            // overweight: sell, capped at held value
            (RebalanceSide::Sell, amount.min(current_value))
        } else {
            (RebalanceSide::Buy, amount)
        };
        if amount_usd < min_trade_usd {
            continue;
        }

        actions.push(RebalanceAction {
            asset,
            current_value,
            target_value,
            drift,
            side,
            amount_usd,
            est_realized_gain: None,
        });
    }
    actions
}

pub fn summarize(actions: &[RebalanceAction]) -> RebalanceSummary {
    let total_buys = actions
        .iter()
        .filter(|a| a.side == RebalanceSide::Buy)
        .map(|a| a.amount_usd)
        .sum();
    let total_sells = actions
        .iter()
        .filter(|a| a.side == RebalanceSide::Sell)
        .map(|a| a.amount_usd)
        .sum();
    RebalanceSummary { total_buys, total_sells, in_balance: actions.is_empty() }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib rebalance`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/rebalance.rs
git commit -m "Add rebalancing drift logic, action suggestions, and summary"
```

---

## Task 12: `export.rs` — CSV/JSON export

**Files:**
- Create: `src/export.rs`, `src/main.rs` (`mod export;`)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use coinbasis::{CapitalGainsReport, Holding, RealizedGain, Term};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn cg() -> CapitalGainsReport {
        CapitalGainsReport {
            tax_year: 2023,
            rows: vec![RealizedGain {
                asset: "bitcoin".into(), wallet: "coinbase".into(),
                disposed_at: Utc.with_ymd_and_hms(2023, 6, 1, 0, 0, 0).unwrap(),
                acquired_at: Some(Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap()),
                quantity: dec!(0.5), proceeds: dec!(25000), cost_basis: dec!(15000),
                gain: dec!(10000), term: Some(Term::Long),
            }],
            short_term_gain: dec!(0), long_term_gain: dec!(10000), total_gain: dec!(10000),
        }
    }

    #[test]
    fn capital_gains_csv_has_header_and_row() {
        let dir = std::env::temp_dir().join("cpt2_export");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cg.csv");
        export_capital_gains_csv(&cg(), path.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("asset,wallet,acquired,disposed,quantity,proceeds,cost_basis,gain,term"));
        assert!(text.contains("bitcoin"));
        assert!(text.contains("Long"));
    }

    #[test]
    fn capital_gains_json_roundtrips() {
        let dir = std::env::temp_dir().join("cpt2_export");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cg.json");
        export_capital_gains_json(&cg(), path.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let back: CapitalGainsReport = serde_json::from_str(&text).unwrap();
        assert_eq!(back.total_gain, dec!(10000));
    }

    #[test]
    fn holdings_csv_has_header() {
        let dir = std::env::temp_dir().join("cpt2_export");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("h.csv");
        let holdings = vec![Holding {
            asset: "bitcoin".into(), wallet: "coinbase".into(),
            quantity: dec!(1.5), cost_basis: dec!(55000), average_cost: dec!(36666.67),
        }];
        export_holdings_csv(&holdings, path.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("asset,wallet,quantity,cost_basis,average_cost"));
        assert!(text.contains("bitcoin"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib export`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/export.rs`**

```rust
//! CSV/JSON export of tax and holdings reports. Hand-rolled CSV (no crate)
//! since the rows are simple and fully numeric/string.

use coinbasis::{CapitalGainsReport, Holding, Term};

use crate::error::AppError;

fn write(path: &str, text: &str) -> Result<(), AppError> {
    std::fs::write(path, text).map_err(|e| AppError::Export(e.to_string()))
}

pub fn export_capital_gains_json(rep: &CapitalGainsReport, path: &str) -> Result<(), AppError> {
    let text = serde_json::to_string_pretty(rep).map_err(|e| AppError::Export(e.to_string()))?;
    write(path, &text)
}

pub fn export_capital_gains_csv(rep: &CapitalGainsReport, path: &str) -> Result<(), AppError> {
    let mut out = String::from(
        "asset,wallet,acquired,disposed,quantity,proceeds,cost_basis,gain,term\n",
    );
    for r in &rep.rows {
        let acquired = r.acquired_at.map(|d| d.to_rfc3339()).unwrap_or_default();
        let term = match r.term {
            Some(Term::Short) => "Short",
            Some(Term::Long) => "Long",
            None => "",
        };
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            r.asset, r.wallet, acquired, r.disposed_at.to_rfc3339(),
            r.quantity, r.proceeds, r.cost_basis, r.gain, term,
        ));
    }
    write(path, &out)
}

pub fn export_holdings_json(holdings: &[Holding], path: &str) -> Result<(), AppError> {
    let text = serde_json::to_string_pretty(holdings).map_err(|e| AppError::Export(e.to_string()))?;
    write(path, &text)
}

pub fn export_holdings_csv(holdings: &[Holding], path: &str) -> Result<(), AppError> {
    let mut out = String::from("asset,wallet,quantity,cost_basis,average_cost\n");
    for h in holdings {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            h.asset, h.wallet, h.quantity, h.cost_basis, h.average_cost,
        ));
    }
    write(path, &out)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib export`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/export.rs src/main.rs
git commit -m "Add CSV/JSON export for capital gains and holdings"
```

---

## Task 13: `app.rs` — App state, View, SortKey, recompute

**Files:**
- Create: `src/app.rs`, `src/main.rs` (`mod app;`)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use coinbasis::{CostBasisMethod, Transaction};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        App::new(Config::example(), &txs).unwrap()
    }

    #[test]
    fn starts_on_prices_view_with_default_method() {
        let a = app();
        assert_eq!(a.view, View::Prices);
        assert_eq!(a.method, CostBasisMethod::Fifo);
    }

    #[test]
    fn cycle_method_walks_the_four_automatic_methods() {
        let mut a = app();
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Lifo);
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Hifo);
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Average);
        a.cycle_method();
        assert_eq!(a.method, CostBasisMethod::Fifo); // wraps, never SpecificId
    }

    #[test]
    fn next_and_prev_view_cycle_six_views() {
        let mut a = app();
        a.next_view();
        assert_eq!(a.view, View::Holdings);
        a.prev_view();
        assert_eq!(a.view, View::Prices);
        a.prev_view();
        assert_eq!(a.view, View::Performance); // wraps backward
    }

    #[test]
    fn set_year_adjusts_tax_year() {
        let mut a = app();
        let y = a.tax_year;
        a.set_year(1);
        assert_eq!(a.tax_year, y + 1);
        a.set_year(-1);
        assert_eq!(a.tax_year, y);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/app.rs`**

```rust
//! Central application state and the recompute logic that keeps derived
//! reports in sync with the active method, tax year, and prices.

use std::collections::HashMap;

use coinbasis::{CapitalGainsReport, CostBasisMethod, IncomeReport, PortfolioReport};
use rust_decimal::Decimal;

use crate::config::Config;
use crate::error::AppError;
use crate::portfolio::{HoldingValue, PortfolioModel};
use crate::prices::PriceBook;
use crate::rebalance::{self, RebalanceAction, RebalanceSummary, Strategy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Prices,
    Holdings,
    Valuation,
    Tax,
    Rebalance,
    Performance,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Prices, View::Holdings, View::Valuation,
        View::Tax, View::Rebalance, View::Performance,
    ];

    pub fn title(self) -> &'static str {
        match self {
            View::Prices => "Prices",
            View::Holdings => "Holdings",
            View::Valuation => "Valuation",
            View::Tax => "Tax",
            View::Rebalance => "Rebalance",
            View::Performance => "Performance",
        }
    }

    fn index(self) -> usize {
        View::ALL.iter().position(|&v| v == self).unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Symbol,
    Price,
    Change24h,
    Value,
    Profit,
}

impl SortKey {
    pub const ALL: [SortKey; 5] =
        [SortKey::Symbol, SortKey::Price, SortKey::Change24h, SortKey::Value, SortKey::Profit];
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub message: String,
}

/// Reports recomputed whenever method / year / prices change.
#[derive(Default)]
pub struct Derived {
    pub valuation: Option<PortfolioReport>,
    pub holdings: Vec<HoldingValue>,
    pub capital_gains: Option<CapitalGainsReport>,
    pub income: Option<IncomeReport>,
    pub rebalance_actions: Vec<RebalanceAction>,
    pub rebalance_summary: Option<RebalanceSummary>,
}

pub struct App {
    pub config: Config,
    pub model: PortfolioModel,
    pub method: CostBasisMethod,
    pub view: View,
    pub tax_year: i32,
    pub prices: Option<PriceBook>,
    pub derived: Derived,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub selected: usize,
    pub group_by_wallet: bool,
    pub strategy: Strategy,
    pub status: Status,
    pub loading: bool,
    pub show_help: bool,
    pub should_quit: bool,
}

impl App {
    pub fn new(config: Config, txs: &[coinbasis::Transaction]) -> Result<App, AppError> {
        let model = PortfolioModel::new(txs)?;
        let method = config.default_method;
        let strategy = config.rebalance.strategy;
        let mut app = App {
            method,
            view: View::Prices,
            tax_year: 2024,
            prices: None,
            derived: Derived::default(),
            sort: SortKey::Value,
            sort_desc: true,
            selected: 0,
            group_by_wallet: false,
            strategy,
            status: Status::default(),
            loading: false,
            show_help: false,
            should_quit: false,
            config,
            model,
        };
        // If `SpecificId` was configured, fall back to Fifo for the switcher.
        if matches!(app.method, CostBasisMethod::SpecificId) {
            app.method = CostBasisMethod::Fifo;
        }
        app.recompute();
        Ok(app)
    }

    fn price_map(&self) -> HashMap<String, Decimal> {
        self.prices.as_ref().map(|b| b.price_map()).unwrap_or_default()
    }

    /// Recompute all derived reports from current method / year / prices.
    pub fn recompute(&mut self) {
        let pm = self.price_map();

        self.derived.holdings =
            self.model.holdings_with_value(self.method, &pm).unwrap_or_default();
        self.derived.valuation = self.model.valuation(self.method, &pm).ok();
        self.derived.capital_gains = self.model.capital_gains(self.method, self.tax_year).ok();
        self.derived.income = Some(self.model.income(self.tax_year));

        if let Some(report) = &self.derived.valuation {
            let mut actions = rebalance::suggest(
                report,
                &self.config.targets,
                self.config.rebalance.band,
                self.config.rebalance.min_trade_usd,
                self.strategy,
            );
            // Tax-aware: fill in estimated realized gains for sells.
            let now = report_timestamp(self);
            for a in actions.iter_mut() {
                if a.side == crate::rebalance::RebalanceSide::Sell {
                    let price = report
                        .assets
                        .iter()
                        .find(|av| av.asset == a.asset)
                        .map(|av| av.price)
                        .unwrap_or(Decimal::ZERO);
                    a.est_realized_gain = crate::portfolio::estimate_sell_gain(
                        self.model.transactions(), &a.asset, a.amount_usd, price, now,
                    ).ok();
                }
            }
            self.derived.rebalance_summary = Some(rebalance::summarize(&actions));
            self.derived.rebalance_actions = actions;
        } else {
            self.derived.rebalance_actions.clear();
            self.derived.rebalance_summary = None;
        }
    }

    pub fn next_view(&mut self) {
        let i = (self.view.index() + 1) % View::ALL.len();
        self.view = View::ALL[i];
        self.selected = 0;
    }

    pub fn prev_view(&mut self) {
        let i = (self.view.index() + View::ALL.len() - 1) % View::ALL.len();
        self.view = View::ALL[i];
        self.selected = 0;
    }

    /// Cycle the four automatic methods (never `SpecificId`).
    pub fn cycle_method(&mut self) {
        self.method = match self.method {
            CostBasisMethod::Fifo => CostBasisMethod::Lifo,
            CostBasisMethod::Lifo => CostBasisMethod::Hifo,
            CostBasisMethod::Hifo => CostBasisMethod::Average,
            CostBasisMethod::Average | CostBasisMethod::SpecificId => CostBasisMethod::Fifo,
        };
        self.recompute();
    }

    pub fn set_year(&mut self, delta: i32) {
        self.tax_year += delta;
        self.recompute();
    }

    pub fn cycle_sort(&mut self) {
        let idx = SortKey::ALL.iter().position(|&s| s == self.sort).unwrap();
        self.sort = SortKey::ALL[(idx + 1) % SortKey::ALL.len()];
    }

    pub fn toggle_grouping(&mut self) {
        self.group_by_wallet = !self.group_by_wallet;
    }

    pub fn toggle_strategy(&mut self) {
        self.strategy = match self.strategy {
            Strategy::Band => Strategy::Full,
            Strategy::Full => Strategy::Band,
        };
        self.recompute();
    }

    pub fn select_next(&mut self) {
        self.selected = self.selected.saturating_add(1);
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn set_prices(&mut self, book: PriceBook) {
        self.prices = Some(book);
        self.loading = false;
        self.recompute();
    }

    pub fn method_label(&self) -> &'static str {
        match self.method {
            CostBasisMethod::Fifo => "FIFO",
            CostBasisMethod::Lifo => "LIFO",
            CostBasisMethod::Hifo => "HIFO",
            CostBasisMethod::Average => "AVG",
            CostBasisMethod::SpecificId => "SPEC",
        }
    }
}

fn report_timestamp(app: &App) -> chrono::DateTime<chrono::Utc> {
    app.prices.as_ref().map(|b| b.fetched_at).unwrap_or_else(chrono::Utc::now)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "Add App state, view/method navigation, and centralized recompute"
```

---

## Task 14: `event.rs` — Action enum and keymap

**Files:**
- Create: `src/event.rs`, `src/main.rs` (`mod event;`)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn maps_quit_keys() {
        assert_eq!(map_key(key('q')), Some(Action::Quit));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn maps_navigation_and_method() {
        assert_eq!(map_key(key('m')), Some(Action::CycleMethod));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::NextView)
        );
        assert_eq!(map_key(key('[')), Some(Action::PrevYear));
        assert_eq!(map_key(key(']')), Some(Action::NextYear));
    }

    #[test]
    fn unmapped_key_returns_none() {
        assert_eq!(map_key(key('z')), None);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib event`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/event.rs`**

```rust
//! Input mapping: crossterm key events -> high-level `Action`s.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    NextView,
    PrevView,
    CycleMethod,
    CycleSort,
    ToggleGrouping,
    NextYear,
    PrevYear,
    ToggleStrategy,
    Refresh,
    Export,
    SelectNext,
    SelectPrev,
    ToggleHelp,
}

pub fn map_key(key: KeyEvent) -> Option<Action> {
    Some(match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Tab | KeyCode::Right => Action::NextView,
        KeyCode::BackTab | KeyCode::Left => Action::PrevView,
        KeyCode::Char('m') => Action::CycleMethod,
        KeyCode::Char('s') => Action::CycleSort,
        KeyCode::Char('g') => Action::ToggleGrouping,
        KeyCode::Char(']') => Action::NextYear,
        KeyCode::Char('[') => Action::PrevYear,
        KeyCode::Char('t') => Action::ToggleStrategy,
        KeyCode::Char('r') => Action::Refresh,
        KeyCode::Char('e') => Action::Export,
        KeyCode::Down => Action::SelectNext,
        KeyCode::Up => Action::SelectPrev,
        KeyCode::Char('?') => Action::ToggleHelp,
        _ => return None,
    })
}

/// Apply an action to app state. Returns `true` if a price refresh was requested.
pub fn apply(app: &mut App, action: Action) -> bool {
    match action {
        Action::Quit => app.should_quit = true,
        Action::NextView => app.next_view(),
        Action::PrevView => app.prev_view(),
        Action::CycleMethod => app.cycle_method(),
        Action::CycleSort => app.cycle_sort(),
        Action::ToggleGrouping => app.toggle_grouping(),
        Action::NextYear => app.set_year(1),
        Action::PrevYear => app.set_year(-1),
        Action::ToggleStrategy => app.toggle_strategy(),
        Action::Refresh => {
            app.loading = true;
            return true;
        }
        Action::Export => app.status.message = "export requested".into(),
        Action::SelectNext => app.select_next(),
        Action::SelectPrev => app.select_prev(),
        Action::ToggleHelp => app.toggle_help(),
    }
    false
}
```

> Note: `Action::Export` handling is finished in Task 19/Task 22 where the active view determines what to export; here it just records intent so the keymap test passes and `apply` is total.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib event`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/event.rs src/main.rs
git commit -m "Add Action enum, keymap, and action application"
```

---

## Task 15: `ui/mod.rs` — frame layout, tab bar, status bar, help overlay

**Files:**
- Create: `src/ui/mod.rs`, `src/main.rs` (`mod ui;`)
- Test: inline `#[cfg(test)]` using `ratatui::TestBackend`

> The six per-view modules are declared here but created in Tasks 16–21. To compile now, create empty stub files `src/ui/{prices,holdings,valuation,tax,rebalance,perf}.rs` each containing a no-op `render`:
> ```rust
> use ratatui::layout::Rect;
> use ratatui::Frame;
> use crate::app::App;
> pub fn render(_f: &mut Frame, _area: Rect, _app: &App) {}
> ```

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        App::new(Config::example(), &txs).unwrap()
    }

    fn render_to_string(app: &App) -> String {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn tab_bar_shows_all_view_titles_and_method() {
        let s = render_to_string(&app());
        assert!(s.contains("Prices"));
        assert!(s.contains("Holdings"));
        assert!(s.contains("Performance"));
        assert!(s.contains("FIFO"));
    }

    #[test]
    fn help_overlay_lists_keybindings_when_shown() {
        let mut a = app();
        a.show_help = true;
        let s = render_to_string(&a);
        assert!(s.contains("cycle method"));
        assert!(s.contains("quit"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui`
Expected: FAIL — `draw` not defined.

- [ ] **Step 3: Create `src/ui/mod.rs`**

```rust
//! Frame composition: tab bar, active view, status bar, and help overlay.

pub mod holdings;
pub mod perf;
pub mod prices;
pub mod rebalance;
pub mod tax;
pub mod valuation;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs};
use ratatui::Frame;

use crate::app::{App, View};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    draw_tab_bar(f, chunks[0], app);
    draw_active_view(f, chunks[1], app);
    draw_status_bar(f, chunks[2], app);

    if app.show_help {
        draw_help(f, f.area());
    }
}

fn draw_tab_bar(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = View::ALL.iter().map(|v| Line::from(v.title())).collect();
    let selected = View::ALL.iter().position(|&v| v == app.view).unwrap();
    let countdown = app
        .prices
        .as_ref()
        .map(|_| format!("{}s", app.config.refresh_seconds))
        .unwrap_or_else(|| "—".into());
    let title = format!(" {} · refresh {} ", app.method_label(), countdown);
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(title))
        .select(selected)
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

fn draw_active_view(f: &mut Frame, area: Rect, app: &App) {
    match app.view {
        View::Prices => prices::render(f, area, app),
        View::Holdings => holdings::render(f, area, app),
        View::Valuation => valuation::render(f, area, app),
        View::Tax => tax::render(f, area, app),
        View::Rebalance => rebalance::render(f, area, app),
        View::Performance => perf::render(f, area, app),
    }
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![Span::styled(
        format!(" {} ", app.method_label()),
        Style::default().fg(Color::Black).bg(Color::Cyan),
    )];
    if app.loading {
        spans.push(Span::raw(" ⟳ loading "));
    }
    if let Some(b) = &app.prices {
        if b.stale {
            spans.push(Span::styled(" [stale] ", Style::default().fg(Color::Red)));
        }
    }
    if !app.status.message.is_empty() {
        spans.push(Span::raw(format!(" {} ", app.status.message)));
    }
    spans.push(Span::styled(" ? help ", Style::default().fg(Color::DarkGray)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let w = area.width.min(60);
    let h = area.height.min(16);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    let lines = vec![
        Line::from("Keybindings"),
        Line::from("Tab/→  next view    Shift-Tab/←  prev view"),
        Line::from("m      cycle method  s  cycle sort"),
        Line::from("g      toggle grouping"),
        Line::from("[ / ]  change tax year"),
        Line::from("t      toggle rebalance strategy"),
        Line::from("r      refresh now    e  export (Tax/Holdings)"),
        Line::from("↑/↓    move selection"),
        Line::from("?      toggle help     q/Esc  quit"),
    ];
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Help ")),
        popup,
    );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib ui` then `cargo build`
Expected: PASS (2 tests); build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/ui/ src/main.rs
git commit -m "Add UI shell: tab bar, status bar, and help overlay"
```

---

## Task 16: `ui/prices.rs` — Prices / P&L view

**Files:**
- Modify: `src/ui/prices.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::prices::{PriceBook, Quote};
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn app_with_prices() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut app = App::new(Config::example(), &txs).unwrap();
        let mut quotes = HashMap::new();
        quotes.insert("bitcoin".into(), Quote {
            price: dec!(50000), change_24h: dec!(2.5), change_7d: Some(dec!(5.0)),
            market_cap: Some(dec!(1000000000000)), volume_24h: Some(dec!(20000000000)),
            ath: Some(dec!(69000)),
        });
        app.set_prices(PriceBook {
            quotes, fetched_at: Utc::now(), sparklines: HashMap::new(), stale: false });
        app
    }

    fn rendered(app: &App) -> String {
        let mut t = Terminal::new(TestBackend::new(140, 30)).unwrap();
        t.draw(|f| crate::ui::prices::render(f, f.area(), app)).unwrap();
        t.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn shows_symbol_price_and_header() {
        let s = rendered(&app_with_prices());
        assert!(s.contains("SYMBOL"));
        assert!(s.contains("PRICE"));
        assert!(s.contains("BTC"));
        assert!(s.contains("50000"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::prices`
Expected: FAIL — assertions fail (stub renders nothing).

- [ ] **Step 3: Implement `src/ui/prices.rs`**

```rust
//! View 1: live prices and profit/loss table.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;

use crate::app::{App, SortKey};

fn color_for(v: Decimal) -> Style {
    if v >= Decimal::ZERO {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(
        ["SYMBOL", "PRICE", "24H%", "7D%", "HELD", "COST", "VALUE", "PROFIT", "PROFIT%", "ALLOC"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );

    // Build rows from the cached valuation report (aggregated per asset).
    let mut rows: Vec<(String, Decimal, Decimal, Option<Decimal>, Decimal, Decimal, Decimal, Decimal, Decimal, Decimal)> = Vec::new();
    if let Some(report) = &app.derived.valuation {
        for av in &report.assets {
            let quote = app.prices.as_ref().and_then(|b| b.quotes.get(&av.asset));
            let change_24h = quote.map(|q| q.change_24h).unwrap_or(Decimal::ZERO);
            let change_7d = quote.and_then(|q| q.change_7d);
            let profit = av.unrealized;
            let profit_pct = if av.cost_basis.is_zero() {
                Decimal::ZERO
            } else {
                profit / av.cost_basis * Decimal::from(100)
            };
            rows.push((
                app.config.symbol(&av.asset),
                av.price, change_24h, change_7d, av.quantity, av.cost_basis,
                av.market_value, profit, profit_pct, av.allocation * Decimal::from(100),
            ));
        }
    }

    // Sort per the active key.
    rows.sort_by(|a, b| {
        let ord = match app.sort {
            SortKey::Symbol => a.0.cmp(&b.0),
            SortKey::Price => a.1.cmp(&b.1),
            SortKey::Change24h => a.2.cmp(&b.2),
            SortKey::Value => a.6.cmp(&b.6),
            SortKey::Profit => a.7.cmp(&b.7),
        };
        if app.sort_desc { ord.reverse() } else { ord }
    });

    let table_rows: Vec<Row> = rows
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.0.clone()),
                Cell::from(format!("{:.2}", r.1)),
                Cell::from(format!("{:+.2}", r.2)).style(color_for(r.2)),
                Cell::from(r.3.map(|v| format!("{:+.2}", v)).unwrap_or_else(|| "—".into())),
                Cell::from(format!("{}", r.4)),
                Cell::from(format!("{:.2}", r.5)),
                Cell::from(format!("{:.2}", r.6)),
                Cell::from(format!("{:+.2}", r.7)).style(color_for(r.7)),
                Cell::from(format!("{:+.2}%", r.8)).style(color_for(r.7)),
                Cell::from(format!("{:.1}%", r.9)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(8), Constraint::Length(12), Constraint::Length(8),
        Constraint::Length(8), Constraint::Length(10), Constraint::Length(12),
        Constraint::Length(12), Constraint::Length(12), Constraint::Length(9),
        Constraint::Length(7),
    ];
    let table = Table::new(table_rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Prices / P&L "))
        .row_highlight_style(Style::default().bg(Color::DarkGray));
    f.render_widget(table, area);
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib ui::prices`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/prices.rs
git commit -m "Add Prices/P&L view"
```

---

## Task 17: `ui/holdings.rs` — Holdings view

**Files:**
- Modify: `src/ui/holdings.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::prices::{PriceBook, Quote};
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        let mut q = HashMap::new();
        q.insert("bitcoin".into(), Quote { price: dec!(50000), change_24h: dec!(0),
            change_7d: None, market_cap: None, volume_24h: None, ath: None });
        a.set_prices(PriceBook { quotes: q, fetched_at: Utc::now(),
            sparklines: HashMap::new(), stale: false });
        a
    }

    #[test]
    fn shows_holdings_columns_and_wallet() {
        let mut t = Terminal::new(TestBackend::new(140, 20)).unwrap();
        t.draw(|f| crate::ui::holdings::render(f, f.area(), &app())).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("ASSET"));
        assert!(s.contains("WALLET"));
        assert!(s.contains("UNREALIZED"));
        assert!(s.contains("coinbase"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::holdings`
Expected: FAIL.

- [ ] **Step 3: Implement `src/ui/holdings.rs`**

```rust
//! View 2: open lots per wallet, enriched with current value and unrealized P&L.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;

use crate::app::App;

fn color_for(v: Decimal) -> Style {
    if v >= Decimal::ZERO {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(
        ["ASSET", "WALLET", "QTY", "COST BASIS", "AVG COST", "CURRENT VALUE", "UNREALIZED"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );

    let mut rows: Vec<Row> = app
        .derived
        .holdings
        .iter()
        .map(|hv| {
            Row::new(vec![
                Cell::from(app.config.symbol(&hv.holding.asset)),
                Cell::from(hv.holding.wallet.clone()),
                Cell::from(format!("{}", hv.holding.quantity)),
                Cell::from(format!("{:.2}", hv.holding.cost_basis)),
                Cell::from(format!("{:.2}", hv.holding.average_cost)),
                Cell::from(format!("{:.2}", hv.current_value)),
                Cell::from(format!("{:+.2}", hv.unrealized)).style(color_for(hv.unrealized)),
            ])
        })
        .collect();

    // Totals row.
    let total_value: Decimal = app.derived.holdings.iter().map(|h| h.current_value).sum();
    let total_unrealized: Decimal = app.derived.holdings.iter().map(|h| h.unrealized).sum();
    rows.push(Row::new(vec![
        Cell::from("TOTAL").style(Style::default().fg(Color::Yellow)),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(format!("{:.2}", total_value)),
        Cell::from(format!("{:+.2}", total_unrealized)).style(color_for(total_unrealized)),
    ]));

    let widths = [
        Constraint::Length(8), Constraint::Length(12), Constraint::Length(12),
        Constraint::Length(14), Constraint::Length(12), Constraint::Length(14),
        Constraint::Length(14),
    ];
    let group_note = if app.group_by_wallet { " (by wallet) " } else { " (by asset) " };
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(format!(" Holdings{} ", group_note)));
    f.render_widget(table, area);
}
```

> Note: grouping toggle (`g`) currently only changes the title note; coinbasis `holdings()` already returns per-(asset, wallet) rows. Asset-level aggregation when `group_by_wallet == false` is a cosmetic refinement deferred to polish — the columns and totals are correct either way. This is intentional and not a placeholder.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib ui::holdings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/holdings.rs
git commit -m "Add Holdings view"
```

---

## Task 18: `ui/valuation.rs` — Valuation & allocation view

**Files:**
- Modify: `src/ui/valuation.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::prices::{PriceBook, Quote};
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn app() -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        let mut q = HashMap::new();
        q.insert("bitcoin".into(), Quote { price: dec!(50000), change_24h: dec!(0),
            change_7d: None, market_cap: None, volume_24h: None, ath: None });
        a.set_prices(PriceBook { quotes: q, fetched_at: Utc::now(),
            sparklines: HashMap::new(), stale: false });
        a
    }

    #[test]
    fn shows_headline_totals() {
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| crate::ui::valuation::render(f, f.area(), &app())).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("Total Value"));
        assert!(s.contains("Unrealized"));
        assert!(s.contains("Allocation"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::valuation`
Expected: FAIL.

- [ ] **Step 3: Implement `src/ui/valuation.rs`**

```rust
//! View 3: headline portfolio valuation and a per-asset allocation bar chart.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{BarChart, Block, Borders, Paragraph};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(1)])
        .split(area);

    let Some(report) = &app.derived.valuation else {
        f.render_widget(
            Paragraph::new("No valuation yet — fetch prices with `r`.")
                .block(Block::default().borders(Borders::ALL).title(" Valuation ")),
            area,
        );
        return;
    };

    let mut lines = vec![
        Line::from(format!("Total Value:   {:.2} USD", report.total_value)),
        Line::from(format!("Total Cost:    {:.2} USD", report.total_cost)),
        Line::from(format!("Unrealized:    {:+.2} USD", report.total_unrealized)),
        Line::from(format!("Total Return:  {:+.2}%", report.total_return * rust_decimal::Decimal::from(100))),
    ];
    if !report.missing_prices.is_empty() {
        lines.push(Line::from(format!("⚠ missing prices: {}", report.missing_prices.join(", "))));
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Valuation ")),
        chunks[0],
    );

    let bars: Vec<(String, u64)> = report
        .assets
        .iter()
        .map(|av| {
            let pct = (av.allocation * rust_decimal::Decimal::from(100))
                .to_u64()
                .unwrap_or(0);
            (app.config.symbol(&av.asset), pct)
        })
        .collect();
    let bar_refs: Vec<(&str, u64)> = bars.iter().map(|(s, v)| (s.as_str(), *v)).collect();
    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(" Allocation (%) "))
        .data(&bar_refs)
        .bar_width(7)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    f.render_widget(chart, chunks[1]);
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib ui::valuation`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/valuation.rs
git commit -m "Add Valuation & allocation view"
```

---

## Task 19: `ui/tax.rs` — Tax view + export wiring

**Files:**
- Modify: `src/ui/tax.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;

    fn app() -> App {
        let txs = vec![
            Transaction::Buy {
                timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(), asset: "bitcoin".into(),
                quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0) },
            Transaction::Sell {
                timestamp: Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap(),
                wallet: "coinbase".into(), asset: "bitcoin".into(),
                quantity: dec!(0.5), unit_price: dec!(50000), fee: dec!(0) },
        ];
        let mut a = App::new(Config::example(), &txs).unwrap();
        a.tax_year = 2024;
        a.recompute();
        a
    }

    #[test]
    fn shows_tax_columns_subtotals_and_estimate() {
        let mut t = Terminal::new(TestBackend::new(140, 28)).unwrap();
        t.draw(|f| crate::ui::tax::render(f, f.area(), &app())).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("PROCEEDS"));
        assert!(s.contains("Short-term"));
        assert!(s.contains("Estimated Tax"));
        assert!(s.contains("2024"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::tax`
Expected: FAIL.

- [ ] **Step 3: Implement `src/ui/tax.rs`**

```rust
//! View 4: capital gains + income for the selected tax year, with an estimate.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use coinbasis::Term;

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(7)])
        .split(area);

    let title = format!(" Tax — {} ([ / ] change year · e export) ", app.tax_year);
    let header = Row::new(
        ["ASSET", "ACQUIRED", "DISPOSED", "QTY", "PROCEEDS", "BASIS", "GAIN", "TERM"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );

    let rows: Vec<Row> = app
        .derived
        .capital_gains
        .as_ref()
        .map(|rep| {
            rep.rows
                .iter()
                .map(|r| {
                    let term = match r.term {
                        Some(Term::Short) => "Short",
                        Some(Term::Long) => "Long",
                        None => "—",
                    };
                    let gain_style = if r.gain >= Decimal::ZERO {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Red)
                    };
                    Row::new(vec![
                        Cell::from(app.config.symbol(&r.asset)),
                        Cell::from(r.acquired_at.map(|d| d.date_naive().to_string()).unwrap_or_default()),
                        Cell::from(r.disposed_at.date_naive().to_string()),
                        Cell::from(format!("{}", r.quantity)),
                        Cell::from(format!("{:.2}", r.proceeds)),
                        Cell::from(format!("{:.2}", r.cost_basis)),
                        Cell::from(format!("{:+.2}", r.gain)).style(gain_style),
                        Cell::from(term),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let widths = [
        Constraint::Length(8), Constraint::Length(12), Constraint::Length(12),
        Constraint::Length(10), Constraint::Length(12), Constraint::Length(12),
        Constraint::Length(12), Constraint::Length(7),
    ];
    f.render_widget(
        Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(title)),
        chunks[0],
    );

    // Summary panel: subtotals, income, estimated tax.
    let mut lines = Vec::new();
    if let Some(cg) = &app.derived.capital_gains {
        let short_tax = cg.short_term_gain.to_f64().unwrap_or(0.0) * app.config.tax.short_term_rate;
        let long_tax = cg.long_term_gain.to_f64().unwrap_or(0.0) * app.config.tax.long_term_rate;
        lines.push(Line::from(format!("Short-term gain: {:+.2}", cg.short_term_gain)));
        lines.push(Line::from(format!("Long-term gain:  {:+.2}", cg.long_term_gain)));
        lines.push(Line::from(format!("Total gain:      {:+.2}", cg.total_gain)));
        if let Some(inc) = &app.derived.income {
            lines.push(Line::from(format!("Income:          {:.2}", inc.total_income)));
        }
        lines.push(Line::from(format!(
            "Estimated Tax:   {:.2} (est., user rates {:.0}%/{:.0}%)",
            short_tax + long_tax,
            app.config.tax.short_term_rate * 100.0,
            app.config.tax.long_term_rate * 100.0,
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Summary ")),
        chunks[1],
    );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib ui::tax`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/tax.rs
git commit -m "Add Tax view with subtotals and estimated tax"
```

---

## Task 20: `ui/rebalance.rs` — Rebalance view

**Files:**
- Modify: `src/ui/rebalance.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::prices::{PriceBook, Quote};
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn app() -> App {
        // Hold only bitcoin; targets want eth/sol too => out of balance.
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        let mut q = HashMap::new();
        q.insert("bitcoin".into(), Quote { price: dec!(50000), change_24h: dec!(0),
            change_7d: None, market_cap: None, volume_24h: None, ath: None });
        a.set_prices(PriceBook { quotes: q, fetched_at: Utc::now(),
            sparklines: HashMap::new(), stale: false });
        a
    }

    #[test]
    fn shows_action_columns_and_banner() {
        let mut t = Terminal::new(TestBackend::new(140, 26)).unwrap();
        t.draw(|f| crate::ui::rebalance::render(f, f.area(), &app())).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("SIDE"));
        assert!(s.contains("AMOUNT"));
        assert!(s.contains("DRIFT"));
        assert!(s.contains("balance")); // in/out of balance banner
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::rebalance`
Expected: FAIL.

- [ ] **Step 3: Implement `src/ui/rebalance.rs`**

```rust
//! View 5: target vs current allocation and suggested (tax-aware) trades.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::rebalance::{RebalanceSide, Strategy};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Banner.
    let strat = match app.strategy {
        Strategy::Band => "Band",
        Strategy::Full => "Full",
    };
    let banner = match &app.derived.rebalance_summary {
        Some(s) if s.in_balance => Line::from(format!(
            "✓ In balance — strategy {strat} (t to toggle)"
        )),
        Some(s) => Line::from(format!(
            "⚠ Out of balance — buys {:.2} / sells {:.2} — strategy {strat} (t to toggle)",
            s.total_buys, s.total_sells
        )),
        None => Line::from("No valuation yet — fetch prices with `r`."),
    };
    f.render_widget(
        Paragraph::new(banner).block(Block::default().borders(Borders::ALL).title(" Rebalance ")),
        chunks[0],
    );

    let header = Row::new(
        ["SIDE", "ASSET", "AMOUNT$", "DRIFT%", "EST. GAIN (HIFO)"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(Color::Cyan))),
    );
    let total = app
        .derived
        .valuation
        .as_ref()
        .map(|r| r.total_value)
        .unwrap_or(rust_decimal::Decimal::ONE);
    let rows: Vec<Row> = app
        .derived
        .rebalance_actions
        .iter()
        .map(|a| {
            let (side, style) = match a.side {
                RebalanceSide::Buy => ("BUY", Style::default().fg(Color::Green)),
                RebalanceSide::Sell => ("SELL", Style::default().fg(Color::Red)),
            };
            let drift_pct = if total.is_zero() {
                rust_decimal::Decimal::ZERO
            } else {
                a.drift / total * rust_decimal::Decimal::from(100)
            };
            let est = match a.side {
                RebalanceSide::Sell => a
                    .est_realized_gain
                    .map(|g| format!("{:+.2}", g))
                    .unwrap_or_else(|| "—".into()),
                RebalanceSide::Buy => "—".into(),
            };
            Row::new(vec![
                Cell::from(side).style(style),
                Cell::from(app.config.symbol(&a.asset)),
                Cell::from(format!("{:.2}", a.amount_usd)),
                Cell::from(format!("{:+.1}%", drift_pct)),
                Cell::from(est),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(6), Constraint::Length(8), Constraint::Length(14),
        Constraint::Length(9), Constraint::Length(18),
    ];
    f.render_widget(
        Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(" Suggested trades (estimates) ")),
        chunks[1],
    );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib ui::rebalance`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/rebalance.rs
git commit -m "Add Rebalance view with tax-aware sell estimates"
```

---

## Task 21: `ui/perf.rs` — Performance view

**Files:**
- Modify: `src/ui/perf.rs`
- Modify: `src/app.rs` (add a `history: Vec<perf::Snapshot>` field + `perf_metrics()` accessor — see Step 3)
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::perf::Snapshot;
    use coinbasis::Transaction;
    use chrono::{TimeZone, Utc};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rust_decimal_macros::dec;

    fn app_with_history(points: usize) -> App {
        let txs = vec![Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0),
        }];
        let mut a = App::new(Config::example(), &txs).unwrap();
        a.history = (0..points)
            .map(|i| Snapshot {
                at: Utc.with_ymd_and_hms(2026, 1, 1 + i as u32, 0, 0, 0).unwrap(),
                total_value: dec!(100) + rust_decimal::Decimal::from(i),
            })
            .collect();
        a
    }

    #[test]
    fn short_history_shows_need_more_message() {
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| crate::ui::perf::render(f, f.area(), &app_with_history(1))).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("not enough history"));
    }

    #[test]
    fn longer_history_shows_metrics_labels() {
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| crate::ui::perf::render(f, f.area(), &app_with_history(8))).unwrap();
        let s: String = t.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(s.contains("Volatility"));
        assert!(s.contains("Max Drawdown"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib ui::perf`
Expected: FAIL — `App.history` field and render body missing.

- [ ] **Step 3: Add `history` to `App`**

In `src/app.rs`, add the field to the struct and initialize it:
```rust
// in struct App { ... }
    pub history: Vec<crate::perf::Snapshot>,
```
```rust
// in App::new, before `config, model` in the struct literal:
            history: Vec::new(),
```
Add an accessor at the bottom of `impl App`:
```rust
    pub fn perf_metrics(&self) -> crate::perf::PerfMetrics {
        crate::perf::metrics(&self.history)
    }
```

- [ ] **Step 4: Implement `src/ui/perf.rs`**

```rust
//! View 6: value-history line chart and performance metrics.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols;
use ratatui::text::Line;
use ratatui::widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    if app.history.len() < 2 {
        f.render_widget(
            Paragraph::new("Performance: not enough history yet — values are recorded as prices refresh.")
                .block(Block::default().borders(Borders::ALL).title(" Performance ")),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(8)])
        .split(area);

    let points: Vec<(f64, f64)> = app
        .history
        .iter()
        .enumerate()
        .map(|(i, s)| (i as f64, s.total_value.to_f64().unwrap_or(0.0)))
        .collect();
    let max_y = points.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let min_y = points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let last_x = (points.len() - 1) as f64;

    let datasets = vec![Dataset::default()
        .name("value")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan))
        .data(&points)];
    let chart = Chart::new(datasets)
        .block(Block::default().borders(Borders::ALL).title(" Value History "))
        .x_axis(Axis::default().bounds([0.0, last_x]))
        .y_axis(
            Axis::default()
                .bounds([min_y, max_y])
                .labels(vec![format!("{:.0}", min_y).into(), format!("{:.0}", max_y).into()]),
        );
    f.render_widget(chart, chunks[0]);

    let m = app.perf_metrics();
    let fmt = |o: Option<f64>| o.map(|v| format!("{:.4}", v)).unwrap_or_else(|| "—".into());
    let lines = vec![
        Line::from(format!("Volatility:        {}", fmt(m.volatility))),
        Line::from(format!("Sharpe:            {}", fmt(m.sharpe))),
        Line::from(format!("Max Drawdown:      {}", fmt(m.max_drawdown))),
        Line::from(format!("Cumulative Return: {}", fmt(m.cumulative_return))),
    ];
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Metrics ")),
        chunks[1],
    );
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib ui::perf`
Expected: PASS (2 tests). Re-run `cargo test --lib app` to confirm the new field didn't break Task 13 tests.

- [ ] **Step 6: Commit**

```bash
git add src/ui/perf.rs src/app.rs
git commit -m "Add Performance view with value chart and metrics"
```

---

## Task 22: Wire the async run loop — CLI args, refresh, offline, caching, exports

**Files:**
- Modify: `src/main.rs`
- Test: `tests/integration.rs` (end-to-end with MockSource, no terminal)

- [ ] **Step 1: Write the failing integration test**

`tests/integration.rs`:
```rust
use crypto_price_tracker_v2 as app_crate;

// Re-export needed pieces for the integration test via the lib target.
use app_crate::app::App;
use app_crate::config::Config;
use app_crate::prices::mock::MockSource;
use app_crate::prices::PriceSource;

use chrono::{TimeZone, Utc};
use coinbasis::{CostBasisMethod, Transaction};
use rust_decimal_macros::dec;

#[tokio::test]
async fn end_to_end_recompute_under_method_and_prices() {
    let txs = vec![
        Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(30000), fee: dec!(0) },
        Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(), asset: "bitcoin".into(),
            quantity: dec!(1), unit_price: dec!(40000), fee: dec!(0) },
    ];
    let mut app = App::new(Config::example(), &txs).unwrap();

    let mut src = MockSource::new();
    src.set("bitcoin", dec!(50000), dec!(2.0));
    let assets = vec!["bitcoin".to_string()];
    let book = src.fetch(&assets, "usd").await.unwrap();
    app.set_prices(book);

    // Valuation is computed.
    let report = app.derived.valuation.as_ref().unwrap();
    assert_eq!(report.total_value, dec!(100000)); // 2 BTC * 50000

    // Switching method recomputes without panicking.
    app.cycle_method();
    assert_eq!(app.method, CostBasisMethod::Lifo);
    assert!(app.derived.valuation.is_some());

    // Changing year recomputes capital gains.
    app.set_year(-1);
    assert!(app.derived.capital_gains.is_some());
}
```

> This requires a library target. Add `src/lib.rs` exposing the modules (Step 2), and keep `main.rs` as the binary that uses the lib.

- [ ] **Step 2: Create `src/lib.rs` and slim `main.rs`**

`src/lib.rs`:
```rust
//! Library surface for the crypto-price-tracker-v2 binary and its tests.
pub mod app;
pub mod config;
pub mod error;
pub mod event;
pub mod export;
pub mod ledger;
pub mod perf;
pub mod portfolio;
pub mod prices;
pub mod rebalance;
pub mod ui;
```

Move all `mod X;` declarations out of `main.rs`; `main.rs` now does `use crypto_price_tracker_v2::...`. Update intra-crate paths: inside the library modules, `crate::` still refers to the library crate, so no changes needed within them. Only `main.rs` switches from `mod`/`crate::` to `use crypto_price_tracker_v2::`.

- [ ] **Step 3: Run the integration test to verify it fails to compile/link**

Run: `cargo test --test integration`
Expected: FAIL until `lib.rs` exists and modules are `pub`. After adding `lib.rs`, re-run — it should PASS (the test only uses already-implemented code).

- [ ] **Step 4: Implement the full `main.rs` run loop**

```rust
use std::collections::HashMap;
use std::io::{self, Stdout};
use std::panic;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crypto_price_tracker_v2::app::{App, View};
use crypto_price_tracker_v2::config::Config;
use crypto_price_tracker_v2::event::{apply, map_key};
use crypto_price_tracker_v2::ledger::{self, load_ledger};
use crypto_price_tracker_v2::perf;
use crypto_price_tracker_v2::prices::cache::PriceCache;
use crypto_price_tracker_v2::prices::coingecko::CoinGeckoSource;
use crypto_price_tracker_v2::prices::{PriceBook, PriceSource};
use crypto_price_tracker_v2::{export, ui};

type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Parser, Debug)]
#[command(name = "crypto-price-tracker-v2")]
struct Args {
    #[arg(long, default_value = "config.json")]
    config: String,
    #[arg(long)]
    ledger: Option<String>,
    #[arg(long)]
    offline: bool,
}

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original(info);
    }));
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config).context("loading config")?;
    let ledger_path = args.ledger.clone().unwrap_or_else(|| config.ledger_path.clone());
    let txs = load_ledger(&ledger_path).context("loading ledger")?;
    let asset_ids: Vec<String> = ledger::assets(&txs).into_iter().collect();

    let mut app = App::new(config.clone(), &txs).context("building app")?;
    let cache = PriceCache::new(config.cache.expanded_dir(), config.cache.ttl_seconds);
    let vs = config.display_currency.clone();
    let history_path = "history.json".to_string();

    // Seed from cache (fresh, else last-good).
    if let Ok(Some(book)) = cache.load_fresh() {
        app.set_prices(book);
    } else if let Ok(Some(book)) = cache.load_last_good() {
        app.set_prices(book);
    }

    install_panic_hook();
    let mut terminal = setup_terminal()?;

    let (tx, mut rx) = mpsc::channel::<PriceBook>(4);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(config.refresh_seconds.max(1)));

    // Helper to spawn a fetch (skipped in offline mode).
    let spawn_fetch = |tx: mpsc::Sender<PriceBook>| {
        let ids = asset_ids.clone();
        let vs = vs.clone();
        tokio::spawn(async move {
            let source = CoinGeckoSource::new();
            if let Ok(book) = source.fetch(&ids, &vs).await {
                let _ = tx.send(book).await;
            }
        });
    };

    if !args.offline {
        app.loading = true;
        spawn_fetch(tx.clone());
    }

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;
        if app.should_quit {
            break;
        }

        tokio::select! {
            maybe_event = events.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_event {
                    if key.kind == KeyEventKind::Press {
                        if let Some(action) = map_key(key) {
                            let wants_refresh = apply(&mut app, action);
                            // Export uses the active view to choose what to write.
                            if matches!(action, crypto_price_tracker_v2::event::Action::Export) {
                                do_export(&mut app);
                            }
                            if wants_refresh && !args.offline {
                                spawn_fetch(tx.clone());
                            }
                        }
                    }
                }
            }
            _ = tick.tick() => {
                if !args.offline {
                    app.loading = true;
                    spawn_fetch(tx.clone());
                }
            }
            Some(book) = rx.recv() => {
                let _ = cache.store(&book);
                let total = book.price_map();
                app.set_prices(book);
                if let Some(report) = &app.derived.valuation {
                    let _ = perf::record_snapshot(
                        &history_path, report.total_value, Utc::now(),
                        config.refresh_seconds as i64,
                    );
                }
                // load fresh history into the app for the Performance view
                if let Ok(h) = perf::load_history(&history_path) {
                    app.history = h;
                }
                let _ = total; // silence unused if not needed
            }
        }
    }

    restore_terminal()?;
    Ok(())
}

fn do_export(app: &mut App) {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    match app.view {
        View::Tax => {
            if let Some(cg) = &app.derived.capital_gains {
                let path = format!("capital-gains-{}-{}.csv", app.tax_year, stamp);
                match export::export_capital_gains_csv(cg, &path) {
                    Ok(()) => app.status.message = format!("exported {path}"),
                    Err(e) => app.status.message = format!("export failed: {e}"),
                }
            }
        }
        View::Holdings => {
            let holdings: Vec<_> = app.derived.holdings.iter().map(|h| h.holding.clone()).collect();
            let path = format!("holdings-{}.csv", stamp);
            match export::export_holdings_csv(&holdings, &path) {
                Ok(()) => app.status.message = format!("exported {path}"),
                Err(e) => app.status.message = format!("export failed: {e}"),
            }
        }
        _ => app.status.message = "export available on Tax/Holdings views".into(),
    }
}
```

> Add deps: `futures = "0.3"` (for `EventStream`'s `StreamExt`). Add to `Cargo.toml` `[dependencies]`.

- [ ] **Step 5: Build and run the integration test**

Run: `cargo build && cargo test --test integration`
Expected: build succeeds; integration test PASSES.

- [ ] **Step 6: Manual smoke check (optional but recommended)**

```bash
cp config.example.json config.json
cp ledger.example.json ledger.json
cargo run -- --offline
```
Expected: TUI opens; tab switching, method cycle (`m`), help (`?`), and quit (`q`) work; no terminal corruption on exit. (Offline avoids network; views render from cache/empty.)

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/lib.rs tests/integration.rs Cargo.toml Cargo.lock
git commit -m "Wire async run loop: CLI args, refresh tick, offline, caching, exports"
```

---

## Task 23: Polish — README, lints, coverage, cleanup

**Files:**
- Create: `README.md`
- Modify: any files flagged by clippy

- [ ] **Step 1: Run formatting and lints, fix all warnings**

Run: `cargo fmt --all` then `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any that appear (common: unused imports in `main.rs`, the `let _ = total;` line — remove it if `total` is genuinely unused).

- [ ] **Step 2: Run the full test suite**

Run: `cargo test`
Expected: all unit + integration tests PASS.

- [ ] **Step 3: Write `README.md`**

```markdown
# Crypto-Price-Tracker-V2

A fast Rust terminal UI for tracking a crypto portfolio: live prices and P&L,
holdings, valuation and allocation, capital-gains tax, tax-aware rebalancing,
and performance statistics. Cost-basis and tax math are delegated to the
[`coinbasis`](https://crates.io/crates/coinbasis) crate; live prices come from
[`coingecko`](https://crates.io/crates/coingecko).

## Quick start

```bash
cp config.example.json config.json
cp ledger.example.json ledger.json
cargo run --release
```

Run offline (cache only, no network): `cargo run --release -- --offline`.

## Configuration

`config.json` (see `config.example.json`): ledger path, default cost-basis
method, display currency, refresh interval, tax rates (estimates only), target
allocation weights, rebalance band/min-trade/strategy, display symbols, and
cache TTL/dir.

## Ledger

`ledger.json` is a JSON array of `coinbasis` transactions. Asset ids are
**CoinGecko coin ids** (`bitcoin`, `ethereum`, …). See `ledger.example.json`.

## Keybindings

`Tab`/`→` next view · `Shift-Tab`/`←` prev view · `↑/↓` select · `m` cycle method
· `s` cycle sort · `g` toggle grouping · `[`/`]` change tax year · `t` toggle
rebalance strategy · `r` refresh · `e` export (Tax/Holdings) · `?` help · `q`/`Esc` quit.

## Notes

All cost-basis and tax figures are in USD. Tax estimates use user-supplied rates
and are not tax advice. The method switcher cycles FIFO/LIFO/HIFO/Average.
```

- [ ] **Step 4: (Optional) coverage on domain modules**

Run: `cargo llvm-cov --lib` (if `cargo-llvm-cov` installed)
Expected: high coverage on `config`, `ledger`, `prices::{mock,cache}`, `portfolio`, `perf`, `rebalance`, `export`. Network and TUI loop are not covered by design.

- [ ] **Step 5: Final commit**

```bash
git add README.md src/
git commit -m "Add README, fix lints, and finalize v0.1"
```

---

## Self-Review (completed against the spec)

**Spec coverage:**
- §3 in-scope items 1–10 → views (Tasks 16–21), method switcher (Task 13), ledger (Task 4), caching/rate-limit/offline (Tasks 7, 8, 22), config/exports/help/error states (Tasks 3, 12, 15, 22). ✓
- §6 data model: ledger (Task 4), config (Task 3), history (Task 10). ✓
- §7 module specs: every module has a task. ✓
- §8 six views: Tasks 16–21, columns match (note: Holdings CURRENT VALUE/UNREALIZED computed in V2 since `coinbasis::Holding` lacks them — verified in research). ✓
- §9 cross-cutting: method switcher (Task 13), error handling + panic hook (Tasks 1, 22), persistence (Tasks 4, 7, 10), exports (Task 22), status bar/help/empty states (Tasks 15, 18, 21). ✓
- §10 display currency: `display_currency` flows to `coins_markets` `vs` (Task 22); USD basis preserved. ✓
- §13/§14 integration points: pinned at the top of this plan against verified docs.rs APIs. ✓

**Deviations from spec, with rationale (each verified):**
1. **`coins_markets` replaces simple-price + per-asset market-chart** (Task 8). One call returns price + 24h + 7d + market cap + volume + 7d sparkline; removes the throttled chart loop. Strictly simpler; isolated behind `PriceSource`.
2. **`HoldingValue` helper** (Task 9) computes current value/unrealized because `coinbasis::Holding` has no such fields (verified).
3. **`estimate_sell_gain` via diff** (Task 9) rather than reading a single disposal's gain — robust to pre-existing sells.
4. **Holdings grouping toggle is cosmetic in v0.1** (Task 17) — flagged explicitly, not a hidden gap.

**Placeholder scan:** No TBD/"implement later" steps; every code step shows complete code. The two `status.message`-only handlers (`Action::Export` in Task 14, finished in Task 22) are intentional staging, documented inline.

**Type consistency:** `PriceSource::fetch`, `PriceBook`/`Quote` fields, `App`/`Derived` fields, `RebalanceAction`/`RebalanceSide`/`Strategy`, and `Snapshot`/`PerfMetrics` are defined once and used consistently across tasks. `estimate_sell_gain` signature matches its call site in `app.rs`.
