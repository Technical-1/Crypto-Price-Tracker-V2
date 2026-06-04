# Part B — `cryptolytics` 0.1.0 Crate — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Build and publish `cryptolytics` 0.1.0 — a pure, standalone, no-network Rust crate of portfolio risk & allocation analytics (`f64`), built to coinbasis's first-class standard (docs, examples, integration + property tests).

**Architecture:** Module-per-concern (`returns`, `volatility`, `correlation`, `portfolio`, `ratios`, `allocation`, `backtest`, `error`). Pure functions over `f64`/`BTreeMap`. `#![forbid(unsafe_code)]` + `#![deny(missing_docs)]`. Optional `serde` feature for the public enum/struct types.

**Tech Stack:** Rust 2021, `thiserror`, optional `serde`; dev: `proptest`.

**Repo:** NEW at `/Users/jacobkanfer/CodeRepos/cryptolytics` (Rust owns this path; the Python parity package uses `cryptolytics-py`).

**Author rule (hook-enforced):** author `51518860+Technical-1@users.noreply.github.com`; NO Co-Authored-By / AI attribution; never `--no-verify`.

**Spec:** `~/CodeRepos/Crypto-Price-Tracker-V2/docs/superpowers/specs/2026-06-03-partB-cryptolytics-design.md`.

**Conventions (mirror coinbasis):** types re-exported at the crate root; functions called module-qualified (`cryptolytics::volatility::volatility`). Doc-example on every public item (`#![deny(missing_docs)]`). `volatility` uses **population** stdev (matches V1's `pstdev`). Float test asserts use an epsilon helper (no extra dep). All commits on `main` in this fresh repo (no feature branch needed for a greenfield repo, but verify author first).

---

## Task 1: Scaffold the crate

**Files:** Create `Cargo.toml`, `src/lib.rs`, `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`, `.gitignore`.

- [ ] **Step 1: Init repo + dirs.**
```bash
mkdir -p /Users/jacobkanfer/CodeRepos/cryptolytics/src && cd /Users/jacobkanfer/CodeRepos/cryptolytics
git init -q
git config user.email   # confirm 51518860+Technical-1@users.noreply.github.com (set it if not)
printf '/target\nCargo.lock\n' > .gitignore   # library crate: Cargo.lock not committed
```
(Copy `LICENSE-MIT` and `LICENSE-APACHE` from `~/CodeRepos/coinbasis/` so the dual license matches.)

- [ ] **Step 2: `Cargo.toml`:**
```toml
[package]
name = "cryptolytics"
version = "0.1.0"
edition = "2021"
rust-version = "1.74"
description = "Pure portfolio risk & allocation analytics: returns, volatility, correlation, Sharpe, drawdown, rebalancing, and backtesting."
license = "MIT OR Apache-2.0"
repository = "https://github.com/Technical-1/cryptolytics"
readme = "README.md"
keywords = ["portfolio", "finance", "risk", "analytics", "rebalancing"]
categories = ["finance"]

[dependencies]
thiserror = "1"
serde = { version = "1", features = ["derive"], optional = true }

[dev-dependencies]
proptest = "1"

[features]
default = []
serde = ["dep:serde"]

[package.metadata.docs.rs]
all-features = true
```

- [ ] **Step 3: `src/lib.rs`** (modules added as each task lands; start with all declared + the two not-yet-created as empty files):
```rust
//! `cryptolytics` — pure portfolio risk & allocation analytics.
//!
//! No network, no I/O: callers supply return/price series as `f64`. Companion to
//! `coinbasis` (which does cost-basis/tax accounting); this crate does portfolio
//! *behavior* — volatility, correlation, Sharpe, drawdown, allocation, and
//! backtesting. Not financial advice.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod allocation;
pub mod backtest;
pub mod correlation;
pub mod error;
pub mod portfolio;
pub mod ratios;
pub mod returns;
pub mod volatility;

pub use allocation::{RebalanceTrade, TargetStrategy, TradeAction};
pub use error::AllocError;
```

- [ ] **Step 4: Create empty module files** so it compiles: each of `src/{returns,volatility,correlation,portfolio,ratios,allocation,backtest,error}.rs` with a single module doc line `//! <name>. Implemented in a later task.` — EXCEPT this leaves `pub use` lines referencing not-yet-defined items. To keep Step 4 compiling, implement `error.rs` and the `allocation.rs` enums/struct in Task 7 BEFORE the re-exports resolve. **Simplest ordering:** comment out the two `pub use` lines in `lib.rs` now and uncomment them in Task 7. Do that.

- [ ] **Step 5: Verify + commit.** `cargo build` → compiles (empty modules; re-exports commented). 
```bash
git add -A && git commit -m "Scaffold cryptolytics crate"
```

---

## Task 2: `returns`

**Files:** `src/returns.rs`.

- [ ] **Step 1: Failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }
    #[test]
    fn computes_period_returns() {
        let r = daily_returns(&[100.0, 110.0, 99.0]);
        assert_eq!(r.len(), 2);
        assert!(close(r[0], 0.10));
        assert!(close(r[1], 99.0 / 110.0 - 1.0));
    }
    #[test]
    fn short_series_is_empty() {
        assert!(daily_returns(&[100.0]).is_empty());
        assert!(daily_returns(&[]).is_empty());
    }
}
```
- [ ] **Step 2: Run** `cargo test returns` → FAIL.
- [ ] **Step 3: Implement** (top of `src/returns.rs`):
```rust
//! Period-over-period returns.

/// Period returns `p[i]/p[i-1] - 1`. Empty if fewer than 2 prices.
///
/// # Example
/// ```
/// let r = cryptolytics::returns::daily_returns(&[100.0, 110.0]);
/// assert!((r[0] - 0.10).abs() < 1e-9);
/// ```
pub fn daily_returns(prices: &[f64]) -> Vec<f64> {
    if prices.len() < 2 {
        return Vec::new();
    }
    prices.windows(2).map(|w| w[1] / w[0] - 1.0).collect()
}
```
- [ ] **Step 4: Run** `cargo test returns` → PASS.
- [ ] **Step 5: Lint + commit.** `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`.
```bash
git add src/returns.rs && git commit -m "Add returns::daily_returns"
```

---

## Task 3: `volatility`

**Files:** `src/volatility.rs`.

- [ ] **Step 1: Failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn population_stdev_and_none_for_short() {
        assert!(volatility(&[1.0]).is_none());
        let v = volatility(&[0.0, 0.0, 0.0]).unwrap();
        assert!(v.abs() < 1e-12);
        // returns [0.1, -0.1]: mean 0, pstdev = 0.1
        let v2 = volatility(&[0.1, -0.1]).unwrap();
        assert!((v2 - 0.1).abs() < 1e-9);
    }
    #[test]
    fn annualize_scales_by_sqrt() {
        assert!((annualize(0.02, 365.0) - 0.02 * 365f64.sqrt()).abs() < 1e-9);
    }
}
```
- [ ] **Step 2: Run** `cargo test volatility` → FAIL.
- [ ] **Step 3: Implement:**
```rust
//! Volatility (population standard deviation) and annualization.

/// Population standard deviation of a returns series. `None` if fewer than 2.
///
/// # Example
/// ```
/// assert!((cryptolytics::volatility::volatility(&[0.1, -0.1]).unwrap() - 0.1).abs() < 1e-9);
/// ```
pub fn volatility(returns: &[f64]) -> Option<f64> {
    if returns.len() < 2 {
        return None;
    }
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
    Some(var.sqrt())
}

/// Annualize a per-period volatility: `daily_vol * sqrt(periods_per_year)`.
///
/// # Example
/// ```
/// let a = cryptolytics::volatility::annualize(0.02, 365.0);
/// assert!(a > 0.0);
/// ```
pub fn annualize(daily_vol: f64, periods_per_year: f64) -> f64 {
    daily_vol * periods_per_year.sqrt()
}
```
- [ ] **Step 4: Run** → PASS. **Step 5: Lint + commit** `git add src/volatility.rs && git commit -m "Add volatility::{volatility, annualize}"`.

---

## Task 4: `correlation`

**Files:** `src/correlation.rs`.

- [ ] **Step 1: Failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn perfect_and_inverse_and_zero_variance() {
        assert!((correlation(&[1.0,2.0,3.0], &[2.0,4.0,6.0]).unwrap() - 1.0).abs() < 1e-9);
        assert!((correlation(&[1.0,2.0,3.0], &[3.0,2.0,1.0]).unwrap() + 1.0).abs() < 1e-9);
        assert!(correlation(&[1.0,1.0,1.0], &[1.0,2.0,3.0]).is_none()); // zero variance
        assert!(correlation(&[1.0], &[1.0]).is_none());
    }
    #[test]
    fn matrix_has_unit_diagonal() {
        let mut m = BTreeMap::new();
        m.insert("a".to_string(), vec![1.0,2.0,3.0]);
        m.insert("b".to_string(), vec![3.0,2.0,1.0]);
        let c = correlation_matrix(&m);
        assert_eq!(c[&("a".into(),"a".into())], 1.0);
        assert!((c[&("a".into(),"b".into())] + 1.0).abs() < 1e-9);
    }
}
```
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement:**
```rust
//! Pearson correlation and the all-pairs correlation matrix.

use std::collections::BTreeMap;

/// Pearson correlation over the overlapping prefix of two series. `None` if
/// fewer than 2 overlapping points or either series has zero variance.
///
/// # Example
/// ```
/// let c = cryptolytics::correlation::correlation(&[1.0,2.0,3.0], &[2.0,4.0,6.0]).unwrap();
/// assert!((c - 1.0).abs() < 1e-9);
/// ```
pub fn correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let ma = a[..n].iter().sum::<f64>() / nf;
    let mb = b[..n].iter().sum::<f64>() / nf;
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (da, db) = (a[i] - ma, b[i] - mb);
        cov += da * db;
        va += da * da;
        vb += db * db;
    }
    if va == 0.0 || vb == 0.0 {
        return None;
    }
    Some(cov / (va.sqrt() * vb.sqrt()))
}

/// All ordered-pair correlations across assets. Diagonal is `1.0`; missing/edge
/// pairs are `0.0`.
///
/// # Example
/// ```
/// use std::collections::BTreeMap;
/// let mut m = BTreeMap::new();
/// m.insert("a".to_string(), vec![1.0, 2.0, 3.0]);
/// let c = cryptolytics::correlation::correlation_matrix(&m);
/// assert_eq!(c[&("a".to_string(), "a".to_string())], 1.0);
/// ```
pub fn correlation_matrix(returns_by_asset: &BTreeMap<String, Vec<f64>>) -> BTreeMap<(String, String), f64> {
    let mut out = BTreeMap::new();
    let keys: Vec<&String> = returns_by_asset.keys().collect();
    for a in &keys {
        for b in &keys {
            let v = if a == b {
                1.0
            } else {
                correlation(&returns_by_asset[*a], &returns_by_asset[*b]).unwrap_or(0.0)
            };
            out.insert(((*a).clone(), (*b).clone()), v);
        }
    }
    out
}
```
- [ ] **Step 4: Run** → PASS. **Step 5: Lint + commit** `git add src/correlation.rs && git commit -m "Add correlation::{correlation, correlation_matrix}"`.

---

## Task 5: `portfolio`

**Files:** `src/portfolio.rs`.

- [ ] **Step 1: Failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn map(p: &[(&str, f64)]) -> BTreeMap<String, f64> { p.iter().map(|(k,v)|(k.to_string(),*v)).collect() }
    #[test]
    fn single_asset_equals_its_vol() {
        let w = map(&[("a", 1.0)]); let v = map(&[("a", 0.2)]);
        let mut c = BTreeMap::new(); c.insert(("a".into(),"a".into()), 1.0);
        assert!((portfolio_volatility(&w, &v, &c) - 0.2).abs() < 1e-9);
    }
    #[test]
    fn uncorrelated_two_asset_quadrature() {
        let w = map(&[("a", 0.5), ("b", 0.5)]); let v = map(&[("a", 0.2), ("b", 0.2)]);
        let mut c = BTreeMap::new(); // off-diagonal missing => 0.0; diagonal 1.0 via i==j
        c.insert(("a".into(),"a".into()),1.0); c.insert(("b".into(),"b".into()),1.0);
        let expected = (0.5f64.powi(2)*0.2*0.2 + 0.5f64.powi(2)*0.2*0.2).sqrt();
        assert!((portfolio_volatility(&w, &v, &c) - expected).abs() < 1e-9);
    }
}
```
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement:**
```rust
//! Portfolio volatility from weights, per-asset volatilities, and correlations.

use std::collections::BTreeMap;

/// Portfolio volatility `sqrt(Σ_i Σ_j w_i w_j σ_i σ_j ρ_ij)`. Missing `ρ`
/// defaults to `1.0` when `i == j`, else `0.0`. Returns `0.0` if variance ≤ 0.
///
/// # Example
/// ```
/// use std::collections::BTreeMap;
/// let w: BTreeMap<String,f64> = [("a".to_string(),1.0)].into_iter().collect();
/// let v: BTreeMap<String,f64> = [("a".to_string(),0.2)].into_iter().collect();
/// let mut c = BTreeMap::new(); c.insert(("a".to_string(),"a".to_string()), 1.0);
/// assert!((cryptolytics::portfolio::portfolio_volatility(&w,&v,&c) - 0.2).abs() < 1e-9);
/// ```
pub fn portfolio_volatility(
    weights: &BTreeMap<String, f64>,
    vols: &BTreeMap<String, f64>,
    corr: &BTreeMap<(String, String), f64>,
) -> f64 {
    let mut var = 0.0;
    for (i, wi) in weights {
        for (j, wj) in weights {
            let si = vols.get(i).copied().unwrap_or(0.0);
            let sj = vols.get(j).copied().unwrap_or(0.0);
            let rho = if i == j {
                1.0
            } else {
                corr.get(&(i.clone(), j.clone())).copied().unwrap_or(0.0)
            };
            var += wi * wj * si * sj * rho;
        }
    }
    if var <= 0.0 {
        0.0
    } else {
        var.sqrt()
    }
}
```
- [ ] **Step 4: Run** → PASS. **Step 5: Lint + commit** `git add src/portfolio.rs && git commit -m "Add portfolio::portfolio_volatility"`.

---

## Task 6: `ratios`

**Files:** `src/ratios.rs`.

- [ ] **Step 1: Failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sharpe_drawdown_cumulative() {
        assert!(sharpe_ratio(&[1.0], 0.0).is_none());
        let s = sharpe_ratio(&[0.1, -0.1], 0.0).unwrap(); // mean 0 => 0
        assert!(s.abs() < 1e-9);
        let md = max_drawdown(&[100.0, 120.0, 90.0, 110.0]).unwrap(); // (120-90)/120 = 0.25
        assert!((md - 0.25).abs() < 1e-9);
        assert!(max_drawdown(&[100.0]).is_none());
        assert!((cumulative_return(&[100.0, 150.0]).unwrap() - 0.5).abs() < 1e-9);
        assert!(cumulative_return(&[0.0, 1.0]).is_none()); // first 0
    }
}
```
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement:**
```rust
//! Sharpe ratio, max drawdown, and cumulative return.

use crate::volatility::volatility;

/// `(mean(returns) - risk_free) / volatility(returns)`. `None` if < 2 or vol 0.
///
/// # Example
/// ```
/// assert!(cryptolytics::ratios::sharpe_ratio(&[0.1, -0.1], 0.0).unwrap().abs() < 1e-9);
/// ```
pub fn sharpe_ratio(returns: &[f64], risk_free: f64) -> Option<f64> {
    if returns.len() < 2 {
        return None;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let vol = volatility(returns)?;
    if vol == 0.0 {
        return None;
    }
    Some((mean - risk_free) / vol)
}

/// Worst peak-to-trough decline as a fraction `0.0..=1.0`. `None` if < 2 values.
///
/// # Example
/// ```
/// let md = cryptolytics::ratios::max_drawdown(&[100.0, 120.0, 90.0]).unwrap();
/// assert!((md - 0.25).abs() < 1e-9);
/// ```
pub fn max_drawdown(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mut peak = values[0];
    let mut mdd = 0.0;
    for &v in values {
        if v > peak {
            peak = v;
        }
        let dd = if peak != 0.0 { (peak - v) / peak } else { 0.0 };
        if dd > mdd {
            mdd = dd;
        }
    }
    Some(mdd)
}

/// Total return `last/first - 1`. `None` if < 2 values or `first == 0`.
///
/// # Example
/// ```
/// assert!((cryptolytics::ratios::cumulative_return(&[100.0, 150.0]).unwrap() - 0.5).abs() < 1e-9);
/// ```
pub fn cumulative_return(values: &[f64]) -> Option<f64> {
    if values.len() < 2 || values[0] == 0.0 {
        return None;
    }
    Some(values[values.len() - 1] / values[0] - 1.0)
}
```
- [ ] **Step 4: Run** → PASS. **Step 5: Lint + commit** `git add src/ratios.rs && git commit -m "Add ratios::{sharpe_ratio, max_drawdown, cumulative_return}"`.

---

## Task 7: `error` + `allocation` (+ re-exports)

**Files:** `src/error.rs`, `src/allocation.rs`, `src/lib.rs` (uncomment re-exports).

- [ ] **Step 1: Failing test** (in `src/allocation.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn map(p: &[(&str, f64)]) -> BTreeMap<String, f64> { p.iter().map(|(k,v)|(k.to_string(),*v)).collect() }
    fn assets(a: &[&str]) -> Vec<String> { a.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn equal_weights() {
        let w = target_weights(TargetStrategy::Equal, &assets(&["a","b"]), None, None).unwrap();
        assert!((w["a"] - 0.5).abs() < 1e-9 && (w["b"] - 0.5).abs() < 1e-9);
        assert_eq!(target_weights(TargetStrategy::Equal, &[], None, None), Err(AllocError::EmptyAssets));
    }
    #[test]
    fn marketcap_drops_zero_and_renormalizes() {
        let caps = map(&[("a", 30.0), ("b", 10.0), ("c", 0.0)]);
        let w = target_weights(TargetStrategy::MarketCap, &assets(&["a","b","c"]), Some(&caps), None).unwrap();
        assert!((w["a"] - 0.75).abs() < 1e-9 && (w["b"] - 0.25).abs() < 1e-9);
        assert!(!w.contains_key("c"));
    }
    #[test]
    fn custom_validates_sum() {
        let good = map(&[("a", 0.6), ("b", 0.4)]);
        assert!(target_weights(TargetStrategy::Custom, &assets(&["a","b"]), None, Some(&good)).is_ok());
        let bad = map(&[("a", 0.6), ("b", 0.6)]);
        assert!(matches!(target_weights(TargetStrategy::Custom, &assets(&["a","b"]), None, Some(&bad)),
            Err(AllocError::WeightsNotNormalized(_))));
        assert_eq!(target_weights(TargetStrategy::Custom, &assets(&["a"]), None, None), Err(AllocError::MissingCustomWeights));
    }
    #[test]
    fn trades_buy_sell_hold_and_missing_price() {
        // total 100; target a=100% => buy 40 of a; sell all 30 of b; c held 30 -> target 0 sell
        let current = map(&[("a", 30.0), ("b", 30.0), ("c", 40.0)]);
        let target = map(&[("a", 1.0)]);
        let prices = map(&[("a", 2.0), ("b", 10.0)]); // c has no price
        let trades = compute_trades(&current, &target, &prices);
        let a = trades.iter().find(|t| t.asset == "a").unwrap();
        assert_eq!(a.action, TradeAction::Buy);
        assert!((a.delta_usd - 70.0).abs() < 1e-9); // 100 - 30
        assert!((a.amount - 35.0).abs() < 1e-9);     // 70 / 2
        let c = trades.iter().find(|t| t.asset == "c").unwrap();
        assert_eq!(c.action, TradeAction::Sell);
        assert_eq!(c.amount, 0.0); // no price
    }
}
```
- [ ] **Step 2: Run** `cargo test allocation` → FAIL (undefined).
- [ ] **Step 3: Implement `src/error.rs`:**
```rust
//! Allocation/rebalancing errors.

/// Errors from [`crate::allocation::target_weights`].
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AllocError {
    /// No assets supplied for an equal-weight allocation.
    #[error("no assets to weight")]
    EmptyAssets,
    /// No usable (positive) market-cap data for a market-cap allocation.
    #[error("no usable market-cap data")]
    NoMarketCapData,
    /// Custom strategy chosen but no custom weights provided.
    #[error("custom strategy requires target weights")]
    MissingCustomWeights,
    /// Custom weights do not sum to 1.0 (±1e-6); contains the actual sum.
    #[error("target weights must sum to 1.0, got {0}")]
    WeightsNotNormalized(f64),
}
```
- [ ] **Step 4: Implement `src/allocation.rs`** (above the test module):
```rust
//! Target-weight strategies and rebalancing-trade computation.

use std::collections::BTreeMap;

use crate::error::AllocError;

/// How target weights are derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TargetStrategy {
    /// Equal weight across all assets.
    Equal,
    /// Weight proportional to market cap.
    MarketCap,
    /// Caller-supplied weights.
    Custom,
}

/// Direction of a suggested trade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TradeAction {
    /// Increase the position.
    Buy,
    /// Decrease the position.
    Sell,
    /// No change.
    Hold,
}

/// A suggested rebalancing trade for one asset.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RebalanceTrade {
    /// Asset.
    pub asset: String,
    /// Buy / Sell / Hold.
    pub action: TradeAction,
    /// `target_value - current_value` (USD).
    pub delta_usd: f64,
    /// `delta_usd / price` (0.0 if no price).
    pub amount: f64,
    /// Target weight as a percent.
    pub target_pct: f64,
}

/// Compute target weights under a [`TargetStrategy`].
///
/// # Example
/// ```
/// use cryptolytics::allocation::{target_weights, TargetStrategy};
/// let w = target_weights(TargetStrategy::Equal, &["a".into(), "b".into()], None, None).unwrap();
/// assert!((w["a"] - 0.5).abs() < 1e-9);
/// ```
pub fn target_weights(
    strategy: TargetStrategy,
    assets: &[String],
    market_caps: Option<&BTreeMap<String, f64>>,
    custom: Option<&BTreeMap<String, f64>>,
) -> Result<BTreeMap<String, f64>, AllocError> {
    match strategy {
        TargetStrategy::Equal => {
            if assets.is_empty() {
                return Err(AllocError::EmptyAssets);
            }
            let w = 1.0 / assets.len() as f64;
            Ok(assets.iter().map(|a| (a.clone(), w)).collect())
        }
        TargetStrategy::MarketCap => {
            let caps = market_caps.ok_or(AllocError::NoMarketCapData)?;
            let usable: Vec<(String, f64)> = assets
                .iter()
                .filter_map(|a| caps.get(a).copied().filter(|c| *c > 0.0).map(|c| (a.clone(), c)))
                .collect();
            let total: f64 = usable.iter().map(|(_, c)| c).sum();
            if total <= 0.0 {
                return Err(AllocError::NoMarketCapData);
            }
            Ok(usable.into_iter().map(|(a, c)| (a, c / total)).collect())
        }
        TargetStrategy::Custom => {
            let c = custom.ok_or(AllocError::MissingCustomWeights)?;
            if c.is_empty() {
                return Err(AllocError::MissingCustomWeights);
            }
            let sum: f64 = c.values().sum();
            if (sum - 1.0).abs() > 1e-6 {
                return Err(AllocError::WeightsNotNormalized(sum));
            }
            Ok(c.clone())
        }
    }
}

/// Compute rebalancing trades from current USD values toward target weights,
/// preserving total portfolio value. `amount` is `0.0` when no price is known.
///
/// # Example
/// ```
/// use std::collections::BTreeMap;
/// use cryptolytics::allocation::compute_trades;
/// let cur: BTreeMap<String,f64> = [("a".into(), 100.0)].into_iter().collect();
/// let tgt: BTreeMap<String,f64> = [("a".into(), 1.0)].into_iter().collect();
/// let px:  BTreeMap<String,f64> = [("a".into(), 2.0)].into_iter().collect();
/// assert!(compute_trades(&cur, &tgt, &px).iter().all(|t| t.asset == "a"));
/// ```
pub fn compute_trades(
    current_values: &BTreeMap<String, f64>,
    target_weights: &BTreeMap<String, f64>,
    prices: &BTreeMap<String, f64>,
) -> Vec<RebalanceTrade> {
    let total: f64 = current_values.values().sum();
    let mut keys: Vec<String> = current_values.keys().cloned().collect();
    for k in target_weights.keys() {
        if !current_values.contains_key(k) {
            keys.push(k.clone());
        }
    }
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|asset| {
            let weight = target_weights.get(&asset).copied().unwrap_or(0.0);
            let target_value = weight * total;
            let current = current_values.get(&asset).copied().unwrap_or(0.0);
            let delta = target_value - current;
            let action = if delta > 1e-9 {
                TradeAction::Buy
            } else if delta < -1e-9 {
                TradeAction::Sell
            } else {
                TradeAction::Hold
            };
            let amount = prices
                .get(&asset)
                .copied()
                .filter(|p| *p > 0.0)
                .map(|p| delta / p)
                .unwrap_or(0.0);
            RebalanceTrade { asset, action, delta_usd: delta, amount, target_pct: weight * 100.0 }
        })
        .collect()
}
```
- [ ] **Step 5: Uncomment the `pub use` lines in `src/lib.rs`** (the `error::AllocError` and `allocation::{RebalanceTrade, TargetStrategy, TradeAction}` re-exports).
- [ ] **Step 6: Run + lint + commit.** `cargo test allocation` → PASS; `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`.
```bash
git add src/error.rs src/allocation.rs src/lib.rs && git commit -m "Add allocation strategies, rebalancing trades, and AllocError"
```

---

## Task 8: `backtest`

**Files:** `src/backtest.rs`.

- [ ] **Step 1: Failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn weighted_renormalized_return() {
        let mut h = BTreeMap::new();
        h.insert("a".to_string(), vec![100.0, 110.0]); // +10%
        h.insert("b".to_string(), vec![100.0, 130.0]); // +30%
        h.insert("c".to_string(), vec![100.0]);        // unusable (1 point)
        let mut w = BTreeMap::new();
        w.insert("a".to_string(), 0.5); w.insert("b".to_string(), 0.5); w.insert("c".to_string(), 0.0);
        // usable a,b weights 0.5/0.5 => 0.5*0.1 + 0.5*0.3 = 0.2
        assert!((buy_and_hold_return(&h, &w) - 0.2).abs() < 1e-9);
    }
    #[test]
    fn no_usable_history_is_zero() {
        let h = BTreeMap::new();
        let w = BTreeMap::new();
        assert_eq!(buy_and_hold_return(&h, &w), 0.0);
    }
}
```
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement:**
```rust
//! Buy-and-hold backtest over historical price series.

use std::collections::BTreeMap;

/// Weighted buy-and-hold return over the window. Considers only assets with
/// `>= 2` prices and `first > 0`; weights are renormalized across usable assets.
/// Returns `0.0` if none are usable.
///
/// # Example
/// ```
/// use std::collections::BTreeMap;
/// let mut h = BTreeMap::new(); h.insert("a".to_string(), vec![100.0, 110.0]);
/// let mut w = BTreeMap::new(); w.insert("a".to_string(), 1.0);
/// assert!((cryptolytics::backtest::buy_and_hold_return(&h, &w) - 0.10).abs() < 1e-9);
/// ```
pub fn buy_and_hold_return(
    history_by_asset: &BTreeMap<String, Vec<f64>>,
    weights: &BTreeMap<String, f64>,
) -> f64 {
    let usable: Vec<(&String, f64)> = history_by_asset
        .iter()
        .filter_map(|(a, h)| {
            if h.len() >= 2 && h[0] > 0.0 {
                weights.get(a).copied().map(|w| (a, w))
            } else {
                None
            }
        })
        .collect();
    let wsum: f64 = usable.iter().map(|(_, w)| w).sum();
    if wsum <= 0.0 {
        return 0.0;
    }
    usable
        .into_iter()
        .map(|(a, w)| {
            let h = &history_by_asset[a];
            (w / wsum) * (h[h.len() - 1] / h[0] - 1.0)
        })
        .sum()
}
```
- [ ] **Step 4: Run** → PASS. **Step 5: Lint + commit** `git add src/backtest.rs && git commit -m "Add backtest::buy_and_hold_return"`.

---

## Task 9: Examples + integration + property + serde tests

**Files:** `examples/{quickstart,risk_metrics,rebalancing,backtest}.rs`, `tests/headline.rs`, `tests/properties.rs`, `tests/serde_roundtrip.rs`.

- [ ] **Step 1: `examples/quickstart.rs`:**
```rust
//! Returns -> volatility -> annualize -> Sharpe. Run: `cargo run --example quickstart`.
use cryptolytics::{returns, volatility, ratios};
fn main() {
    let prices = [100.0, 102.0, 101.0, 105.0, 104.0];
    let r = returns::daily_returns(&prices);
    let dv = volatility::volatility(&r).unwrap();
    println!("daily vol: {dv:.4}  annual: {:.4}", volatility::annualize(dv, 365.0));
    println!("sharpe: {:?}", ratios::sharpe_ratio(&r, 0.0));
}
```
- [ ] **Step 2:** `examples/risk_metrics.rs` (build a 3-asset returns `BTreeMap`, print `correlation::correlation_matrix` + `portfolio::portfolio_volatility`); `examples/rebalancing.rs` (`allocation::target_weights(MarketCap, …)` + `compute_trades`); `examples/backtest.rs` (`backtest::buy_and_hold_return` current vs target). Each `fn main()` prints results, no panic.
- [ ] **Step 3: `tests/headline.rs`** — an end-to-end scenario: from price histories → returns → vols → correlation matrix → portfolio vol → target_weights → compute_trades → buy_and_hold_return, asserting a couple of known values.
- [ ] **Step 4: `tests/properties.rs`** (`proptest`):
```rust
use proptest::prelude::*;
proptest! {
    #[test]
    fn volatility_non_negative(xs in proptest::collection::vec(-1.0f64..1.0, 2..50)) {
        if let Some(v) = cryptolytics::volatility::volatility(&xs) { prop_assert!(v >= 0.0); }
    }
    #[test]
    fn correlation_in_range(a in proptest::collection::vec(-1.0f64..1.0, 2..50)) {
        // correlation with itself is 1.0 (or None if zero-variance)
        if let Some(c) = cryptolytics::correlation::correlation(&a, &a) { prop_assert!((c-1.0).abs() < 1e-6); }
    }
    #[test]
    fn equal_weights_sum_to_one(n in 1usize..20) {
        let assets: Vec<String> = (0..n).map(|i| format!("a{i}")).collect();
        let w = cryptolytics::allocation::target_weights(
            cryptolytics::allocation::TargetStrategy::Equal, &assets, None, None).unwrap();
        let sum: f64 = w.values().sum();
        prop_assert!((sum - 1.0).abs() < 1e-9);
    }
}
```
- [ ] **Step 5: `tests/serde_roundtrip.rs`** (feature `serde`): round-trip `TargetStrategy`, `TradeAction`, and a `RebalanceTrade` via `serde_json` (add `serde_json` to dev-deps).
- [ ] **Step 6: Run + lint + commit.** `cargo test --all-features && cargo run --example quickstart && cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`.
```bash
git add examples/ tests/ Cargo.toml && git commit -m "Add examples and integration/property/serde tests"
```

---

## Task 10: README, docs gate, release prep + PUBLISH GATE

**Files:** `README.md`.

- [ ] **Step 1: README** — what the crate is, the coinbasis-vs-cryptolytics boundary, a quickstart snippet, module overview, "not financial advice", dual license.
- [ ] **Step 2: Docs + full gates.** `cargo doc --no-deps --all-features` (missing-docs gate), `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features` → all green.
- [ ] **Step 3: Package dry-run.** `cargo publish --dry-run` then `cargo package --list` → succeeds; `src/*.rs` + `examples/*` present.
- [ ] **Step 4: Commit.** `git add README.md && git commit -m "Add README and finalize cryptolytics 0.1.0"`.
- [ ] **Step 5: PUBLISH GATE.** Controller stops and asks **Jacob to run `cargo publish`** in `~/CodeRepos/cryptolytics` and confirm `cryptolytics 0.1.0` is live on crates.io before Part C's analytics tasks (C3.2+).

---

## Self-Review (against the spec)
- **Spec coverage:** scaffold/metadata/lints (T1); `returns` (T2), `volatility`+`annualize` (T3), `correlation`+matrix (T4), `portfolio_volatility` (T5), `ratios` ×3 (T6), `allocation` + `AllocError` (T7), `backtest` (T8); examples ×4 + integration + proptest + serde (T9); README + docs gate + publish (T10). ✅
- **Placeholders:** none; publish gate (T10 S5) is a deliberate human action.
- **Type consistency:** `TargetStrategy`/`TradeAction`/`RebalanceTrade`/`AllocError` and every fn signature match the spec and the Part C consumer (`correlation` name, `annualize(_, periods_per_year)`, `compute_trades` field names). 
- **Executor note:** keep the two `pub use` lines in `lib.rs` commented until Task 7 defines `AllocError`/allocation types (T1 S4), then uncomment (T7 S5).
