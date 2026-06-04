# Part B — `cryptolytics` 0.1.0: Portfolio Risk & Allocation Analytics Crate — Design Spec

**Author:** Jacob Kanfer · **Date:** 2026-06-03 · **Status:** Approved (design); pending spec review

Part B of the 3-part parity effort. A **new, standalone, publishable** Rust crate at `~/CodeRepos/cryptolytics`, built to the same first-class standard as coinbasis (proper metadata, `#![forbid(unsafe_code)]` + `#![deny(missing_docs)]`, a runnable doc-example on every public fn, `examples/`, integration **and** `proptest` property tests, README, published 0.1.0). It is the analytics engine behind V2's Rebalance and Performance views (Part C), and useful to any Rust portfolio/quant tool.

## 1. Scope & boundary
Pure, no-network, no-IO **portfolio risk & allocation math**, operating on `f64` return/price series. Self-contained — it restates the single-series basics rather than depending on `coinbasis::stats`, so it stands alone for the community. `coinbasis` = accounting (positions/basis/realized); `cryptolytics` = behavior (risk/allocation/backtest).

**Decisions (locked):** `f64` math (matches the Python original and `coinbasis::stats`; these are estimates, not ledger money). Self-contained (intentional, minor overlap with `coinbasis::stats`). `BTreeMap` for deterministic multi-asset ordering. `serde` is an optional feature for the public output/enum types.

**Non-goals:** no cost-basis/tax (that's coinbasis); no network/price fetching (the app supplies series); no charting.

## 2. Crate metadata (`Cargo.toml`)
`name = "cryptolytics"`, `version = "0.1.0"`, `edition = "2021"`, `rust-version = "1.74"`, `license = "MIT OR Apache-2.0"`, description = "Pure portfolio risk & allocation analytics: returns, volatility, correlation, Sharpe, drawdown, rebalancing, and backtesting.", `keywords = ["portfolio","finance","risk","analytics","rebalancing"]`, `categories = ["finance"]`, `repository`, `readme`, `[package.metadata.docs.rs] all-features = true`.
deps: `thiserror = "1"`; `serde = { version = "1", features = ["derive"], optional = true }`. dev-deps: `proptest = "1"`. `[features] default = []; serde = ["dep:serde"]`. `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`.

## 3. Public API (by module)

### `returns`
- `daily_returns(prices: &[f64]) -> Vec<f64>` — `p[i]/p[i-1] - 1`; `[]` if `< 2` prices.

### `volatility`
- `volatility(returns: &[f64]) -> Option<f64>` — population stdev; `None` if `< 2`.
- `annualize(daily_vol: f64, periods_per_year: f64) -> f64` — `daily_vol * periods_per_year.sqrt()` (caller passes `365.0`).

### `correlation`
- `correlation(a: &[f64], b: &[f64]) -> Option<f64>` — Pearson over the overlapping prefix (`min` length); `None` if `< 2` overlap or either series has zero variance. (Named `correlation` to match the Python `cryptolytics` parity package — see §8.)
- `correlation_matrix(returns_by_asset: &BTreeMap<String, Vec<f64>>) -> BTreeMap<(String, String), f64>` — all ordered pairs; diagonal `1.0`; off-diagonal via `correlation` (missing → `0.0`).

### `portfolio`
- `portfolio_volatility(weights: &BTreeMap<String, f64>, vols: &BTreeMap<String, f64>, corr: &BTreeMap<(String, String), f64>) -> f64` — `sqrt(Σ_i Σ_j w_i w_j σ_i σ_j ρ_ij)`; missing `ρ` defaults `1.0` if `i==j` else `0.0`; variance `≤ 0` → `0.0`.

### `ratios`
- `sharpe_ratio(returns: &[f64], risk_free: f64) -> Option<f64>` — `(mean(returns) - risk_free) / volatility(returns)`; `None` if `< 2` or vol `0`.
- `max_drawdown(values: &[f64]) -> Option<f64>` — worst peak-to-trough decline as a fraction `0.0..=1.0`; `None` if `< 2`.
- `cumulative_return(values: &[f64]) -> Option<f64>` — `last/first - 1`; `None` if `< 2` or `first == 0`.

### `allocation`
- `enum TargetStrategy { Equal, MarketCap, Custom }` (Copy, serde).
- `target_weights(strategy: TargetStrategy, assets: &[String], market_caps: Option<&BTreeMap<String, f64>>, custom: Option<&BTreeMap<String, f64>>) -> Result<BTreeMap<String, f64>, AllocError>`:
  - `Equal` → `1/N` each (`Err(EmptyAssets)` if none).
  - `MarketCap` → `cap_i / Σcap` over assets with positive cap; drop missing/zero and renormalize; `Err(NoMarketCapData)` if none usable.
  - `Custom` → the provided map (`Err(MissingCustomWeights)` if `None`/empty; `Err(WeightsNotNormalized(sum))` if `|Σ−1| > 1e-6`).
- `enum TradeAction { Buy, Sell, Hold }` (Copy, serde).
- `struct RebalanceTrade { asset: String, action: TradeAction, delta_usd: f64, amount: f64, target_pct: f64 }` (serde).
- `compute_trades(current_values: &BTreeMap<String, f64>, target_weights: &BTreeMap<String, f64>, prices: &BTreeMap<String, f64>) -> Vec<RebalanceTrade>` — over the union of held + targeted assets; `target_value = weight × total`; `delta = target − current`; `action` by `±1e-9` threshold; `amount = delta/price` (`0.0` if no price); `target_pct = weight × 100`.

### `backtest`
- `buy_and_hold_return(history_by_asset: &BTreeMap<String, Vec<f64>>, weights: &BTreeMap<String, f64>) -> f64` — over assets with `≥ 2` prices and `first > 0`; renormalize weights across usable assets; `Σ w_norm × (last/first − 1)`; `0.0` if none usable.

### `error`
- `enum AllocError { EmptyAssets, NoMarketCapData, MissingCustomWeights, WeightsNotNormalized(f64) }` (`thiserror`, `Display`).

### `lib.rs`
Module docs + re-exports of the key public types: `pub use error::AllocError;` and `pub use allocation::{RebalanceTrade, TargetStrategy, TradeAction};`. Functions stay module-qualified (e.g. `cryptolytics::volatility::volatility`, `cryptolytics::allocation::target_weights`), matching coinbasis's re-export conventions (types re-exported at the root, functions called via their module).

## 4. Examples (`examples/`)
- `quickstart.rs` — returns → volatility → annualize → sharpe.
- `risk_metrics.rs` — correlation matrix + portfolio volatility over 3 assets.
- `rebalancing.rs` — `target_weights(MarketCap, …)` + `compute_trades(…)`.
- `backtest.rs` — `buy_and_hold_return` current vs target.

## 5. Tests
- Inline `#[cfg(test)]` per module: empty/short-series edges, known-value checks (e.g. returns of `[100,110,99]`), zero-variance correlation → `None`, portfolio vol of a single asset = its vol, equal/marketcap/custom weights, custom not-normalized error, trade buy/sell/hold + missing-price amount 0, backtest renormalization + no-usable → 0.
- `tests/headline.rs` — an end-to-end scenario stitching several functions.
- `tests/properties.rs` (`proptest`) — invariants: `volatility ≥ 0`; `pearson ∈ [-1, 1]`; `target_weights` sums to `1.0` (±1e-9) for equal/marketcap; `compute_trades` deltas sum to ≈ 0 when prices present.
- `tests/serde_roundtrip.rs` (feature `serde`) — `TargetStrategy`, `TradeAction`, `RebalanceTrade` round-trip.

## 6. Release
Gates: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, `cargo doc --no-deps --all-features`, `cargo publish --dry-run`. **Jacob runs the real `cargo publish`** for `cryptolytics 0.1.0` before Part C's analytics tasks. Author rule + no AI attribution as everywhere. New git repo initialized at `~/CodeRepos/cryptolytics` (LICENSE-MIT, LICENSE-APACHE, README, `.gitignore` for `/target`).

## 7. Risks
- **Overlap with `coinbasis::stats`:** intentional; the crates serve different audiences. Documented in the README.
- **f64/Decimal boundary:** Part C converts V2's `Decimal` values to `f64` at the cryptolytics call boundary and back for display — specified in Part C.
- **Publish gating:** Part C analytics can't compile until 0.1.0 is published (same `[patch.crates-io]` fallback available).

## 8. Cross-ecosystem parity (vs the Python `cryptolytics`)
Validated 2026-06-03 against the Python parity program (`~/.claude/tmp/crypto-python-parity/`).
- **Repo path:** Rust owns `~/CodeRepos/cryptolytics`. The Python parity package must use **`~/CodeRepos/cryptolytics-py`** (mirroring the `coinbasis`/`coinbasis-py` convention) — the master arch currently points both at `~/CodeRepos/cryptolytics`, a directory collision to fix on the Python side. PyPI and crates.io names can both be `cryptolytics` (different registries).
- **Shared analytics names match:** `correlation` (not `pearson`), `correlation_matrix`, `portfolio_volatility`, `sharpe_ratio`, `max_drawdown`, `cumulative_return`, `daily_returns`, `volatility`, `annualize`, `target_weights`, `compute_trades`, `buy_and_hold_return` line up name-for-name with the Python package. (Rust `annualize` takes `periods_per_year` explicitly where Python hardcodes 365 — Rust is the superset; V2 callers pass `365.0`.)
- **Intentional scope asymmetry (per master §1):** the Python `cryptolytics` is broad ("analytics + ALL networking" — it depends on Python-`coinbasis` and folds in the CoinGecko client, DefiLlama, RSS, **tax-aware rebalance**, perf, history, staking, news). The Rust `cryptolytics` is deliberately **pure & standalone** (no `coinbasis` dep, no network): the CoinGecko client is the external `coingecko` crate, and tax-aware rebalance / perf / history reconstruction live in the **V2 app** (which depends on both crates). All capabilities exist in the Rust bucket — the package boundary just differs, exactly as the master §1 table maps it.
- **Recommendation for the Python side:** move `correlation` / `correlation_matrix` / `portfolio_volatility` out of Python `coinbasis.stats` and into Python `cryptolytics`, so BOTH ecosystems keep `coinbasis(.stats)` = single-series only and `cryptolytics` = multi-asset correlation/portfolio/allocation/backtest.
