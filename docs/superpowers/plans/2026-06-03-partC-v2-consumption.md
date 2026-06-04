# Part C — Crypto-Price-Tracker-V2 Consumption — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Wire `coinbasis 0.2` (tax brackets) and `cryptolytics 0.1` (analytics) into the V2 TUI and add CSV import, CoinGecko key/plan config, rebalance analytics, and history reconstruction/playback — closing the remaining Python-parity gaps.

**Architecture:** Four sub-parts — C1 tax brackets, C2 CSV import, C3 rebalance analytics (new `fetch_history` price path + a `FetchMsg` channel + cryptolytics-driven `recompute`), C4 history reconstruction/playback (ledger-replay valuation + a Performance playback mode). V2 stays `Decimal`; convert to `f64` at each cryptolytics boundary.

**Tech Stack:** Rust 2021, `coinbasis 0.2` (serde), `cryptolytics 0.1`, `ratatui`/`crossterm`, `tokio`, `coingecko`, `rust_decimal`, `chrono`, `clap`.

**Repo:** `/Users/jacobkanfer/CodeRepos/Crypto-Price-Tracker-V2`. **Branch:** `feat/parity-phase-c`; ff-merge to `main` when green.

**Author rule (hook-enforced):** author `51518860+Technical-1@users.noreply.github.com`; NO AI/Co-Authored-By attribution; never `--no-verify`.

**GATE:** Part C compiles only after **coinbasis 0.2.0** (Part A) and **cryptolytics 0.1.0** (Part B) are published. Fallback during dev: `[patch.crates-io]` path deps to the local checkouts, removed before the final commit.

**Spec:** `docs/superpowers/specs/2026-06-03-partC-v2-consumption-design.md`.

---

## Verified V2 facts (pinned)
- `config.rs`: `Config { ledger_path, default_method: CostBasisMethod, display_currency, refresh_seconds: u64, tax, targets: BTreeMap<String,Decimal>, rebalance: RebalanceConfig, symbols, cache: CacheConfig }`, `Config::example()`.
- `prices/mod.rs`: `Quote { price, change_24h, change_7d, market_cap, volume_24h, ath }` (Decimals); `PriceBook { quotes: HashMap<String,Quote>, fetched_at, sparklines, stale }`; `trait PriceSource { async fn fetch(&self, ids: &[String], vs: &str) -> Result<PriceBook, AppError>; }`.
- `prices/coingecko.rs`: `CoinGeckoSource { client }`, `new()`; `to_decimal`/`to_opt_decimal`. `prices/mock.rs`: `MockSource { quotes }`, `set`, `fetch`.
- `app.rs`: `App { config, model: PortfolioModel, method, view, tax_year, prices: Option<PriceBook>, derived: Derived, sort, sort_desc, selected, group_by_wallet, strategy: rebalance::Strategy, status, loading, show_help, should_quit, history: Vec<perf::Snapshot>, seconds_to_refresh }`; `Derived { valuation, holdings, capital_gains, income, rebalance_actions, rebalance_summary }`; `recompute()`, `set_prices()`.
- `main.rs`: `Args { config, ledger, offline }`; mpsc channel `Result<PriceBook,String>`; `EventStream` + `tokio::select!`; `spawn_fetch` closure; `do_export`.
- `event.rs`: `Action` enum + `map_key` + `apply(&mut App, Action) -> bool`.
- `perf.rs`: `Snapshot { at: DateTime<Utc>, total_value: Decimal }`; `record_snapshot`, `load_history`, `metrics`, `PerfMetrics`.

---

# C1 — Tax brackets

## Task 1: Bump coinbasis 0.2 + migrate tax config

**Files:** `Cargo.toml`, `src/config.rs`, `config.example.json`.

- [ ] **Step 1: Branch + bump.**
```bash
cd /Users/jacobkanfer/CodeRepos/Crypto-Price-Tracker-V2 && git checkout -b feat/parity-phase-c
```
`Cargo.toml`: `coinbasis = { version = "0.1", … }` → `version = "0.2"`. `cargo update -p coinbasis && cargo build` (resolves 0.2.0). *(If not yet published, add `[patch.crates-io] coinbasis = { path = "../coinbasis" }` temporarily.)*

- [ ] **Step 2: Update the config test** (`src/config.rs` tests) — replace the `tax` block in `sample_json()` and the tax assertions in `parses_full_config`:
```rust
// sample_json() tax block:
          "tax": { "jurisdiction": "US", "long_term_threshold_days": 365,
                   "short_term_rate": "0.35",
                   "long_term_brackets": [ {"up_to": "47025", "rate": "0.0"},
                                           {"up_to": null, "rate": "0.20"} ] },
// parses_full_config(): replace the short_term_rate f64 assertion with:
        assert_eq!(c.tax.short_term_rate, rust_decimal_macros::dec!(0.35));
        assert_eq!(c.tax.long_term_threshold_days, 365);
        assert_eq!(c.tax.long_term_brackets.len(), 2);
```

- [ ] **Step 3: Run** `cargo test config` → FAIL (local `TaxConfig` lacks brackets / f64 mismatch).

- [ ] **Step 4: Migrate `src/config.rs`.** Delete V2's local `TaxConfig` struct; add `use coinbasis::tax::TaxConfig;`; keep `Config.tax: TaxConfig` (now the coinbasis type); in `Config::example()` set `tax: TaxConfig::default()`.

- [ ] **Step 5: Run** `cargo test config` → PASS.

- [ ] **Step 6: `config.example.json`** — replace `"tax"` with the bracket preset:
```json
  "tax": {
    "jurisdiction": "US",
    "long_term_threshold_days": 365,
    "short_term_rate": "0.35",
    "long_term_brackets": [
      {"up_to": "47025", "rate": "0.0"},
      {"up_to": "518900", "rate": "0.15"},
      {"up_to": null, "rate": "0.20"}
    ]
  },
```

- [ ] **Step 7: Commit.** `cargo fmt && cargo clippy --all-targets -- -D warnings`.
```bash
git add Cargo.toml Cargo.lock src/config.rs config.example.json
git commit -m "Bump coinbasis 0.2 and migrate tax config to coinbasis::tax::TaxConfig"
```

## Task 2: Bracketed estimate in the Tax view

**Files:** `src/ui/tax.rs`.

- [ ] **Step 1: Update the test** — add to `shows_tax_columns_subtotals_and_estimate`: `assert!(s.contains("US"));` (jurisdiction label).
- [ ] **Step 2: Run** `cargo test ui::tax` → FAIL.
- [ ] **Step 3: Rewrite the summary block** in `src/ui/tax.rs` (replace the flat-rate `short_tax`/`long_tax` computation):
```rust
    let mut lines = Vec::new();
    if let Some(cg) = &app.derived.capital_gains {
        let est = coinbasis::tax::estimate(cg, &app.config.tax);
        lines.push(Line::from(format!("Short-term gain: {:+.2}    tax: {:.2}", est.short_term_gain, est.short_term_tax)));
        lines.push(Line::from(format!("Long-term gain:  {:+.2}    tax: {:.2}", est.long_term_gain, est.long_term_tax)));
        lines.push(Line::from(format!("Total gain:      {:+.2}", cg.total_gain)));
        lines.push(Line::from(format!("Estimated Tax:   {:.2}  ({}, brackets)", est.total_tax, app.config.tax.jurisdiction)));
    }
    if let Some(inc) = &app.derived.income {
        lines.push(Line::from(format!("Income:          {:.2}", inc.total_income)));
    }
```
Remove the now-unused `rust_decimal::prelude::ToPrimitive` import if no longer referenced.
- [ ] **Step 4: Run** `cargo test ui::tax` + `cargo build` → PASS.
- [ ] **Step 5: Commit.** `cargo fmt && cargo clippy --all-targets -- -D warnings`. `git add src/ui/tax.rs && git commit -m "Render bracketed tax estimate via coinbasis::tax"`.

---

# C2 — CSV import

## Task 3: `import_csv`

**Files:** `src/ledger.rs`.

- [ ] **Step 1: Failing tests** (append to `src/ledger.rs` tests) — covering buy/sell + optional/default wallet, dedup, invalid-row skip + blank-fee default:
```rust
    #[test]
    fn import_maps_buys_sells_optional_wallet() {
        let dir = std::env::temp_dir().join("cpt2_imp_basic"); std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv"); let led = dir.join("l.json"); let _ = std::fs::remove_file(&led);
        std::fs::write(&csv, "date,coin,action,quantity,price_usd,fee_usd,wallet\n\
            2021-01-01,bitcoin,buy,0.5,30000,5,coinbase\n2021-06-01,bitcoin,sell,0.2,40000,2\n").unwrap();
        assert_eq!(import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(), (2, 0));
        let txs = load_ledger(led.to_str().unwrap()).unwrap();
        match &txs[0] { coinbasis::Transaction::Buy { wallet, asset, .. } => { assert_eq!(wallet, "coinbase"); assert_eq!(asset, "bitcoin"); } _ => panic!() }
        match &txs[1] { coinbasis::Transaction::Sell { wallet, .. } => assert_eq!(wallet, "default"), _ => panic!() }
    }
    #[test]
    fn import_dedups() {
        let dir = std::env::temp_dir().join("cpt2_imp_dedup"); std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv"); let led = dir.join("l.json"); let _ = std::fs::remove_file(&led);
        std::fs::write(&csv, "date,coin,action,quantity,price_usd,fee_usd\n2021-01-01,bitcoin,buy,1,30000,0\n").unwrap();
        assert_eq!(import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(), (1, 0));
        assert_eq!(import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(), (0, 1));
    }
    #[test]
    fn import_skips_invalid_and_defaults_fee() {
        let dir = std::env::temp_dir().join("cpt2_imp_bad"); std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv"); let led = dir.join("l.json"); let _ = std::fs::remove_file(&led);
        std::fs::write(&csv, "date,coin,action,quantity,price_usd,fee_usd\n\
            2021-01-01,bitcoin,buy,1,30000,\nbad,bitcoin,buy,1,30000,0\n2021-01-02,bitcoin,hodl,1,30000,0\n2021-01-03,bitcoin,buy,-1,30000,0\n").unwrap();
        assert_eq!(import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(), (1, 3));
    }
```
- [ ] **Step 2: Run** `cargo test ledger::tests::import` → FAIL.
- [ ] **Step 3: Implement** — add imports `use chrono::{NaiveDate, TimeZone, Utc}; use rust_decimal::Decimal; use std::str::FromStr;` and:
```rust
/// Import `date,coin,action,quantity,price_usd,fee_usd` (+ optional `wallet`)
/// CSV into the JSON ledger, appending non-duplicate rows. `(added, skipped)`.
pub fn import_csv(csv_path: &str, ledger_path: &str) -> Result<(usize, usize), AppError> {
    let text = std::fs::read_to_string(csv_path)
        .map_err(|e| AppError::Ledger { path: csv_path.to_string(), reason: e.to_string() })?;
    let mut existing = load_ledger(ledger_path).unwrap_or_default();
    let (mut added, mut skipped) = (0usize, 0usize);
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() { continue; }
        match parse_csv_row(line) {
            Some(tx) if existing.contains(&tx) => skipped += 1,
            Some(tx) => { existing.push(tx); added += 1; }
            None => { eprintln!("(skipped import line {}: invalid row)", i + 1); skipped += 1; }
        }
    }
    if added > 0 { save_ledger(ledger_path, &existing)?; }
    Ok((added, skipped))
}

fn parse_csv_row(line: &str) -> Option<Transaction> {
    let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if f.len() < 6 { return None; }
    let date = NaiveDate::parse_from_str(f[0], "%Y-%m-%d").ok()?;
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?);
    let asset = f[1];
    if asset.is_empty() { return None; }
    let quantity = Decimal::from_str(f[3]).ok()?;
    if quantity <= Decimal::ZERO { return None; }
    let unit_price = Decimal::from_str(f[4]).ok()?;
    if unit_price < Decimal::ZERO { return None; }
    let fee = if f[5].is_empty() { Decimal::ZERO } else { Decimal::from_str(f[5]).ok()? };
    if fee < Decimal::ZERO { return None; }
    let wallet = f.get(6).map(|s| s.to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "default".to_string());
    match f[2].to_lowercase().as_str() {
        "buy" => Some(Transaction::Buy { timestamp, wallet, asset: asset.to_string(), quantity, unit_price, fee }),
        "sell" => Some(Transaction::Sell { timestamp, wallet, asset: asset.to_string(), quantity, unit_price, fee }),
        _ => None,
    }
}
```
- [ ] **Step 4: Run** `cargo test ledger` → PASS. **Step 5: Commit** (`cargo fmt && clippy`): `git add src/ledger.rs && git commit -m "Add CSV ledger import"`.

## Task 4: `--import` flag + sample

**Files:** `src/main.rs`, `transactions.example.csv`.

- [ ] **Step 1: Add to `Args`:** `#[arg(long)] import: Option<String>,`.
- [ ] **Step 2: Wire in `main()`** after `ledger_path` is resolved, before `install_panic_hook()`:
```rust
    if let Some(csv) = args.import.as_deref() {
        let (added, skipped) = ledger::import_csv(csv, &ledger_path).context("importing CSV")?;
        println!("imported {added}, skipped {skipped} (ledger: {ledger_path})");
        return Ok(());
    }
```
- [ ] **Step 3: Smoke test.** `cargo build`; `printf 'date,coin,action,quantity,price_usd,fee_usd\n2021-01-01,bitcoin,buy,0.5,30000,5\n' > /tmp/s.csv; cargo run -- --import /tmp/s.csv --ledger /tmp/sl.json` → prints `imported 1, skipped 0 …`, exits without TUI; re-run → `imported 0, skipped 1`.
- [ ] **Step 4: `transactions.example.csv`:**
```
date,coin,action,quantity,price_usd,fee_usd,wallet
2021-01-01,bitcoin,buy,0.5,30000,5,coinbase
2021-06-01,ethereum,buy,2,2000,3,coinbase
2024-02-01,bitcoin,sell,0.2,50000,4,coinbase
```
- [ ] **Step 5: Commit** (`fmt && clippy`): `git add src/main.rs transactions.example.csv && git commit -m "Add --import flag and CSV sample"`.

---

# C3 — Rebalance analytics

## Task 5: CoinGecko key + plan config

**Files:** `src/config.rs`, `src/prices/coingecko.rs`, `src/main.rs`, `config.example.json`.

- [ ] **Step 1: Failing test** (in `src/config.rs` tests):
```rust
    #[test]
    fn parses_coingecko_key_and_plan() {
        let json = r#"{ "api_key": "abc", "plan": "Pro" }"#;
        let c: CoinGeckoConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.api_key.as_deref(), Some("abc"));
        assert_eq!(c.plan, Plan::Pro);
    }
```
- [ ] **Step 2: Run** `cargo test config::tests::parses_coingecko` → FAIL.
- [ ] **Step 3: Implement in `src/config.rs`:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Plan { #[default] Demo, Pro }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoinGeckoConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub plan: Plan,
}
```
Add `#[serde(default)] pub coingecko: CoinGeckoConfig,` to `Config`; in `Config::example()` add `coingecko: CoinGeckoConfig::default(),`. Add a `Config::coingecko_key()` helper that prefers the `COINGECKO_API_KEY` env var over `self.coingecko.api_key`:
```rust
    pub fn coingecko_key(&self) -> Option<String> {
        std::env::var("COINGECKO_API_KEY").ok().filter(|s| !s.is_empty())
            .or_else(|| self.coingecko.api_key.clone())
    }
```
- [ ] **Step 4: `CoinGeckoSource::new` selects the constructor** — change `src/prices/coingecko.rs`:
```rust
use crate::config::Plan;

impl CoinGeckoSource {
    pub fn new(api_key: Option<&str>, plan: Plan) -> Self {
        let client = match (api_key, plan) {
            (Some(k), Plan::Pro) => CoinGeckoClient::new_with_pro_api_key(k),
            (Some(k), Plan::Demo) => CoinGeckoClient::new_with_demo_api_key(k),
            (None, _) => CoinGeckoClient::new(coingecko::COINGECKO_API_DEMO_URL),
        };
        Self { client }
    }
}
```
Update `Default for CoinGeckoSource` to `Self::new(None, Plan::Demo)`. Update the call site in `main.rs`'s `spawn_fetch` to `CoinGeckoSource::new(key.as_deref(), plan)` (capture `let key = config.coingecko_key(); let plan = config.coingecko.plan;` before the loop).
- [ ] **Step 5: `config.example.json`** — add `"coingecko": { "api_key": "", "plan": "Demo" },`.
- [ ] **Step 6: Run + commit.** `cargo test config && cargo build && cargo fmt && cargo clippy --all-targets -- -D warnings`. `git add -A && git commit -m "Add CoinGecko API key + Demo/Pro plan config"`.

## Task 6: `fetch_history` on PriceSource + cryptolytics dep

**Files:** `Cargo.toml`, `src/prices/mod.rs`, `src/prices/mock.rs`, `src/prices/coingecko.rs`.

- [ ] **Step 1: Add dep.** `Cargo.toml`: `cryptolytics = "0.1"` (or `[patch.crates-io]` path until published).
- [ ] **Step 2: Failing test** (in `src/prices/mock.rs` tests):
```rust
    #[tokio::test]
    async fn mock_history_returns_configured_series() {
        use chrono::{TimeZone, Utc};
        let mut s = MockSource::new();
        s.set_history("bitcoin", vec![(Utc.with_ymd_and_hms(2024,1,1,0,0,0).unwrap(), 100.0),
                                      (Utc.with_ymd_and_hms(2024,1,2,0,0,0).unwrap(), 110.0)]);
        let h = s.fetch_history(&["bitcoin".to_string()], "usd", 2).await.unwrap();
        assert_eq!(h["bitcoin"].len(), 2);
        assert_eq!(h["bitcoin"][1].1, 110.0);
    }
```
- [ ] **Step 3: Extend the trait** in `src/prices/mod.rs`:
```rust
/// asset id -> daily (timestamp, price) series.
pub type HistoryData = std::collections::HashMap<String, Vec<(chrono::DateTime<chrono::Utc>, f64)>>;
```
Add to `trait PriceSource`:
```rust
    /// Fetch up to `days` of daily price history per id (best-effort per id).
    async fn fetch_history(&self, ids: &[String], vs: &str, days: u32) -> Result<HistoryData, AppError>;
```
- [ ] **Step 4: Implement on `MockSource`** (`src/prices/mock.rs`): add a `history: HashMap<String, Vec<(DateTime<Utc>, f64)>>` field + `set_history(&mut self, id: &str, series: Vec<(DateTime<Utc>, f64)>)`, and:
```rust
    async fn fetch_history(&self, ids: &[String], _vs: &str, _days: u32) -> Result<HistoryData, AppError> {
        Ok(ids.iter().filter_map(|id| self.history.get(id).map(|h| (id.clone(), h.clone()))).collect())
    }
```
- [ ] **Step 5: Implement on `CoinGeckoSource`** (`src/prices/coingecko.rs`):
```rust
    async fn fetch_history(&self, ids: &[String], vs: &str, days: u32) -> Result<HistoryData, AppError> {
        use chrono::{TimeZone, Utc};
        let mut out = HistoryData::new();
        for id in ids {
            match self.client.coin_market_chart(id, vs, days as i64, true).await {
                Ok(chart) => {
                    let series = chart.prices.iter().filter_map(|p| {
                        let (ms, price) = (*p.first()? as i64, *p.get(1)?);
                        Utc.timestamp_millis_opt(ms).single().map(|t| (t, price))
                    }).collect();
                    out.insert(id.clone(), series);
                }
                Err(_) => { /* best-effort: skip this id */ }
            }
        }
        Ok(out)
    }
```
(Confirm `coin_market_chart(&self, id, vs_currency, days: i64, interval_daily: bool)` and `MarketChart.prices: Vec<Vec<f64>>` against the installed `coingecko` version; adjust if the signature differs and report.)
- [ ] **Step 6: Run + commit.** `cargo test prices && cargo build && cargo fmt && cargo clippy --all-targets -- -D warnings`. `git add -A && git commit -m "Add PriceSource::fetch_history and cryptolytics dependency"`.

## Task 7: `FetchMsg` channel + history fetch trigger

**Files:** `src/main.rs`.

- [ ] **Step 1: Define the message enum** in `main.rs`:
```rust
enum FetchMsg {
    Prices(Result<PriceBook, String>),
    History(Result<crypto_price_tracker_v2::prices::HistoryData, String>),
}
```
- [ ] **Step 2: Change the channel** to `mpsc::channel::<FetchMsg>(4)`; update `run()` param types and the `spawn_fetch` closure to send `FetchMsg::Prices(...)`. Add a `spawn_history` closure that fetches `fetch_history(&ids, &vs, history_days)` and sends `FetchMsg::History(...)` (capture `history_days = config.history_days`).
- [ ] **Step 3: Trigger history** on startup (once, if `!offline`) and on the manual `r` refresh (NOT the tick): after `spawn_fetch` on startup add `spawn_history(tx.clone());`, and in the key-handler's `Action::Refresh` branch also call `spawn_history`.
- [ ] **Step 4: Handle both arms** in the `rx.recv()` select branch:
```rust
            Some(msg) = rx.recv() => match msg {
                FetchMsg::Prices(Ok(book)) => { /* existing: cache.store, set_prices, snapshot, reload history */ }
                FetchMsg::Prices(Err(e)) => { app.loading = false; app.status.message = format!("fetch error: {e}"); }
                FetchMsg::History(Ok(h)) => { app.set_price_history(h); }
                FetchMsg::History(Err(e)) => { app.status.message = format!("history error: {e}"); }
            }
```
- [ ] **Step 5: Add `Config.history_days`** (`src/config.rs`): `#[serde(default = "default_history_days")] pub history_days: u32,` with `fn default_history_days() -> u32 { 90 }`; `Config::example()` sets `history_days: 90`; `config.example.json` adds `"history_days": 90,`. (`app.set_price_history` is added in Task 8 — until then stub it as `app.price_history = h;` after Task 8's field exists; do Task 8 before building this fully, or land Tasks 7–8 together.)
- [ ] **Step 6: Build + commit.** `cargo build && cargo fmt && cargo clippy --all-targets -- -D warnings`. `git add -A && git commit -m "Route price + history fetches through a FetchMsg channel"`.

## Task 8: App analytics state + recompute

**Files:** `src/app.rs`, `src/event.rs`.

- [ ] **Step 1: Failing test** (in `src/app.rs` tests) — set price history + cycle strategy, assert analytics populate:
```rust
    #[test]
    fn recompute_populates_rebalance_analytics() {
        use chrono::{TimeZone, Utc};
        let mut a = app(); // existing helper building an App from a fixture ledger
        let mut hist = std::collections::HashMap::new();
        hist.insert("bitcoin".to_string(), vec![
            (Utc.with_ymd_and_hms(2024,1,1,0,0,0).unwrap(), 100.0),
            (Utc.with_ymd_and_hms(2024,1,2,0,0,0).unwrap(), 110.0),
            (Utc.with_ymd_and_hms(2024,1,3,0,0,0).unwrap(), 105.0)]);
        a.set_price_history(hist);
        assert!(a.derived.vols_daily.contains_key("bitcoin"));
        assert_eq!(a.target_strategy, cryptolytics::allocation::TargetStrategy::Custom);
        a.cycle_target_strategy();
        assert_eq!(a.target_strategy, cryptolytics::allocation::TargetStrategy::Equal);
    }
```
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** in `src/app.rs`:
  - Add fields: `pub price_history: crate::prices::HistoryData` (init `HashMap::new()`), `pub target_strategy: cryptolytics::allocation::TargetStrategy` (init `TargetStrategy::Custom`).
  - Add to `Derived`: `pub vols_daily: BTreeMap<String,f64>`, `pub vols_annual: BTreeMap<String,f64>`, `pub correlation: BTreeMap<(String,String),f64>`, `pub portfolio_vol: Option<f64>`, `pub backtest_current: Option<f64>`, `pub backtest_target: Option<f64>` (all `#[derive(Default)]`-friendly).
  - Add `pub fn set_price_history(&mut self, h: HistoryData) { self.price_history = h; self.recompute(); }` and `pub fn cycle_target_strategy(&mut self)` (Custom→Equal→MarketCap→Custom; then `recompute()`).
  - In `recompute()`, after the existing rebalance block, add an analytics block:
```rust
        // Per-coin return series (f64) from price history.
        let mut returns_by_coin: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for (coin, series) in &self.price_history {
            let prices: Vec<f64> = series.iter().map(|(_, p)| *p).collect();
            let r = cryptolytics::returns::daily_returns(&prices);
            if !r.is_empty() { returns_by_coin.insert(coin.clone(), r); }
        }
        self.derived.vols_daily = returns_by_coin.iter()
            .filter_map(|(c, r)| cryptolytics::volatility::volatility(r).map(|v| (c.clone(), v))).collect();
        self.derived.vols_annual = self.derived.vols_daily.iter()
            .map(|(c, v)| (c.clone(), cryptolytics::volatility::annualize(*v, 365.0))).collect();
        self.derived.correlation = if returns_by_coin.len() >= 2 {
            cryptolytics::correlation::correlation_matrix(&returns_by_coin)
        } else { BTreeMap::new() };
        // value-weighted portfolio vol over coins with history
        if let Some(report) = &self.derived.valuation {
            let total = report.total_value.to_f64().unwrap_or(0.0);
            if total > 0.0 && self.derived.vols_daily.len() >= 2 {
                let weights: BTreeMap<String,f64> = report.assets.iter()
                    .filter(|a| self.derived.vols_daily.contains_key(&a.asset))
                    .map(|a| (a.asset.clone(), a.market_value.to_f64().unwrap_or(0.0) / total)).collect();
                self.derived.portfolio_vol = Some(cryptolytics::portfolio::portfolio_volatility(
                    &weights, &self.derived.vols_daily, &self.derived.correlation));
            } else { self.derived.portfolio_vol = None; }
            // backtest current vs target weights
            let hist_prices: BTreeMap<String, Vec<f64>> = self.price_history.iter()
                .map(|(c, s)| (c.clone(), s.iter().map(|(_, p)| *p).collect())).collect();
            let cur_w: BTreeMap<String,f64> = report.assets.iter()
                .map(|a| (a.asset.clone(), if total>0.0 { a.market_value.to_f64().unwrap_or(0.0)/total } else {0.0})).collect();
            self.derived.backtest_current = Some(cryptolytics::backtest::buy_and_hold_return(&hist_prices, &cur_w));
            let tgt_w = self.target_weights_f64(); // helper below
            self.derived.backtest_target = Some(cryptolytics::backtest::buy_and_hold_return(&hist_prices, &tgt_w));
        }
```
  - Add a `target_weights_f64(&self) -> BTreeMap<String,f64>` helper computing weights via `cryptolytics::allocation::target_weights(self.target_strategy, &assets, market_caps?, custom?)` where `custom` = `self.config.targets` as f64 and `market_caps` from `self.prices` quotes (`market_cap.to_f64()`); on `Err`, fall back to the config targets (normalized). **Also** feed these weights (as `Decimal`) into the existing `rebalance::suggest(...)` instead of only `config.targets`, so the target *source* honors `target_strategy` (convert the f64 weights back to a `BTreeMap<String,Decimal>` via `Decimal::from_f64_retain`).
- [ ] **Step 4: `event.rs`** — add `Action::CycleTargetStrategy` mapped to key `w`; in `apply`, call `app.cycle_target_strategy()`.
- [ ] **Step 5: Run** `cargo test app && cargo test event` → PASS. **Step 6: Commit** (`fmt && clippy`): `git add src/app.rs src/event.rs && git commit -m "Compute rebalance analytics via cryptolytics in recompute"`.

## Task 9: Rebalance view panels

**Files:** `src/ui/rebalance.rs`.

- [ ] **Step 1: Update the test** — add assertions that the new panels render with a history fixture: `assert!(s.contains("Risk"));`, `assert!(s.contains("Backtest"));`, `assert!(s.contains("Strategy"));` (extend the existing rebalance test to `set_price_history(...)` first).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** — insert, between the banner and the trades table, a strategy line + risk panel + (when present) correlation matrix + backtest line. Render `app.derived.vols_daily`/`vols_annual` as a small table; `app.derived.portfolio_vol` as a "Portfolio daily volatility: X%" line (or "(need ≥2 coins with history)"); `app.derived.correlation` as a labeled matrix when non-empty; a "Backtest (buy&hold): current X% / target Y%" line from `backtest_current`/`backtest_target`; and "Strategy: {target_strategy:?} · {Band|Full} (w/t)". Use a vertical layout sized to the panels (mirror the dynamic-height approach already used for the current-vs-target panel). Keep the existing trades table + tax-aware est gain column.
- [ ] **Step 4: Run** `cargo test ui::rebalance` + build → PASS. **Step 5: Commit** (`fmt && clippy`): `git add src/ui/rebalance.rs && git commit -m "Add risk, correlation, backtest, and strategy panels to Rebalance view"`.

---

# C4 — History reconstruction & playback

## Task 10: Snapshot extension + `reconstruct_series`

**Files:** `src/perf.rs`.

- [ ] **Step 1: Failing tests** (in `src/perf.rs` tests) — extended snapshot serde-default load + reconstruction:
```rust
    #[test]
    fn old_snapshot_loads_with_default_cost_pl() {
        let dir = std::env::temp_dir().join("cpt2_perf_mig"); std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("h.jsonl"); std::fs::write(&path, "{\"date\":\"2026-06-01T12:00:00Z\",\"total_value\":\"100\"}\n").unwrap();
        let h = load_history(path.to_str().unwrap()).unwrap();
        assert_eq!(h[0].cost, rust_decimal_macros::dec!(0));
    }
    #[test]
    fn reconstruct_values_holdings_as_of_each_day() {
        use chrono::{TimeZone, Utc};
        use coinbasis::{CostBasisMethod, Transaction};
        use rust_decimal_macros::dec;
        let txs = vec![Transaction::Buy { timestamp: Utc.with_ymd_and_hms(2024,1,1,0,0,0).unwrap(),
            wallet: "w".into(), asset: "bitcoin".into(), quantity: dec!(1), unit_price: dec!(100), fee: dec!(0) }];
        let mut hist = std::collections::HashMap::new();
        hist.insert("bitcoin".to_string(), vec![
            (Utc.with_ymd_and_hms(2024,1,1,0,0,0).unwrap(), 100.0),
            (Utc.with_ymd_and_hms(2024,1,2,0,0,0).unwrap(), 150.0)]);
        let series = reconstruct_series(&txs, &hist, CostBasisMethod::Fifo);
        assert_eq!(series.len(), 2);
        assert_eq!(series[1].total_value, dec!(150)); // 1 btc * 150
        assert_eq!(series[1].pl, dec!(50));           // 150 - 100 cost
    }
```
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** — change `Snapshot` to:
```rust
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
```
Switch `load_history` to read **JSONL** (one object per line; tolerate a legacy JSON-array file by trying `serde_json::from_str::<Vec<Snapshot>>` first, else line-by-line). `record_snapshot` writes a JSONL line `{date,total_value,cost,pl}` (compute `cost`/`pl` from the valuation's `total_cost`/`total_unrealized`; update its signature to accept them, and update `metrics`/`main.rs` callers). Add:
```rust
/// Reconstruct daily snapshots by replaying the ledger as-of each history date.
pub fn reconstruct_series(
    txs: &[coinbasis::Transaction],
    history: &crate::prices::HistoryData,
    method: coinbasis::CostBasisMethod,
) -> Vec<Snapshot> {
    use coinbasis::Portfolio;
    use rust_decimal::prelude::FromPrimitive;
    use std::collections::BTreeSet;
    // union of all dated points, by date
    let mut dates: BTreeSet<chrono::DateTime<Utc>> = BTreeSet::new();
    for series in history.values() { for (d, _) in series { dates.insert(*d); } }
    let mut out = Vec::new();
    for d in dates {
        let upto: Vec<coinbasis::Transaction> = txs.iter().filter(|t| tx_time(t) <= d).cloned().collect();
        let prices: std::collections::HashMap<String, Decimal> = history.iter().filter_map(|(c, s)| {
            s.iter().find(|(dt, _)| *dt == d).and_then(|(_, p)| Decimal::from_f64_retain(*p)).map(|px| (c.clone(), px))
        }).collect();
        if let Ok(p) = Portfolio::from_transactions(&upto) {
            if let Ok(r) = p.valuation(method, &prices) {
                out.push(Snapshot { date: d, total_value: r.total_value, cost: r.total_cost, pl: r.total_unrealized });
            }
        }
    }
    out
}
```
Add a private `tx_time(&Transaction) -> DateTime<Utc>` returning each variant's `timestamp` (match all 8 variants; `Transfer`/`GiftSent`/etc. carry `timestamp`).
- [ ] **Step 4: Run** `cargo test perf` → PASS. **Step 5: Commit** (`fmt && clippy`; fix `main.rs`/`metrics` snapshot callers): `git add -A && git commit -m "Extend snapshots (JSONL, cost/pl) and add ledger-replay reconstruction"`.

## Task 11: Performance chart/playback modes

**Files:** `src/app.rs`, `src/event.rs`, `src/ui/perf.rs`.

- [ ] **Step 1: State + test.** Add `pub enum PerfMode { Chart, Playback }` + `pub perf_mode: PerfMode` (default `Chart`) to `App`; add `Derived.reconstructed: Vec<perf::Snapshot>` recomputed in `recompute()` from `perf::reconstruct_series(self.model.transactions(), &self.price_history, self.method)` (empty when no history). `event.rs`: `Action::TogglePlayback` on key `p` → flips `perf_mode`. Test (`src/ui/perf.rs`): with a reconstructed fixture + `perf_mode = Playback`, the buffer shows a day row + a breakdown header.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement `src/ui/perf.rs`:**
  - **Data source:** if `!app.derived.reconstructed.is_empty()`, use it for the value/P&L series + metrics; else fall back to `app.history` (forward snapshots), as today.
  - **Chart mode:** existing value+P&L chart + metrics, over the chosen series.
  - **Playback mode:** a scrollable list — `date · value · P/L · {pl hbar}` per day (reuse the v0.2 block-bar helper), with `app.selected` (clamped) highlighted; below it a detail pane showing the selected day's per-coin breakdown — recompute that day's `Portfolio::from_transactions(txs ≤ day).holdings_with_value(method, day_prices)` (or valuation rows) and render asset · qty · price · value. ↑/↓ already move `selected`.
- [ ] **Step 4: Run** `cargo test ui::perf && cargo test app && cargo build` → PASS. **Step 5: Commit** (`fmt && clippy`): `git add -A && git commit -m "Add Performance reconstruction + playback mode"`.

---

## Task 12: Help overlay, README, finalize, merge

**Files:** `src/ui/mod.rs` (help text), `README.md`.

- [ ] **Step 1: Help overlay** — add the new keys (`w` cycle target strategy, `p` toggle playback) to the `?` help lines in `src/ui/mod.rs`; update the `ui` test asserting help contents if it checks specific lines.
- [ ] **Step 2: README** — document: tax brackets in `config.json`; `--import` CSV; CoinGecko key/plan + `COINGECKO_API_KEY`; rebalance strategies + analytics; history reconstruction/playback; `history_days`. Update the keybindings list (`w`, `p`).
- [ ] **Step 3: Remove any `[patch.crates-io]`** added during dev so `Cargo.toml` pins `coinbasis = "0.2"` + `cryptolytics = "0.1"` from crates.io; `cargo update`.
- [ ] **Step 4: Full gate.** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` → all green.
- [ ] **Step 5: Commit + merge.**
```bash
git add -A && git commit -m "Document parity features; pin published coinbasis 0.2 + cryptolytics 0.1"
git checkout main && git merge --ff-only feat/parity-phase-c
```

---

## Self-Review (against the spec)
- **C1 tax:** dep bump + config migration (T1), bracketed view (T2). ✅
- **C2 csv:** `import_csv` (T3), `--import` + sample (T4). ✅
- **C3 analytics:** key/plan config (T5), `fetch_history` + cryptolytics dep (T6), `FetchMsg` channel + triggers + `history_days` (T7), recompute analytics + `target_strategy` + key `w` (T8), Rebalance panels (T9). ✅
- **C4 history:** snapshot JSONL/cost/pl + `reconstruct_series` (T10), Performance chart/playback + key `p` (T11). ✅
- **Finalize:** help/README/pins/merge (T12). ✅
- **Placeholders:** none; the A+B publish gate is external (plan header).
- **Type consistency:** `HistoryData`, `FetchMsg`, `Plan`/`CoinGeckoConfig`, `Derived` analytics fields, `target_strategy`/`PerfMode`, and the cryptolytics call signatures (`correlation_matrix`, `portfolio_volatility`, `target_weights`, `buy_and_hold_return`, `annualize(_,365.0)`) match Part B and the spec. `reconstruct_series`/`Snapshot{date,total_value,cost,pl}` consistent across T10–T11.
- **Executor notes:** land Tasks 7–8 together (the `set_price_history`/`price_history` field they share); confirm the `coingecko` `coin_market_chart` signature when implementing T6; do the dev `[patch.crates-io]` dance only if A/B aren't published yet, and remove it in T12.
