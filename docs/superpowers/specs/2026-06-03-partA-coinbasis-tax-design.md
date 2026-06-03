# Part A — coinbasis 0.2.0: Tax-Bracket Estimation — Design Spec

**Author:** Jacob Kanfer · **Date:** 2026-06-03 · **Status:** Approved (design); pending spec review

Part A of the 3-part Crypto-Price-Tracker-V2 parity effort (A: coinbasis tax · B: cryptolytics · C: V2 consumption). Repo: `~/CodeRepos/coinbasis` (published crate, currently 0.1.1).

## 1. Scope

Add tax-liability estimation (flat short-term rate + **progressive long-term brackets**, configurable holding-period threshold, jurisdiction label) to coinbasis as a new pure `tax` module. Bump to **0.2.0** and publish to crates.io. Purely additive — no existing public signature or the fixed 365-day `Term` classification changes.

**Non-goals:** no multi-jurisdiction rule engine (`jurisdiction` is a display label); no change to existing cost-basis/report APIs; no network/IO (crate stays pure).

## 2. Design

New `pub mod tax` (`src/tax.rs`), serde-gated derives, `#![deny(missing_docs)]` doc-examples on every public item.

```rust
pub struct TaxBracket { pub up_to: Option<Decimal>, pub rate: Decimal }   // up_to None = unbounded top
pub struct TaxConfig {
    pub jurisdiction: String,
    pub long_term_threshold_days: i64,
    pub short_term_rate: Decimal,
    pub long_term_brackets: Vec<TaxBracket>,   // ascending, unbounded bracket last
}
impl Default for TaxConfig { /* US preset: 0.35 short; LT 0%→47025, 15%→518900, 20%→∞; threshold 365 */ }
pub struct TaxEstimate {
    pub short_term_gain: Decimal, pub long_term_gain: Decimal,
    pub short_term_tax: Decimal, pub long_term_tax: Decimal, pub total_tax: Decimal,
}
pub fn estimate(report: &CapitalGainsReport, config: &TaxConfig) -> TaxEstimate;
```
Plus a facade convenience: `Portfolio::tax_estimate(method, tax_year, &TaxConfig) -> Result<TaxEstimate, PortfolioError>` (= `capital_gains_report` + `tax::estimate`).

**Estimation algorithm:**
- **Short/long split re-derived per row under the config threshold:** if `acquired_at` is `Some`, long iff `(disposed_at - acquired_at) > Duration::days(long_term_threshold_days)`; if `acquired_at` is `None` (Average method), fall back to the row's existing `term` (`Some(Term::Long)` → long, else short). Sum `gain` into the respective subtotal. This bends the split for non-365 thresholds without touching coinbasis's 365-based `Term`.
- **Short-term tax** = `max(0, short_term_gain) * short_term_rate`.
- **Long-term tax** = progressive over brackets on `max(0, long_term_gain)`: each bracket taxes the slice `min(gain, up_to) - prev` at its `rate`; `up_to: None` is the unbounded final bracket. Losses → 0.
- `total_tax = short + long`.

**Numeric type:** `Decimal` throughout (consistent with the rest of coinbasis).
**serde:** `TaxConfig`/`TaxBracket`/`TaxEstimate` derive Serialize/Deserialize under the `serde` feature (so downstream `config.json` deserializes directly). Accepts string or numeric decimals; `up_to: null` → `None`.

## 3. Crate housekeeping
- `lib.rs`: `pub mod tax;` + `pub use tax::{TaxBracket, TaxConfig, TaxEstimate};`.
- Inline unit tests (`#[cfg(test)] mod tests` in `src/tax.rs`): default preset; short flat on gains only; short loss → 0; long progressive across all brackets; threshold reclassification (400-day hold long@365 / short@500); Average `acquired_at:None` fallback (term None → short, term Long → long); empty report.
- `tests/serde_roundtrip.rs`: `TaxConfig` JSON round-trip + string-decimal/`null`-`up_to` parsing.
- `examples/tax_brackets.rs`: end-to-end `Portfolio::tax_estimate` demo.
- README: "Tax estimation" section (default preset, `tax_estimate`, threshold semantics, "not tax advice").
- `Cargo.toml`: version → `0.2.0`.

## 4. Release
Full gates: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, `cargo doc --no-deps --all-features` (missing-docs gate), `cargo publish --dry-run`. **Jacob runs the real `cargo publish`** and confirms 0.2.0 is live before Part C's tax tasks. Commits use author `51518860+Technical-1@users.noreply.github.com`, no AI attribution.

## 5. Testing strategy
Pure unit + doc + serde tests as above; `--all-features` doc build enforces docs. No network.

## 6. Risks
- **Publish gating:** Part C tax work can't compile until 0.2.0 is on crates.io. Fallback: temporary `[patch.crates-io]` to local path, removed before final commit.
- **Threshold vs report subtotals:** the estimate's subtotals may differ from `CapitalGainsReport.short_term_gain/long_term_gain` (always 365-based) when threshold ≠ 365 — documented; the estimate's own subtotals are authoritative for the tax number.
