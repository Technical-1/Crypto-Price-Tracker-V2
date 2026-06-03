# Parity Phase 1 — Tax Brackets (coinbasis) + CSV Import (V2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add progressive long-term tax-bracket estimation to the `coinbasis` crate (published 0.2.0) and consume it in Crypto-Price-Tracker-V2, plus add CSV ledger import to V2 — closing two Python-parity gaps.

**Architecture:** Tax math (brackets) lives in `coinbasis` as a new pure `tax` module (`TaxConfig`/`TaxBracket`/`TaxEstimate`/`estimate`), additive and non-breaking. V2 deletes its flat-rate tax config, deserializes `config.json`'s `tax` directly into `coinbasis::tax::TaxConfig`, and renders the bracketed estimate. CSV import is a V2 `--import` flag that maps Python's CSV into `coinbasis::Transaction`s.

**Tech Stack:** Rust 2021, `coinbasis` 0.2 (`serde`), `rust_decimal`, `chrono`, `serde`/`serde_json`, `ratatui::TestBackend`, `clap`.

**Two repos:**
- coinbasis: `/Users/jacobkanfer/CodeRepos/coinbasis`
- V2: `/Users/jacobkanfer/CodeRepos/Crypto-Price-Tracker-V2`

**Author rule (both repos, hook-enforced):** commit author `51518860+Technical-1@users.noreply.github.com`; NO Co-Authored-By / Claude / AI attribution; never `--no-verify`. Verify `git config user.email` before the first commit in each repo.

**Branching:** Part A on a `feat/tax-brackets` branch in the coinbasis repo. Part B on a `feat/parity-phase1-tax-csv` branch in the V2 repo.

**PUBLISH GATE:** Tasks 1–5 are in coinbasis and end with `cargo publish --dry-run`. **Jacob runs the real `cargo publish`** (and confirms 0.2.0 is live on crates.io) before Part B (Tasks 6–10) — those don't compile until coinbasis 0.2.0 is published. (Fallback if publish indexing lags: add `[patch.crates-io] coinbasis = { path = "../coinbasis" }` to V2's Cargo.toml to build against the local checkout, and remove it once 0.2.0 resolves from crates.io.)

---

## Verified coinbasis facts (pinned)

- `report::Term` enum `{ Short, Long }` with `Term::classify(acquired_at: DateTime<Utc>, disposed_at: DateTime<Utc>) -> Term` (uses `> Duration::days(365)`).
- `report::RealizedGain { asset, wallet, disposed_at: DateTime<Utc>, acquired_at: Option<DateTime<Utc>>, quantity, proceeds, cost_basis, gain: Decimal, term: Option<Term> }`.
- `report::CapitalGainsReport { tax_year: i32, rows: Vec<RealizedGain>, short_term_gain, long_term_gain, total_gain: Decimal }`.
- `Portfolio::capital_gains_report(method: CostBasisMethod, tax_year: i32) -> Result<CapitalGainsReport, PortfolioError>`.
- Crate has `#![deny(missing_docs)]` (every pub item + field needs `///`, and the crate convention is a runnable doc-example per public fn). serde is an optional feature: `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]`. `rust_decimal_macros::dec!` is a **dev-dependency** (tests/doc-examples only) — non-test code uses `Decimal::new(mantissa, scale)`. Inline unit tests live in `#[cfg(test)] mod tests`; integration tests in `tests/` (`headline.rs`, `properties.rs`, `serde_roundtrip.rs`); examples in `examples/`.

---

# PART A — coinbasis 0.2.0

## Task 1: `tax` module types

**Files:**
- Create: `src/tax.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Branch + write the failing test** (in coinbasis repo)

```bash
cd /Users/jacobkanfer/CodeRepos/coinbasis
git checkout -b feat/tax-brackets
```

Create `src/tax.rs` with ONLY this test module first (so it fails to compile):

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

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib tax`
Expected: FAIL to compile — `TaxConfig` not defined / `tax` module not declared.

- [ ] **Step 3: Add the module declaration + re-exports to `src/lib.rs`**

After the existing `pub mod stats;` line add:
```rust
pub mod tax;
```
And in the re-export block (after the `transaction::` re-export) add:
```rust
pub use tax::{TaxBracket, TaxConfig, TaxEstimate};
```

- [ ] **Step 4: Implement the types at the TOP of `src/tax.rs`** (above the test module)

```rust
//! Tax-liability estimation over a [`crate::CapitalGainsReport`].
//!
//! Cost-basis accounting (which lots, what gain, short/long term) is the rest of
//! the crate's job; this module turns a year's realized gains into an estimated
//! tax using a [`TaxConfig`] — a flat short-term rate plus progressive
//! long-term brackets, with a configurable holding-period threshold.
//!
//! Not tax advice; see the crate root.

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

> Note: `use chrono::Duration;`, `use crate::report::{CapitalGainsReport, Term};`, and `use rust_decimal::Decimal;` are imported now though `estimate` (Task 2) is what consumes them. If the unused-import lint fires before Task 2, temporarily `#[allow(unused_imports)]` is acceptable — but Task 2 immediately follows in the same branch, so prefer to do Tasks 1–2 back-to-back and only commit after Task 2. (This task's commit is folded into Task 2's commit.)

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib tax`
Expected: PASS (`default_config_is_us_preset`). (Do not commit yet — commit with Task 2.)

---

## Task 2: `tax::estimate`

**Files:**
- Modify: `src/tax.rs`

- [ ] **Step 1: Write the failing tests** (append to the `tests` module in `src/tax.rs`)

```rust
    use crate::report::{CapitalGainsReport, RealizedGain, Term};
    use chrono::{TimeZone, Utc};

    fn row(acq_days_before_disposal: Option<i64>, term: Option<Term>, gain: Decimal) -> RealizedGain {
        let disposed = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
        let acquired = acq_days_before_disposal.map(|d| disposed - chrono::Duration::days(d));
        RealizedGain {
            asset: "bitcoin".into(), wallet: "w".into(), disposed_at: disposed,
            acquired_at: acquired, quantity: dec!(1), proceeds: dec!(0),
            cost_basis: dec!(0), gain, term,
        }
    }

    fn report(rows: Vec<RealizedGain>) -> CapitalGainsReport {
        CapitalGainsReport { tax_year: 2024, rows, short_term_gain: dec!(0),
            long_term_gain: dec!(0), total_gain: dec!(0) }
    }

    #[test]
    fn short_term_flat_rate_on_gains_only() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 365,
            short_term_rate: dec!(0.30), long_term_brackets: vec![] };
        // 100-day hold => short
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
        assert_eq!(e.short_term_gain, dec!(-500));
        assert_eq!(e.short_term_tax, dec!(0));
    }

    #[test]
    fn long_term_progressive_brackets() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 365,
            short_term_rate: dec!(0.30),
            long_term_brackets: vec![
                TaxBracket { up_to: Some(dec!(1000)), rate: dec!(0.0) },
                TaxBracket { up_to: Some(dec!(3000)), rate: dec!(0.10) },
                TaxBracket { up_to: None, rate: dec!(0.20) },
            ] };
        // 400-day hold => long; gain 4000 => 0*1000 + 0.10*2000 + 0.20*1000 = 200+200 = 400
        let e = estimate(&report(vec![row(Some(400), Some(Term::Long), dec!(4000))]), &cfg);
        assert_eq!(e.long_term_gain, dec!(4000));
        assert_eq!(e.long_term_tax, dec!(400));
        assert_eq!(e.total_tax, dec!(400));
    }

    #[test]
    fn threshold_reclassifies_independent_of_row_term() {
        // 400-day hold, row says Long, but a 500-day threshold makes it SHORT here.
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 500,
            short_term_rate: dec!(0.30),
            long_term_brackets: vec![TaxBracket { up_to: None, rate: dec!(0.10) }] };
        let e = estimate(&report(vec![row(Some(400), Some(Term::Long), dec!(1000))]), &cfg);
        assert_eq!(e.short_term_gain, dec!(1000)); // reclassified short
        assert_eq!(e.long_term_gain, dec!(0));
        assert_eq!(e.short_term_tax, dec!(300));
    }

    #[test]
    fn average_method_no_acquired_falls_back_to_row_term() {
        let cfg = TaxConfig { jurisdiction: "t".into(), long_term_threshold_days: 365,
            short_term_rate: dec!(0.30),
            long_term_brackets: vec![TaxBracket { up_to: None, rate: dec!(0.10) }] };
        // acquired_at None, term None => treated short
        let e = estimate(&report(vec![row(None, None, dec!(1000))]), &cfg);
        assert_eq!(e.short_term_gain, dec!(1000));
        // acquired_at None, term Long => long
        let e2 = estimate(&report(vec![row(None, Some(Term::Long), dec!(1000))]), &cfg);
        assert_eq!(e2.long_term_gain, dec!(1000));
        assert_eq!(e2.long_term_tax, dec!(100));
    }

    #[test]
    fn empty_report_is_zero() {
        let e = estimate(&report(vec![]), &TaxConfig::default());
        assert_eq!(e.total_tax, dec!(0));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib tax`
Expected: FAIL — `estimate` not defined.

- [ ] **Step 3: Implement `estimate` + a private `progressive_tax`** (in `src/tax.rs`, below the types, above tests)

```rust
/// Estimate the tax on a year's realized gains.
///
/// Short/long subtotals are re-derived from each row's holding period against
/// `config.long_term_threshold_days` (so a non-365 threshold changes the split
/// without depending on the report's own 365-based `term`). Rows with no
/// `acquired_at` (the `Average` method) fall back to the row's `term`.
/// Short-term tax is the flat rate on positive net short-term gain; long-term
/// tax is progressive over the brackets on positive net long-term gain. Losses
/// never produce tax.
///
/// # Example
/// ```
/// use coinbasis::{TaxConfig, tax};
/// use coinbasis::{CapitalGainsReport};
/// let report = CapitalGainsReport { tax_year: 2024, rows: vec![],
///     short_term_gain: rust_decimal::Decimal::ZERO,
///     long_term_gain: rust_decimal::Decimal::ZERO,
///     total_gain: rust_decimal::Decimal::ZERO };
/// let est = tax::estimate(&report, &TaxConfig::default());
/// assert_eq!(est.total_tax, rust_decimal::Decimal::ZERO);
/// ```
pub fn estimate(report: &CapitalGainsReport, config: &TaxConfig) -> TaxEstimate {
    let mut short_gain = Decimal::ZERO;
    let mut long_gain = Decimal::ZERO;
    for r in &report.rows {
        let is_long = match r.acquired_at {
            Some(acq) => (r.disposed_at - acq) > Duration::days(config.long_term_threshold_days),
            None => matches!(r.term, Some(Term::Long)),
        };
        if is_long {
            long_gain += r.gain;
        } else {
            short_gain += r.gain;
        }
    }
    let short_tax = if short_gain > Decimal::ZERO {
        short_gain * config.short_term_rate
    } else {
        Decimal::ZERO
    };
    let long_tax = progressive_tax(long_gain.max(Decimal::ZERO), &config.long_term_brackets);
    TaxEstimate {
        short_term_gain: short_gain,
        long_term_gain: long_gain,
        short_term_tax: short_tax,
        long_term_tax: long_tax,
        total_tax: short_tax + long_tax,
    }
}

/// Apply ascending progressive brackets to a non-negative gain. Each bracket
/// taxes the slice between the previous ceiling and `min(gain, up_to)`; an
/// `up_to: None` bracket is the unbounded final one.
fn progressive_tax(gain: Decimal, brackets: &[TaxBracket]) -> Decimal {
    let mut tax = Decimal::ZERO;
    let mut prev = Decimal::ZERO;
    for b in brackets {
        if prev >= gain {
            break;
        }
        let ceiling = b.up_to.unwrap_or(gain);
        let top = ceiling.min(gain);
        let slice = top - prev;
        if slice > Decimal::ZERO {
            tax += slice * b.rate;
        }
        prev = ceiling;
        if b.up_to.is_none() {
            break;
        }
    }
    tax
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib tax`
Expected: PASS (all tax tests incl. Task 1's).

- [ ] **Step 5: Lint + format, then commit Tasks 1+2 together**

Run: `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.
```bash
git add src/tax.rs src/lib.rs
git commit -m "Add tax module: TaxConfig, brackets, and progressive estimate"
```

---

## Task 3: `Portfolio::tax_estimate` convenience method

**Files:**
- Modify: `src/portfolio.rs`

- [ ] **Step 1: Write the failing test** (append to the `#[cfg(test)] mod tests` in `src/portfolio.rs`; if none exists there, the crate's facade tests live in `tests/headline.rs` — add it there instead, adapting imports to `use coinbasis::...`)

Add to `tests/headline.rs`:
```rust
#[test]
fn portfolio_tax_estimate_matches_module_estimate() {
    use coinbasis::{CostBasisMethod, Portfolio, TaxConfig, Transaction, tax};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    let txs = vec![
        Transaction::Buy { timestamp: Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
            wallet: "w".into(), asset: "btc".into(), quantity: dec!(1),
            unit_price: dec!(100), fee: dec!(0) },
        Transaction::Sell { timestamp: Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap(),
            wallet: "w".into(), asset: "btc".into(), quantity: dec!(1),
            unit_price: dec!(500), fee: dec!(0) },
    ];
    let p = Portfolio::from_transactions(&txs).unwrap();
    let cfg = TaxConfig::default();
    let via_method = p.tax_estimate(CostBasisMethod::Fifo, 2022, &cfg).unwrap();
    let report = p.capital_gains_report(CostBasisMethod::Fifo, 2022).unwrap();
    assert_eq!(via_method, tax::estimate(&report, &cfg));
    assert!(via_method.long_term_gain == dec!(400));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test headline portfolio_tax_estimate_matches_module_estimate`
Expected: FAIL — `tax_estimate` method not found.

- [ ] **Step 3: Implement the method** (inside `impl Portfolio` in `src/portfolio.rs`, after `capital_gains_report`)

Add `use crate::tax::{self, TaxConfig, TaxEstimate};` to the imports at the top of `portfolio.rs` (alongside the existing `use crate::report::{...}`), then add the method:

```rust
    /// Estimate tax for one tax year under `method` and a [`TaxConfig`].
    ///
    /// Convenience wrapper over [`capital_gains_report`](Self::capital_gains_report)
    /// + [`crate::tax::estimate`].
    ///
    /// # Example
    /// ```
    /// use coinbasis::{CostBasisMethod, Portfolio, TaxConfig, Transaction};
    /// use chrono::{TimeZone, Utc};
    /// use rust_decimal::Decimal;
    /// let txs = vec![
    ///     Transaction::Buy { timestamp: Utc.with_ymd_and_hms(2020,1,1,0,0,0).unwrap(),
    ///         wallet: "w".into(), asset: "btc".into(),
    ///         quantity: Decimal::new(1,0), unit_price: Decimal::new(100,0), fee: Decimal::new(0,0) },
    ///     Transaction::Sell { timestamp: Utc.with_ymd_and_hms(2022,1,1,0,0,0).unwrap(),
    ///         wallet: "w".into(), asset: "btc".into(),
    ///         quantity: Decimal::new(1,0), unit_price: Decimal::new(500,0), fee: Decimal::new(0,0) },
    /// ];
    /// let p = Portfolio::from_transactions(&txs).unwrap();
    /// let est = p.tax_estimate(CostBasisMethod::Fifo, 2022, &TaxConfig::default()).unwrap();
    /// assert_eq!(est.long_term_gain, Decimal::new(400, 0));
    /// ```
    pub fn tax_estimate(
        &self,
        method: CostBasisMethod,
        tax_year: i32,
        config: &TaxConfig,
    ) -> Result<TaxEstimate, PortfolioError> {
        let report = self.capital_gains_report(method, tax_year)?;
        Ok(tax::estimate(&report, config))
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --test headline portfolio_tax_estimate_matches_module_estimate` then `cargo test`
Expected: PASS; full suite green.

- [ ] **Step 5: Lint + commit**

Run: `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`
```bash
git add src/portfolio.rs tests/headline.rs
git commit -m "Add Portfolio::tax_estimate convenience method"
```

---

## Task 4: serde round-trip test + example

**Files:**
- Modify: `tests/serde_roundtrip.rs`
- Create: `examples/tax_brackets.rs`

- [ ] **Step 1: Write the failing serde test** (append to `tests/serde_roundtrip.rs`)

```rust
#[test]
fn taxconfig_json_roundtrip() {
    use coinbasis::TaxConfig;
    let cfg = TaxConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: TaxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn taxconfig_parses_string_decimals_and_null_up_to() {
    use coinbasis::TaxConfig;
    let json = r#"{
        "jurisdiction": "US",
        "long_term_threshold_days": 365,
        "short_term_rate": "0.35",
        "long_term_brackets": [
            {"up_to": "47025", "rate": "0.0"},
            {"up_to": null, "rate": "0.20"}
        ]
    }"#;
    let cfg: TaxConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.long_term_brackets.len(), 2);
    assert_eq!(cfg.long_term_brackets[1].up_to, None);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test serde_roundtrip --features serde taxconfig`
Expected: FAIL to compile until... actually it should PASS already (types exist with serde from Task 1). If it passes immediately, that's fine — it confirms serde wiring. If `serde_json` string→Decimal needs the `rust_decimal/serde` feature, it's enabled by the crate's `serde` feature. Run with `--features serde`.

- [ ] **Step 3: Create `examples/tax_brackets.rs`**

```rust
//! Estimate tax with progressive long-term brackets.
//!
//! Run with: `cargo run --example tax_brackets`

use chrono::{TimeZone, Utc};
use coinbasis::{CostBasisMethod, Portfolio, TaxConfig, Transaction};
use rust_decimal::Decimal;

fn main() {
    let txs = vec![
        Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
            wallet: "hot".into(), asset: "btc".into(),
            quantity: Decimal::new(1, 0), unit_price: Decimal::new(10000, 0), fee: Decimal::new(0, 0),
        },
        Transaction::Sell {
            timestamp: Utc.with_ymd_and_hms(2022, 6, 1, 0, 0, 0).unwrap(),
            wallet: "hot".into(), asset: "btc".into(),
            quantity: Decimal::new(1, 0), unit_price: Decimal::new(60000, 0), fee: Decimal::new(0, 0),
        },
    ];
    let p = Portfolio::from_transactions(&txs).unwrap();
    let est = p.tax_estimate(CostBasisMethod::Fifo, 2022, &TaxConfig::default()).unwrap();
    println!("long-term gain: {}", est.long_term_gain);
    println!("long-term tax:  {}", est.long_term_tax);
    println!("total tax:      {}", est.total_tax);
}
```

- [ ] **Step 4: Verify example builds + runs**

Run: `cargo run --example tax_brackets`
Expected: prints a long-term gain of 50000 and a positive tax; no panic.

- [ ] **Step 5: Run full gates + commit**

Run: `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo doc --no-deps --all-features`
Expected: all clean (doc build confirms `#![deny(missing_docs)]` is satisfied).
```bash
git add tests/serde_roundtrip.rs examples/tax_brackets.rs
git commit -m "Add TaxConfig serde round-trip test and tax_brackets example"
```

---

## Task 5: Version bump, README, release prep

**Files:**
- Modify: `Cargo.toml`, `README.md`

- [ ] **Step 1: Bump the version**

In `Cargo.toml` change `version = "0.1.1"` to `version = "0.2.0"`.

- [ ] **Step 2: Add a README section** documenting tax estimation. After the existing capital-gains/income section, add a "Tax estimation" subsection showing `TaxConfig::default()` and `Portfolio::tax_estimate(...)` (mirror the `examples/tax_brackets.rs` snippet) and note the threshold-reclassification semantics and "not tax advice".

- [ ] **Step 3: Final verification**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features`
Expected: all green.

- [ ] **Step 4: Package dry-run**

Run: `cargo publish --dry-run` then `cargo package --list | head -40`
Expected: dry-run succeeds; `src/tax.rs` and `examples/tax_brackets.rs` appear in the package file list.

- [ ] **Step 5: Commit + merge to main (coinbasis repo)**

```bash
git add Cargo.toml Cargo.lock README.md
git commit -m "Release 0.2.0: tax bracket estimation"
git checkout main && git merge --ff-only feat/tax-brackets
```

- [ ] **Step 6: PUBLISH GATE — Jacob publishes**

The controller stops here and asks Jacob to run `cargo publish` in `/Users/jacobkanfer/CodeRepos/coinbasis` and confirm `coinbasis 0.2.0` is live on crates.io. Do not start Part B until confirmed (or until the `[patch.crates-io]` fallback is in place per the plan header).

---

# PART B — Crypto-Price-Tracker-V2 (gated on coinbasis 0.2.0)

## Task 6: Bump dependency + migrate tax config

**Files:**
- Modify: `Cargo.toml`, `src/config.rs`, `config.example.json`

- [ ] **Step 1: Branch + bump the dependency** (in V2 repo)

```bash
cd /Users/jacobkanfer/CodeRepos/Crypto-Price-Tracker-V2
git checkout -b feat/parity-phase1-tax-csv
```
In `Cargo.toml` change `coinbasis = { version = "0.1", features = ["serde"] }` to `coinbasis = { version = "0.2", features = ["serde"] }`. Run `cargo update -p coinbasis` then `cargo build` to confirm 0.2.0 resolves.

- [ ] **Step 2: Update the failing config test** (in `src/config.rs` tests)

Replace the `sample_json()` `tax` block and the `parses_full_config` tax assertions:
```rust
// in sample_json(), replace the "tax": {...} line with:
          "tax": { "jurisdiction": "US", "long_term_threshold_days": 365,
                   "short_term_rate": "0.35",
                   "long_term_brackets": [ {"up_to": "47025", "rate": "0.0"},
                                           {"up_to": null, "rate": "0.20"} ] },
```
```rust
// in parses_full_config(), replace `assert_eq!(c.tax.short_term_rate, 0.24);` with:
        assert_eq!(c.tax.short_term_rate, rust_decimal_macros::dec!(0.35));
        assert_eq!(c.tax.long_term_threshold_days, 365);
        assert_eq!(c.tax.long_term_brackets.len(), 2);
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test config`
Expected: FAIL to compile — V2's local `TaxConfig` has no `jurisdiction`/`brackets` fields (or the f64 assertion type-mismatches).

- [ ] **Step 4: Migrate `src/config.rs`**

- Delete V2's local `TaxConfig` struct entirely.
- Add `use coinbasis::tax::TaxConfig;` at the top.
- The `Config` struct's field stays `pub tax: TaxConfig` (now the coinbasis type).
- In `Config::example()`, replace the `tax: TaxConfig { short_term_rate: 0.24, long_term_rate: 0.15 }` initializer with:
```rust
            tax: TaxConfig::default(),
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test config`
Expected: PASS.

- [ ] **Step 6: Update `config.example.json`** — replace the `"tax"` object with:
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

- [ ] **Step 7: Commit**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
```bash
git add Cargo.toml Cargo.lock src/config.rs config.example.json
git commit -m "Bump coinbasis to 0.2 and migrate tax config to coinbasis::tax::TaxConfig"
```

---

## Task 7: Bracketed estimate in the Tax view

**Files:**
- Modify: `src/ui/tax.rs`

- [ ] **Step 1: Update the test** (in `src/ui/tax.rs` tests)

The existing `shows_tax_columns_subtotals_and_estimate` test already asserts `"Short-term"`, `"Estimated Tax"`, `"2024"`. Add an assertion that the jurisdiction label renders:
```rust
        assert!(s.contains("US")); // jurisdiction label from TaxConfig::default via Config::example
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test ui::tax`
Expected: FAIL — "US" not yet rendered (current view shows flat user-rates text).

- [ ] **Step 3: Rewrite the summary block in `src/ui/tax.rs`**

Replace the summary-building section (the `if let Some(cg) = &app.derived.capital_gains { ... }` block that computes `short_tax`/`long_tax` from `app.config.tax.short_term_rate`/`long_term_rate`) with one that calls coinbasis:

```rust
    let mut lines = Vec::new();
    if let Some(cg) = &app.derived.capital_gains {
        let est = coinbasis::tax::estimate(cg, &app.config.tax);
        lines.push(Line::from(format!(
            "Short-term gain: {:+.2}    tax: {:.2}",
            est.short_term_gain, est.short_term_tax
        )));
        lines.push(Line::from(format!(
            "Long-term gain:  {:+.2}    tax: {:.2}",
            est.long_term_gain, est.long_term_tax
        )));
        lines.push(Line::from(format!("Total gain:      {:+.2}", cg.total_gain)));
        lines.push(Line::from(format!(
            "Estimated Tax:   {:.2}  ({}, brackets)",
            est.total_tax, app.config.tax.jurisdiction
        )));
    }
    if let Some(inc) = &app.derived.income {
        lines.push(Line::from(format!("Income:          {:.2}", inc.total_income)));
    }
```
Remove the now-unused `rust_decimal::prelude::ToPrimitive` import if it is no longer referenced elsewhere in the file (the f64 `to_f64()` rate math is gone). Keep `coinbasis::Term` import (still used by the rows table).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test ui::tax` then `cargo build`
Expected: PASS; build clean.

- [ ] **Step 5: Commit**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
```bash
git add src/ui/tax.rs
git commit -m "Render bracketed tax estimate via coinbasis::tax in Tax view"
```

---

## Task 8: `import_csv` in the ledger

**Files:**
- Modify: `src/ledger.rs`

- [ ] **Step 1: Write the failing tests** (append to `src/ledger.rs` tests)

```rust
    #[test]
    fn import_maps_buys_and_sells_with_optional_wallet() {
        let dir = std::env::temp_dir().join("cpt2_import_basic");
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv");
        let ledger = dir.join("ledger.json");
        let _ = std::fs::remove_file(&ledger);
        std::fs::write(&csv,
            "date,coin,action,quantity,price_usd,fee_usd,wallet\n\
             2021-01-01,bitcoin,buy,0.5,30000,5,coinbase\n\
             2021-06-01,bitcoin,sell,0.2,40000,2\n").unwrap();
        let (added, skipped) = import_csv(csv.to_str().unwrap(), ledger.to_str().unwrap()).unwrap();
        assert_eq!((added, skipped), (2, 0));
        let txs = load_ledger(ledger.to_str().unwrap()).unwrap();
        assert_eq!(txs.len(), 2);
        match &txs[0] {
            coinbasis::Transaction::Buy { wallet, asset, .. } => {
                assert_eq!(wallet, "coinbase"); assert_eq!(asset, "bitcoin");
            }
            _ => panic!("expected Buy"),
        }
        match &txs[1] {
            coinbasis::Transaction::Sell { wallet, .. } => assert_eq!(wallet, "default"),
            _ => panic!("expected Sell"),
        }
    }

    #[test]
    fn import_dedups_against_existing_ledger() {
        let dir = std::env::temp_dir().join("cpt2_import_dedup");
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv");
        let ledger = dir.join("ledger.json");
        let _ = std::fs::remove_file(&ledger);
        let row = "date,coin,action,quantity,price_usd,fee_usd\n2021-01-01,bitcoin,buy,1,30000,0\n";
        std::fs::write(&csv, row).unwrap();
        assert_eq!(import_csv(csv.to_str().unwrap(), ledger.to_str().unwrap()).unwrap(), (1, 0));
        // importing the same file again => all skipped as duplicates
        assert_eq!(import_csv(csv.to_str().unwrap(), ledger.to_str().unwrap()).unwrap(), (0, 1));
        assert_eq!(load_ledger(ledger.to_str().unwrap()).unwrap().len(), 1);
    }

    #[test]
    fn import_skips_invalid_rows_and_defaults_fee() {
        let dir = std::env::temp_dir().join("cpt2_import_invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv");
        let ledger = dir.join("ledger.json");
        let _ = std::fs::remove_file(&ledger);
        std::fs::write(&csv,
            "date,coin,action,quantity,price_usd,fee_usd\n\
             2021-01-01,bitcoin,buy,1,30000,\n\
             bad-date,bitcoin,buy,1,30000,0\n\
             2021-01-02,bitcoin,hodl,1,30000,0\n\
             2021-01-03,bitcoin,buy,-1,30000,0\n").unwrap();
        let (added, skipped) = import_csv(csv.to_str().unwrap(), ledger.to_str().unwrap()).unwrap();
        assert_eq!((added, skipped), (1, 3)); // only the blank-fee buy is valid (fee defaults 0)
        match &load_ledger(ledger.to_str().unwrap()).unwrap()[0] {
            coinbasis::Transaction::Buy { fee, .. } => assert_eq!(*fee, rust_decimal_macros::dec!(0)),
            _ => panic!("expected Buy"),
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test ledger::tests::import`
Expected: FAIL — `import_csv` not defined.

- [ ] **Step 3: Implement `import_csv`** (add to `src/ledger.rs`)

Add imports at the top of `ledger.rs`:
```rust
use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use std::str::FromStr;
```
Then:
```rust
/// Import transactions from a CSV (`date,coin,action,quantity,price_usd,fee_usd`
/// with an optional trailing `wallet` column) into the JSON ledger at
/// `ledger_path`, appending non-duplicate rows. Returns `(added, skipped)`.
/// Invalid rows are skipped with a stderr notice; import continues.
pub fn import_csv(csv_path: &str, ledger_path: &str) -> Result<(usize, usize), AppError> {
    let text = std::fs::read_to_string(csv_path).map_err(|e| AppError::Ledger {
        path: csv_path.to_string(),
        reason: e.to_string(),
    })?;
    let mut existing = load_ledger(ledger_path).unwrap_or_default();
    let mut added = 0usize;
    let mut skipped = 0usize;

    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue; // header or blank
        }
        match parse_csv_row(line) {
            Some(tx) => {
                if existing.contains(&tx) {
                    skipped += 1;
                } else {
                    existing.push(tx);
                    added += 1;
                }
            }
            None => {
                eprintln!("(skipped import line {}: invalid row)", i + 1);
                skipped += 1;
            }
        }
    }

    if added > 0 {
        save_ledger(ledger_path, &existing)?;
    }
    Ok((added, skipped))
}

fn parse_csv_row(line: &str) -> Option<Transaction> {
    let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if f.len() < 6 {
        return None;
    }
    let date = NaiveDate::parse_from_str(f[0], "%Y-%m-%d").ok()?;
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?);
    let asset = f[1];
    if asset.is_empty() {
        return None;
    }
    let action = f[2].to_lowercase();
    let quantity = Decimal::from_str(f[3]).ok()?;
    if quantity <= Decimal::ZERO {
        return None;
    }
    let unit_price = Decimal::from_str(f[4]).ok()?;
    if unit_price < Decimal::ZERO {
        return None;
    }
    let fee = if f[5].is_empty() {
        Decimal::ZERO
    } else {
        Decimal::from_str(f[5]).ok()?
    };
    if fee < Decimal::ZERO {
        return None;
    }
    let wallet = f.get(6).map(|s| s.to_string()).filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string());
    match action.as_str() {
        "buy" => Some(Transaction::Buy { timestamp, wallet, asset: asset.to_string(),
            quantity, unit_price, fee }),
        "sell" => Some(Transaction::Sell { timestamp, wallet, asset: asset.to_string(),
            quantity, unit_price, fee }),
        _ => None,
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test ledger`
Expected: PASS (all ledger tests incl. the 3 new import tests).

- [ ] **Step 5: Commit**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
```bash
git add src/ledger.rs
git commit -m "Add CSV ledger import mapping to coinbasis transactions"
```

---

## Task 9: `--import` CLI flag

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add the flag to `Args`** (in `src/main.rs`)

Add to the `Args` struct:
```rust
    /// Import transactions from a CSV file into the ledger, then exit.
    #[arg(long)]
    import: Option<String>,
```

- [ ] **Step 2: Wire the import branch** in `main()`, AFTER `ledger_path` is computed and BEFORE `install_panic_hook()` / `setup_terminal()`:

```rust
    if let Some(csv) = args.import.as_deref() {
        let (added, skipped) = ledger::import_csv(csv, &ledger_path).context("importing CSV")?;
        println!("imported {added}, skipped {skipped} (ledger: {ledger_path})");
        return Ok(());
    }
```
(`ledger` is already imported in main.rs via `use crypto_price_tracker_v2::ledger::{self, load_ledger};` — confirm `ledger::import_csv` resolves; if main only imported `load_ledger`, the `self` is already there.)

- [ ] **Step 3: Build + manual smoke test**

Run:
```bash
cargo build
printf 'date,coin,action,quantity,price_usd,fee_usd\n2021-01-01,bitcoin,buy,0.5,30000,5\n' > /tmp/cpt2_smoke.csv
cargo run -- --import /tmp/cpt2_smoke.csv --ledger /tmp/cpt2_smoke_ledger.json
```
Expected: prints `imported 1, skipped 0 (ledger: /tmp/cpt2_smoke_ledger.json)` and exits 0 WITHOUT entering the TUI. Re-running prints `imported 0, skipped 1`.

- [ ] **Step 4: Commit**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
```bash
git add src/main.rs
git commit -m "Add --import flag to import a CSV ledger then exit"
```

---

## Task 10: Sample file, README, finalize

**Files:**
- Create: `transactions.example.csv`
- Modify: `README.md`

- [ ] **Step 1: Create `transactions.example.csv`**

```
date,coin,action,quantity,price_usd,fee_usd,wallet
2021-01-01,bitcoin,buy,0.5,30000,5,coinbase
2021-06-01,ethereum,buy,2,2000,3,coinbase
2024-02-01,bitcoin,sell,0.2,50000,4,coinbase
```
(Note in the README that the `wallet` column is optional and defaults to `default`.)

- [ ] **Step 2: Update `README.md`** — add a "CSV import" subsection (`cargo run -- --import transactions.example.csv`, the column format, optional wallet) and update the tax note to mention configurable progressive brackets + jurisdiction in `config.json` (`tax` block).

- [ ] **Step 3: Full gate**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all green (≈ existing tests + new config/tax/import tests + integration).

- [ ] **Step 4: Commit + merge to main (V2 repo)**

```bash
git add transactions.example.csv README.md
git commit -m "Add CSV import sample and document tax brackets + import"
git checkout main && git merge --ff-only feat/parity-phase1-tax-csv
```

---

## Self-Review (against the spec)

**Spec coverage:**
- §3 coinbasis tax module (types, `estimate`, `Portfolio::tax_estimate`, default preset, threshold reclassification, progressive brackets, serde, docs/tests/example, 0.2.0, publish) → Tasks 1–5. ✓
- §4 V2 consumption (dep bump, config → `coinbasis::tax::TaxConfig`, example json, Tax view) → Tasks 6–7. ✓
- §5 V2 CSV import (`import_csv`, optional wallet default, dedup, validation, `--import` flag, sample file) → Tasks 8–10. ✓
- §6 testing → each task is TDD; coinbasis `--all-features` doc build covers missing-docs. ✓
- §7 build ordering → Tasks ordered with the publish gate at Task 5. ✓

**Placeholder scan:** none — every code step has complete code; the publish gate (Task 5 Step 6) is a deliberate human action, not a placeholder.

**Type consistency:** `TaxConfig`/`TaxBracket`/`TaxEstimate` fields and `tax::estimate`/`Portfolio::tax_estimate` signatures match across coinbasis tasks and V2 consumption. `import_csv(&str, &str) -> Result<(usize, usize), AppError>` matches its call site in Task 9. V2 `Config.tax` is `coinbasis::tax::TaxConfig` consistently in Tasks 6–7.

**Note for executor:** if `cargo clippy --all-features` in coinbasis flags the `use crate::report::CapitalGainsReport` import as unused in Task 1's intermediate state, that resolves in Task 2 — do Tasks 1 and 2 back-to-back, committing once (as written).
