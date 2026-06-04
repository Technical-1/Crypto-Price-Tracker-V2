# Part A — coinbasis 0.2.0 Tax-Bracket Estimation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add progressive tax-bracket estimation to the published `coinbasis` crate as a new pure `tax` module, and release 0.2.0.

**Architecture:** New `pub mod tax` (`TaxConfig`/`TaxBracket`/`TaxEstimate`/`estimate` + `Portfolio::tax_estimate`), `Decimal` throughout, purely additive — the fixed 365-day `Term` and all existing signatures are untouched; the configurable threshold is applied inside the estimate.

**Tech Stack:** Rust 2021, `rust_decimal`, `chrono`, optional `serde`; `proptest`/`rust_decimal_macros` dev-deps.

**Repo:** `/Users/jacobkanfer/CodeRepos/coinbasis-rs` (published crate, currently 0.1.1).

**Author rule (hook-enforced):** author `51518860+Technical-1@users.noreply.github.com`; NO Co-Authored-By / Claude / AI attribution; never `--no-verify`. Verify `git config user.email` before the first commit.

**Branch:** `feat/tax-brackets`; fast-forward merge to `main` after gates pass.

**Spec:** `~/CodeRepos/Crypto-Price-Tracker-V2/docs/superpowers/specs/2026-06-03-partA-coinbasis-tax-design.md`.

---

## Verified coinbasis facts (pinned)
- `report::Term { Short, Long }` + `Term::classify(acquired_at: DateTime<Utc>, disposed_at: DateTime<Utc>) -> Term` (uses `> Duration::days(365)`).
- `report::RealizedGain { asset, wallet, disposed_at: DateTime<Utc>, acquired_at: Option<DateTime<Utc>>, quantity, proceeds, cost_basis, gain: Decimal, term: Option<Term> }`.
- `report::CapitalGainsReport { tax_year: i32, rows: Vec<RealizedGain>, short_term_gain, long_term_gain, total_gain: Decimal }`.
- `Portfolio::capital_gains_report(method: CostBasisMethod, tax_year: i32) -> Result<CapitalGainsReport, PortfolioError>`.
- `#![deny(missing_docs)]` is on (doc + runnable doc-example per public item). serde optional: `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]`. `rust_decimal_macros::dec!` is dev-only → non-test code uses `Decimal::new(mantissa, scale)`. Inline tests in `#[cfg(test)] mod tests`; integration tests in `tests/`; examples in `examples/`.

---

## Task 1: `tax` module types

**Files:** Create `src/tax.rs`; Modify `src/lib.rs`.

- [ ] **Step 1: Branch + write the failing test.**
```bash
cd /Users/jacobkanfer/CodeRepos/coinbasis-rs && git checkout -b feat/tax-brackets
```
Create `src/tax.rs` containing only this test module first:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn default_config_is_us_preset() {
        let c = TaxConfig::default();
        assert_eq!(c.long_term_threshold_days, 365);
        assert_eq!(c.short_term_rate, dec!(0.35));
        assert_eq!(c.long_term_brackets.len(), 3);
        assert_eq!(c.long_term_brackets[0].rate, dec!(0.0));
        assert_eq!(c.long_term_brackets[2].up_to, None);
        assert_eq!(c.long_term_brackets[2].rate, dec!(0.20));
    }
}
```

- [ ] **Step 2: Run to verify failure.** `cargo test --lib tax` → FAIL (module/types undefined).

- [ ] **Step 3: Add to `src/lib.rs`.** After `pub mod stats;` add `pub mod tax;`. In the re-export block (after the `transaction::` line) add `pub use tax::{TaxBracket, TaxConfig, TaxEstimate};`.

- [ ] **Step 4: Implement the types** at the top of `src/tax.rs`:
```rust
//! Tax-liability estimation over a [`crate::CapitalGainsReport`].
//!
//! Cost-basis accounting (which lots, what gain, short/long term) is the rest of
//! the crate's job; this module turns a year's realized gains into an estimated
//! tax using a [`TaxConfig`] — a flat short-term rate plus progressive
//! long-term brackets, with a configurable holding-period threshold. Not tax advice.

use crate::report::{CapitalGainsReport, Term};
use chrono::Duration;
use rust_decimal::Decimal;

/// One long-term capital-gains bracket. `up_to` is the cumulative-gain ceiling
/// for this bracket; `None` marks the unbounded top bracket.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaxBracket {
    /// Upper gain bound for this bracket; `None` = unbounded.
    pub up_to: Option<Decimal>,
    /// Marginal rate applied within this bracket (e.g. `0.15` = 15%).
    pub rate: Decimal,
}

/// Tax-rate configuration: a flat short-term rate plus progressive long-term
/// brackets, with a configurable long-term holding threshold.
///
/// # Example
/// ```
/// use coinbasis::TaxConfig;
/// let c = TaxConfig::default();
/// assert_eq!(c.long_term_threshold_days, 365);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaxConfig {
    /// Display label for the jurisdiction (does not affect the math).
    pub jurisdiction: String,
    /// Days held strictly above which a gain is long-term.
    pub long_term_threshold_days: i64,
    /// Flat rate applied to net short-term gains.
    pub short_term_rate: Decimal,
    /// Progressive long-term brackets, ascending, unbounded bracket last.
    pub long_term_brackets: Vec<TaxBracket>,
}

impl Default for TaxConfig {
    fn default() -> Self {
        TaxConfig {
            jurisdiction: "default".to_string(),
            long_term_threshold_days: 365,
            short_term_rate: Decimal::new(35, 2),
            long_term_brackets: vec![
                TaxBracket { up_to: Some(Decimal::new(47025, 0)), rate: Decimal::new(0, 0) },
                TaxBracket { up_to: Some(Decimal::new(518900, 0)), rate: Decimal::new(15, 2) },
                TaxBracket { up_to: None, rate: Decimal::new(20, 2) },
            ],
        }
    }
}

/// The estimated tax from applying a [`TaxConfig`] to a year's gains.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaxEstimate {
    /// Net short-term gain (under the config threshold).
    pub short_term_gain: Decimal,
    /// Net long-term gain (under the config threshold).
    pub long_term_gain: Decimal,
    /// Tax on the short-term gain.
    pub short_term_tax: Decimal,
    /// Tax on the long-term gain.
    pub long_term_tax: Decimal,
    /// `short_term_tax + long_term_tax`.
    pub total_tax: Decimal,
}
```
> The `CapitalGainsReport`/`Term`/`Duration` imports are consumed by `estimate` in Task 2; do Tasks 1–2 back-to-back and commit once (Task 2 Step 5).

- [ ] **Step 5: Run to verify pass.** `cargo test --lib tax` → PASS (`default_config_is_us_preset`). Do not commit yet.

---

## Task 2: `tax::estimate`

**Files:** Modify `src/tax.rs`.

- [ ] **Step 1: Append the failing tests** to the `tests` module:
```rust
    use crate::report::{CapitalGainsReport, RealizedGain, Term};
    use chrono::{TimeZone, Utc};

    fn row(acq_days_before: Option<i64>, term: Option<Term>, gain: Decimal) -> RealizedGain {
        let disposed = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
        let acquired = acq_days_before.map(|d| disposed - chrono::Duration::days(d));
        RealizedGain { asset: "bitcoin".into(), wallet: "w".into(), disposed_at: disposed,
            acquired_at: acquired, quantity: dec!(1), proceeds: dec!(0), cost_basis: dec!(0), gain, term }
    }
    fn report(rows: Vec<RealizedGain>) -> CapitalGainsReport {
        CapitalGainsReport { tax_year: 2024, rows, short_term_gain: dec!(0), long_term_gain: dec!(0), total_gain: dec!(0) }
    }

    #[test]
    fn short_term_flat_rate_on_gains_only() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 365,
            short_term_rate: dec!(0.30), long_term_brackets: vec![] };
        let e = estimate(&report(vec![row(Some(100), Some(Term::Short), dec!(1000))]), &cfg);
        assert_eq!(e.short_term_gain, dec!(1000));
        assert_eq!(e.short_term_tax, dec!(300));
        assert_eq!(e.long_term_tax, dec!(0));
    }
    #[test]
    fn short_term_loss_no_tax() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 365,
            short_term_rate: dec!(0.30), long_term_brackets: vec![] };
        let e = estimate(&report(vec![row(Some(10), Some(Term::Short), dec!(-500))]), &cfg);
        assert_eq!(e.short_term_tax, dec!(0));
    }
    #[test]
    fn long_term_progressive_brackets() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 365, short_term_rate: dec!(0.30),
            long_term_brackets: vec![
                TaxBracket { up_to: Some(dec!(1000)), rate: dec!(0.0) },
                TaxBracket { up_to: Some(dec!(3000)), rate: dec!(0.10) },
                TaxBracket { up_to: None, rate: dec!(0.20) } ] };
        let e = estimate(&report(vec![row(Some(400), Some(Term::Long), dec!(4000))]), &cfg);
        assert_eq!(e.long_term_gain, dec!(4000));
        assert_eq!(e.long_term_tax, dec!(400)); // 0*1000 + .1*2000 + .2*1000
    }
    #[test]
    fn threshold_reclassifies_independent_of_row_term() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 500, short_term_rate: dec!(0.30),
            long_term_brackets: vec![TaxBracket { up_to: None, rate: dec!(0.10) }] };
        let e = estimate(&report(vec![row(Some(400), Some(Term::Long), dec!(1000))]), &cfg);
        assert_eq!(e.short_term_gain, dec!(1000)); // 400d < 500d threshold => short
        assert_eq!(e.long_term_gain, dec!(0));
    }
    #[test]
    fn average_method_no_acquired_falls_back_to_row_term() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 365, short_term_rate: dec!(0.30),
            long_term_brackets: vec![TaxBracket { up_to: None, rate: dec!(0.10) }] };
        assert_eq!(estimate(&report(vec![row(None, None, dec!(1000))]), &cfg).short_term_gain, dec!(1000));
        assert_eq!(estimate(&report(vec![row(None, Some(Term::Long), dec!(1000))]), &cfg).long_term_tax, dec!(100));
    }
    #[test]
    fn empty_report_is_zero() {
        assert_eq!(estimate(&report(vec![]), &TaxConfig::default()).total_tax, dec!(0));
    }
```

- [ ] **Step 2: Run to verify failure.** `cargo test --lib tax` → FAIL (`estimate` undefined).

- [ ] **Step 3: Implement `estimate` + private `progressive_tax`** (below the types, above tests):
```rust
/// Estimate the tax on a year's realized gains.
///
/// Short/long subtotals are re-derived from each row's holding period against
/// `config.long_term_threshold_days` (rows with no `acquired_at` fall back to the
/// row's `term`). Short-term tax is the flat rate on positive net short-term gain;
/// long-term tax is progressive over the brackets on positive net long-term gain.
/// Losses never produce tax.
///
/// # Example
/// ```
/// use coinbasis::{TaxConfig, tax, CapitalGainsReport};
/// use rust_decimal::Decimal;
/// let report = CapitalGainsReport { tax_year: 2024, rows: vec![],
///     short_term_gain: Decimal::ZERO, long_term_gain: Decimal::ZERO, total_gain: Decimal::ZERO };
/// assert_eq!(tax::estimate(&report, &TaxConfig::default()).total_tax, Decimal::ZERO);
/// ```
pub fn estimate(report: &CapitalGainsReport, config: &TaxConfig) -> TaxEstimate {
    let mut short_gain = Decimal::ZERO;
    let mut long_gain = Decimal::ZERO;
    for r in &report.rows {
        let is_long = match r.acquired_at {
            Some(acq) => (r.disposed_at - acq) > Duration::days(config.long_term_threshold_days),
            None => matches!(r.term, Some(Term::Long)),
        };
        if is_long { long_gain += r.gain; } else { short_gain += r.gain; }
    }
    let short_tax = if short_gain > Decimal::ZERO { short_gain * config.short_term_rate } else { Decimal::ZERO };
    let long_tax = progressive_tax(long_gain.max(Decimal::ZERO), &config.long_term_brackets);
    TaxEstimate {
        short_term_gain: short_gain, long_term_gain: long_gain,
        short_term_tax: short_tax, long_term_tax: long_tax, total_tax: short_tax + long_tax,
    }
}

/// Apply ascending progressive brackets to a non-negative gain.
fn progressive_tax(gain: Decimal, brackets: &[TaxBracket]) -> Decimal {
    let mut tax = Decimal::ZERO;
    let mut prev = Decimal::ZERO;
    for b in brackets {
        if prev >= gain { break; }
        let ceiling = b.up_to.unwrap_or(gain);
        let top = ceiling.min(gain);
        let slice = top - prev;
        if slice > Decimal::ZERO { tax += slice * b.rate; }
        prev = ceiling;
        if b.up_to.is_none() { break; }
    }
    tax
}
```

- [ ] **Step 4: Run to verify pass.** `cargo test --lib tax` → PASS (all tax tests).

- [ ] **Step 5: Lint, format, commit Tasks 1+2.** `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings` → clean.
```bash
git add src/tax.rs src/lib.rs
git commit -m "Add tax module: TaxConfig, brackets, and progressive estimate"
```

---

## Task 3: `Portfolio::tax_estimate`

**Files:** Modify `src/portfolio.rs`; Modify `tests/headline.rs`.

- [ ] **Step 1: Write the failing test** in `tests/headline.rs`:
```rust
#[test]
fn portfolio_tax_estimate_matches_module_estimate() {
    use coinbasis::{CostBasisMethod, Portfolio, TaxConfig, Transaction, tax};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;
    let txs = vec![
        Transaction::Buy { timestamp: Utc.with_ymd_and_hms(2020,1,1,0,0,0).unwrap(), wallet: "w".into(),
            asset: "btc".into(), quantity: dec!(1), unit_price: dec!(100), fee: dec!(0) },
        Transaction::Sell { timestamp: Utc.with_ymd_and_hms(2022,1,1,0,0,0).unwrap(), wallet: "w".into(),
            asset: "btc".into(), quantity: dec!(1), unit_price: dec!(500), fee: dec!(0) },
    ];
    let p = Portfolio::from_transactions(&txs).unwrap();
    let cfg = TaxConfig::default();
    let via = p.tax_estimate(CostBasisMethod::Fifo, 2022, &cfg).unwrap();
    let report = p.capital_gains_report(CostBasisMethod::Fifo, 2022).unwrap();
    assert_eq!(via, tax::estimate(&report, &cfg));
    assert_eq!(via.long_term_gain, dec!(400));
}
```

- [ ] **Step 2: Run to verify failure.** `cargo test --test headline portfolio_tax_estimate` → FAIL (method undefined).

- [ ] **Step 3: Implement** — add `use crate::tax::{self, TaxConfig, TaxEstimate};` to `portfolio.rs` imports, then inside `impl Portfolio` after `capital_gains_report`:
```rust
    /// Estimate tax for one tax year under `method` and a [`TaxConfig`].
    ///
    /// # Example
    /// ```
    /// use coinbasis::{CostBasisMethod, Portfolio, TaxConfig, Transaction};
    /// use chrono::{TimeZone, Utc};
    /// use rust_decimal::Decimal;
    /// let txs = vec![
    ///     Transaction::Buy { timestamp: Utc.with_ymd_and_hms(2020,1,1,0,0,0).unwrap(), wallet: "w".into(),
    ///         asset: "btc".into(), quantity: Decimal::new(1,0), unit_price: Decimal::new(100,0), fee: Decimal::new(0,0) },
    ///     Transaction::Sell { timestamp: Utc.with_ymd_and_hms(2022,1,1,0,0,0).unwrap(), wallet: "w".into(),
    ///         asset: "btc".into(), quantity: Decimal::new(1,0), unit_price: Decimal::new(500,0), fee: Decimal::new(0,0) },
    /// ];
    /// let p = Portfolio::from_transactions(&txs).unwrap();
    /// let est = p.tax_estimate(CostBasisMethod::Fifo, 2022, &TaxConfig::default()).unwrap();
    /// assert_eq!(est.long_term_gain, Decimal::new(400, 0));
    /// ```
    pub fn tax_estimate(&self, method: CostBasisMethod, tax_year: i32, config: &TaxConfig)
        -> Result<TaxEstimate, PortfolioError> {
        let report = self.capital_gains_report(method, tax_year)?;
        Ok(tax::estimate(&report, config))
    }
```

- [ ] **Step 4: Run to verify pass.** `cargo test --test headline portfolio_tax_estimate` then `cargo test` → PASS.

- [ ] **Step 5: Lint + commit.** `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`.
```bash
git add src/portfolio.rs tests/headline.rs
git commit -m "Add Portfolio::tax_estimate convenience method"
```

---

## Task 4: serde round-trip + example

**Files:** Modify `tests/serde_roundtrip.rs`; Create `examples/tax_brackets.rs`.

- [ ] **Step 1: Append serde tests** to `tests/serde_roundtrip.rs`:
```rust
#[test]
fn taxconfig_json_roundtrip() {
    use coinbasis::TaxConfig;
    let cfg = TaxConfig::default();
    let back: TaxConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
    assert_eq!(cfg, back);
}
#[test]
fn taxconfig_parses_string_decimals_and_null_up_to() {
    use coinbasis::TaxConfig;
    let json = r#"{"jurisdiction":"US","long_term_threshold_days":365,"short_term_rate":"0.35",
        "long_term_brackets":[{"up_to":"47025","rate":"0.0"},{"up_to":null,"rate":"0.20"}]}"#;
    let cfg: TaxConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.long_term_brackets[1].up_to, None);
}
```

- [ ] **Step 2: Run.** `cargo test --test serde_roundtrip --features serde taxconfig` → PASS (types already serde-derived in Task 1).

- [ ] **Step 3: Create `examples/tax_brackets.rs`:**
```rust
//! Estimate tax with progressive long-term brackets. Run: `cargo run --example tax_brackets`.
use chrono::{TimeZone, Utc};
use coinbasis::{CostBasisMethod, Portfolio, TaxConfig, Transaction};
use rust_decimal::Decimal;

fn main() {
    let txs = vec![
        Transaction::Buy { timestamp: Utc.with_ymd_and_hms(2020,1,1,0,0,0).unwrap(), wallet: "hot".into(),
            asset: "btc".into(), quantity: Decimal::new(1,0), unit_price: Decimal::new(10000,0), fee: Decimal::new(0,0) },
        Transaction::Sell { timestamp: Utc.with_ymd_and_hms(2022,6,1,0,0,0).unwrap(), wallet: "hot".into(),
            asset: "btc".into(), quantity: Decimal::new(1,0), unit_price: Decimal::new(60000,0), fee: Decimal::new(0,0) },
    ];
    let p = Portfolio::from_transactions(&txs).unwrap();
    let est = p.tax_estimate(CostBasisMethod::Fifo, 2022, &TaxConfig::default()).unwrap();
    println!("long-term gain: {}", est.long_term_gain);
    println!("long-term tax:  {}", est.long_term_tax);
    println!("total tax:      {}", est.total_tax);
}
```

- [ ] **Step 4: Verify.** `cargo run --example tax_brackets` → prints long-term gain 50000 + positive tax; no panic.

- [ ] **Step 5: Full gates + commit.** `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo doc --no-deps --all-features` → all clean.
```bash
git add tests/serde_roundtrip.rs examples/tax_brackets.rs
git commit -m "Add TaxConfig serde round-trip test and tax_brackets example"
```

---

## Task 5: Version bump, README, release prep + PUBLISH GATE

**Files:** Modify `Cargo.toml`, `README.md`.

- [ ] **Step 1: Bump** `Cargo.toml` `version = "0.1.1"` → `"0.2.0"`.
- [ ] **Step 2: README** — add a "Tax estimation" section (default preset, `Portfolio::tax_estimate`, threshold-reclassification semantics, "not tax advice"), mirroring `examples/tax_brackets.rs`.
- [ ] **Step 3: Final gates.** `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features` → green.
- [ ] **Step 4: Package dry-run.** `cargo publish --dry-run` then `cargo package --list | grep -E "tax.rs|tax_brackets"` → dry-run succeeds; both files in the package list.
- [ ] **Step 5: Commit + merge to main.**
```bash
git add Cargo.toml Cargo.lock README.md
git commit -m "Release 0.2.0: tax bracket estimation"
git checkout main && git merge --ff-only feat/tax-brackets
```
- [ ] **Step 6: PUBLISH GATE.** Controller stops and asks **Jacob to run `cargo publish`** in `~/CodeRepos/coinbasis-rs` and confirm `coinbasis 0.2.0` is live on crates.io before Part C's tax tasks (C1).

---

## Self-Review (against the spec)
- **Spec coverage:** types (T1), estimate + progressive + threshold (T2), `Portfolio::tax_estimate` (T3), serde + example (T4), version/README/publish (T5). ✅
- **Placeholders:** none; the publish gate (T5 S6) is a deliberate human action.
- **Type consistency:** `TaxBracket`/`TaxConfig`/`TaxEstimate` fields + `estimate`/`tax_estimate` signatures consistent across tasks and the Part C consumer.
- **Executor note:** do Tasks 1–2 back-to-back (single commit) so the Task-1 imports aren't flagged unused.
