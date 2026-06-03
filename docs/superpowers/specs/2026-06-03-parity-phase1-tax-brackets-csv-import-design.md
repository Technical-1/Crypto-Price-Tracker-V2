# Parity Phase 1 — Tax Brackets (coinbasis) + CSV Import (V2) — Design Spec

**Author:** Jacob Kanfer
**Date:** 2026-06-03
**Status:** Approved (design); pending spec review

> Part of the Crypto-Price-Tracker-V2 → Python feature-parity effort. This is Phase 1 of 5. It spans **two repos**: `~/CodeRepos/coinbasis` (the published tax engine) and `~/CodeRepos/Crypto-Price-Tracker-V2` (the TUI app).

---

## 1. Goal

Close two parity gaps vs the original Python app:

1. **Progressive tax brackets.** Python's `tax` report applies a flat short-term rate plus **progressive long-term brackets** with a configurable holding-period threshold and a jurisdiction label (from `taxconfig.json`). V2 currently applies two flat rates (`short_term_rate`, `long_term_rate`). The bracket math belongs in **coinbasis** (the crypto-tax engine), not the app.
2. **CSV ledger import.** Python imports transactions from `date,coin,action,quantity,price_usd,fee_usd` CSV. V2 only reads a hand-edited JSON ledger. Add a CSV importer to V2 (ingestion is app-level, not tax math).

Interactive `add` remains a non-goal (spec §3 of the V2 design).

## 2. Scope / Non-Goals

**In scope:**
- coinbasis: a new `tax` module (`TaxConfig`, `TaxBracket`, `TaxEstimate`, `tax::estimate`, `Portfolio::tax_estimate`); bump to **0.2.0**; publish to crates.io.
- V2: depend on coinbasis 0.2; replace the flat-rate tax config with `coinbasis::tax::TaxConfig`; render the bracketed estimate in the Tax view; add `--import <FILE.csv>`.

**Non-goals:**
- No change to coinbasis's existing 365-day `Term` classification or any existing public signature (purely additive → 0.2.0 is backward-compatible).
- No interactive transaction entry in the TUI.
- No multi-jurisdiction rule engine — `jurisdiction` is a display label; the math is the configured short rate + long brackets.

## 3. coinbasis: the `tax` module

### 3.1 Types (in `src/tax.rs`, `pub mod tax`)

```rust
/// One long-term capital-gains tax bracket. `up_to` is the cumulative gain
/// ceiling for this bracket (None = unbounded top bracket).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaxBracket {
    pub up_to: Option<Decimal>,
    pub rate: Decimal,
}

/// Tax-rate configuration. Mirrors the Python `taxconfig.json` shape.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaxConfig {
    pub jurisdiction: String,
    pub long_term_threshold_days: i64,
    pub short_term_rate: Decimal,
    pub long_term_brackets: Vec<TaxBracket>,
}

/// The result of applying a `TaxConfig` to a `CapitalGainsReport`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaxEstimate {
    pub short_term_gain: Decimal,
    pub long_term_gain: Decimal,
    pub short_term_tax: Decimal,
    pub long_term_tax: Decimal,
    pub total_tax: Decimal,
}
```

`TaxConfig::default()` returns a US-ish preset matching Python's shipped config:
`jurisdiction: "default"`, `long_term_threshold_days: 365`, `short_term_rate: 0.35`,
`long_term_brackets: [{up_to: 47025, rate: 0.0}, {up_to: 518900, rate: 0.15}, {up_to: None, rate: 0.20}]`.

### 3.2 Estimation function

```rust
pub fn estimate(report: &CapitalGainsReport, config: &TaxConfig) -> TaxEstimate
```

Behavior:
1. **Re-derive short/long subtotals from the report's rows under the configured threshold** (so a non-365 threshold changes the split without touching the 365-based `Term`):
   - For each `RealizedGain` row: if `acquired_at` is `Some`, compute `held = (disposed_at - acquired_at).num_days()`; classify **long** iff `held > long_term_threshold_days`, else **short**.
   - If `acquired_at` is `None` (e.g. `Average` method), fall back to the row's existing `term`: `Some(Term::Long) → long`, else short.
   - Sum `gain` into `short_term_gain` / `long_term_gain` accordingly.
2. **Short-term tax** = `max(0, short_term_gain) * short_term_rate`.
3. **Long-term tax** = progressive over `long_term_brackets` applied to `g = max(0, long_term_gain)`:
   - Walk brackets in order, tracking `prev` (previous `up_to`, starting at 0).
   - For each bracket, `ceiling = up_to.unwrap_or(g)`; `slice = clamp(min(g, ceiling) - prev, 0, ∞)`; `tax += slice * rate`; `prev = ceiling`; stop once `prev >= g`.
   - Brackets are assumed sorted ascending with the unbounded (`None`) bracket last; a `None` mid-list is treated as the final bracket.
4. `total_tax = short_term_tax + long_term_tax`. Losses never produce negative tax.

### 3.3 Convenience method on `Portfolio`

```rust
impl Portfolio {
    pub fn tax_estimate(
        &self, method: CostBasisMethod, tax_year: i32, config: &TaxConfig,
    ) -> Result<TaxEstimate, PortfolioError> {
        let report = self.capital_gains_report(method, tax_year)?;
        Ok(tax::estimate(&report, config))
    }
}
```

### 3.4 Crate housekeeping
- `lib.rs`: `pub mod tax;` + re-export `pub use tax::{TaxBracket, TaxConfig, TaxEstimate};`.
- `#![deny(missing_docs)]` is on — every new public item needs a doc comment with a runnable doc-example (matches the crate's existing style).
- Add `tests/` coverage: short-only, long progressive across all brackets, losses → 0, threshold reclassification (e.g. a 400-day hold long at 365 but short at 500), `Average`-method `acquired_at: None` fallback, empty report.
- Add an `examples/tax_brackets.rs` (or extend `examples/tax_reports.rs`) demonstrating `tax_estimate`.
- Bump `version = "0.2.0"` in `Cargo.toml`; update README with a tax-estimate section.
- **Publish:** I prepare the release and run `cargo publish --dry-run` + `cargo package`; **Jacob runs/approves the real `cargo publish`.** Commits in coinbasis use the same author rules (`51518860+Technical-1@users.noreply.github.com`, no AI attribution).

## 4. V2: consume coinbasis 0.2 tax

### 4.1 Config migration
`src/config.rs` `Config.tax` changes from the local `TaxConfig { short_term_rate: f64, long_term_rate: f64 }` to `coinbasis::tax::TaxConfig` (deserialized straight from `config.json`, exactly as `default_method` deserializes into `coinbasis::CostBasisMethod`). Delete V2's local `TaxConfig`.

`config.example.json` `tax` becomes:
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
}
```
(rust_decimal serde accepts string or number; strings preserve exactness.)

### 4.2 Tax view
`src/ui/tax.rs` replaces the flat-rate "Estimated Tax" line. It calls `coinbasis::tax::estimate(cg, &app.config.tax)` (or the new `app.model` convenience) and renders:
```
Short-term gain: {+.2}    tax: {.2}
Long-term gain:  {+.2}    tax: {.2}
Income:          {.2}              (unchanged — from income report)
Estimated Tax:   {total_tax:.2}  ({jurisdiction}, brackets)
```
The estimate uses the report already in `app.derived.capital_gains`, so no new fetch/recompute path is needed. `app.rs` may cache a `TaxEstimate` in `Derived` alongside `capital_gains` (recomputed in `recompute()`), or the view computes it inline — implementation detail for the plan.

### 4.3 Dependency
`Cargo.toml`: `coinbasis = { version = "0.2", features = ["serde"] }`. V2 work that consumes the new API is gated on the coinbasis 0.2.0 publish landing on crates.io.

## 5. V2: CSV import

### 5.1 CLI
`src/main.rs` clap `Args` gains `--import <FILE>` (`Option<String>`). When present, V2 runs the import against the configured ledger path, prints `imported N, skipped M (ledger: <path>)`, and **exits before terminal setup** (no TUI). Errors propagate as `anyhow` with a non-zero exit (terminal never entered).

### 5.2 Importer
`src/ledger.rs` gains:
```rust
pub fn import_csv(csv_path: &str, ledger_path: &str) -> Result<(usize, usize), AppError>;
```
- CSV header (case-sensitive): `date,coin,action,quantity,price_usd,fee_usd` and an **optional** trailing `wallet` column.
- Per row, validate (mirrors Python): `date` ISO `YYYY-MM-DD`; `coin` non-empty; `action` ∈ {buy, sell} (case-insensitive); `quantity` numeric > 0; `price_usd` numeric ≥ 0; `fee_usd` numeric ≥ 0 (default 0 if blank/missing); `wallet` defaults to `"default"` if blank/absent.
- Map to `coinbasis::Transaction`:
  - `buy → Transaction::Buy { timestamp, wallet, asset: coin, quantity, unit_price: price_usd, fee: fee_usd }`
  - `sell → Transaction::Sell { …same fields… }`
  - `timestamp = NaiveDate(date).and_hms(0,0,0)` as `DateTime<Utc>`.
- **Dedup:** load existing ledger (via `load_ledger`, tolerating a missing file as empty); skip any row whose mapped `Transaction` already exists (exact `PartialEq` match). Append new ones and `save_ledger`.
- Invalid rows are skipped with a stderr notice including the line number; import continues. Return `(added, skipped)`.
- Hand-rolled CSV parsing is acceptable (simple comma split on a known header) — consistent with V2's hand-rolled CSV export; no new dependency. Quoted fields/commas-in-fields are out of scope (Python doesn't handle them either).

### 5.3 Sample file
Ship `transactions.example.csv` in the V2 repo mirroring Python's template (with a `wallet` column shown as optional in a comment in the README).

## 6. Testing Strategy
- **coinbasis** (`tests/` + doc-examples): the §3.4 cases. `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`, `cargo doc` (missing-docs gate).
- **V2 config:** parse the new bracket `tax` config; `Config::example()` round-trips.
- **V2 tax view:** `TestBackend` asserts the bracketed estimate line + jurisdiction render; a short-only and a long-bracketed fixture.
- **V2 import:** unit tests for `import_csv` — happy path (buy+sell), optional wallet column, default wallet, dedup against existing ledger, invalid-row skip, missing fee defaulting, missing file. Round-trip: import then `load_ledger` equals expected.
- All V2 gates: `fmt --check`, `clippy -D warnings`, `cargo test`.

## 7. Build Ordering (for the plan)
1. **coinbasis** `tax` module: types → `estimate` → `Portfolio::tax_estimate` → tests/doc-examples → README → 0.2.0 bump → `cargo publish --dry-run` → **(Jacob publishes)**.
2. **V2** dependency bump to `coinbasis = "0.2"` once published.
3. **V2** config migration to `coinbasis::tax::TaxConfig` + `config.example.json`.
4. **V2** Tax view bracketed estimate.
5. **V2** CSV import (`import_csv` + `--import` flag + sample file + README).

Steps 2–5 are gated on step 1's publish. Steps 3–5 are otherwise independent and can interleave.

## 8. Risks / Notes
- **Publish gating:** V2 can't compile against the new API until coinbasis 0.2.0 is on crates.io (per the chosen "publish immediately" approach). If iteration speed becomes painful, a temporary `[patch.crates-io]` to the local path is a fallback, removed before final commit. (Default plan assumes publish-first.)
- **Decimal/serde:** `TaxConfig` uses `Decimal`; V2 config strings preserve exactness; coinbasis `serde` feature must be enabled (it is, in V2's dep).
- **Threshold semantics:** documented as estimate-level reclassification; the report's own `short_term_gain`/`long_term_gain` (always 365-based) may differ from the estimate's subtotals when threshold ≠ 365 — the Tax view shows the estimate's subtotals for consistency with the tax number.
