# Part C — Crypto-Price-Tracker-V2 Consumption — Design Spec

**Author:** Jacob Kanfer · **Date:** 2026-06-03 · **Status:** Approved (design); pending spec review

Part C of the 3-part parity effort. Wires the new engines into the V2 TUI and closes the remaining Python gaps. Repo: `~/CodeRepos/Crypto-Price-Tracker-V2`. **Gated on** Part A (`coinbasis 0.2.0`) and Part B (`cryptolytics 0.1.0`) being published.

Four independently-shippable sub-parts:
- **C1 — Tax brackets** (consume coinbasis 0.2 tax)
- **C2 — CSV import**
- **C3 — Rebalance analytics** (consume cryptolytics + new market-history fetch)
- **C4 — History reconstruction & playback** (consume cryptolytics + coinbasis as-of replay)

Shared infra (the `fetch_history` price path + the fetch-result channel change) is built in C3 and reused by C4.

## Cross-cutting: dependencies & numeric boundary
- `Cargo.toml`: `coinbasis = { version = "0.2", features = ["serde"] }`, add `cryptolytics = "0.1"`.
- **f64/Decimal boundary:** V2 stays `Decimal` for money/positions; cryptolytics is `f64`. Convert `Decimal → f64` (`.to_f64().unwrap_or(0.0)`) at each cryptolytics call site and back to `Decimal`/string for display. coinbasis tax stays `Decimal` end-to-end (no conversion).

---

## C1 — Tax brackets

- `src/config.rs`: delete V2's local `TaxConfig`; `Config.tax` becomes `coinbasis::tax::TaxConfig` (deserialized straight from `config.json`). `Config::example()` uses `coinbasis::tax::TaxConfig::default()`. Update `config.example.json` `tax` to the bracket preset (`jurisdiction`, `long_term_threshold_days`, `short_term_rate`, `long_term_brackets:[{up_to,rate}…]`, `up_to:null` for the top).
- `src/ui/tax.rs`: replace the flat-rate estimate with `coinbasis::tax::estimate(cg, &app.config.tax)`; render short/long gain + tax, total, and the `jurisdiction` label. Income line unchanged.
- Tests: config parses brackets; `Config::example` round-trips; `TestBackend` asserts the bracketed estimate + jurisdiction render (short-only and long-bracketed fixtures).

## C2 — CSV import

- `src/ledger.rs`: `pub fn import_csv(csv_path: &str, ledger_path: &str) -> Result<(usize, usize), AppError>`. Header `date,coin,action,quantity,price_usd,fee_usd` + optional trailing `wallet`. Validate (date ISO `YYYY-MM-DD`; coin non-empty; action buy/sell case-insensitive; quantity > 0; price ≥ 0; fee ≥ 0, blank→0; wallet blank/absent→`"default"`). Map buy/sell → `coinbasis::Transaction::Buy/Sell` (timestamp = date at `00:00:00Z`). Dedup against existing ledger (exact `Transaction` `PartialEq`); append new; `save_ledger`. Invalid rows skipped with stderr notice + line number; return `(added, skipped)`.
- `src/main.rs`: clap `--import <FILE>` (`Option<String>`); when set, run `import_csv` against the resolved ledger path, print `imported N, skipped M (ledger: …)`, and **return before terminal setup** (no TUI).
- Ship `transactions.example.csv` (with optional `wallet` column shown); README "CSV import" section.
- Tests: `import_csv` happy path (buy+sell), optional/default wallet, dedup, invalid-row skip, blank-fee default, missing file; round-trip via `load_ledger`.

## C3 — Rebalance analytics

### C3.1 Market-history fetch (shared infra)
- Extend the `PriceSource` trait (`src/prices/mod.rs`):
  ```rust
  async fn fetch_history(&self, ids: &[String], vs: &str, days: u32)
      -> Result<HashMap<String, Vec<(DateTime<Utc>, f64)>>, AppError>;
  ```
  - `CoinGeckoSource`: one `coin_market_chart(id, vs, days as i64, true)` call **per id**, sequentially (throttled, best-effort — a per-id failure is skipped, not fatal); returns daily `(timestamp, price)` series. Empty `ids` → empty map.
  - `MockSource`: returns a configured deterministic series per id (a `set_history(id, Vec<(DateTime,f64)>)` test helper).
- **Channel change** (`src/main.rs`): the fetch→app mpsc channel carries an enum:
  ```rust
  enum FetchMsg { Prices(Result<PriceBook, String>), History(Result<HistoryData, String>) }
  type HistoryData = HashMap<String, Vec<(DateTime<Utc>, f64)>>;
  ```
  The run loop matches both arms (Prices arm = today's behavior; History arm sets `app.set_history(...)`).
- **History fetch trigger:** on startup and on manual `r` refresh (NOT the periodic tick — `coin_market_chart` is per-coin and rate-limit-sensitive). Spawned as a separate task feeding `FetchMsg::History`. `--offline` skips it (history seeded from nothing; views show "no history yet").
- **Window:** `Config.history_days: u32` (new field, default 90). `config.example.json` adds `"history_days": 90`.
- **Market caps:** reuse `PriceBook.quotes[id].market_cap` (already fetched via `coins_markets`) — no new request.

### C3.2 App state & recompute
- `App` gains: `history: HistoryData` (per-coin dated price series), `target_strategy: cryptolytics::TargetStrategy` (default `Custom`).
- `Derived` gains (for the Rebalance view): `vols_daily: BTreeMap<String,f64>`, `vols_annual: BTreeMap<String,f64>`, `correlation: BTreeMap<(String,String),f64>`, `portfolio_vol: Option<f64>`, `backtest_current: Option<f64>`, `backtest_target: Option<f64>`.
- `recompute()` additions (when method/prices/history/target_strategy change):
  - Build per-coin **return series** from `history` via `cryptolytics::returns::daily_returns` (prices→f64).
  - `vols_daily[c] = cryptolytics::volatility::volatility(&returns)`; `vols_annual[c] = annualize(daily, 365.0)`.
  - `correlation = cryptolytics::correlation::correlation_matrix(&returns_by_coin)` (when ≥2 coins have history).
  - `portfolio_vol = cryptolytics::portfolio::portfolio_volatility(value-weights, vols_daily, correlation)` (≥2 coins).
  - **Target weights:** `cryptolytics::allocation::target_weights(target_strategy, assets, market_caps?, custom?)` where `custom = config.targets` (as f64), `market_caps` from quotes. Convert the resulting f64 weights → `Decimal` and feed the existing `rebalance::suggest(...)` (Band/Full drift, tax-aware sell estimates) unchanged. (So target *source* = strategy; trade *aggressiveness* = Band/Full — orthogonal.)
  - **Backtest:** `cryptolytics::backtest::buy_and_hold_return(history_prices, current_weights)` and `(…, target_weights)` over the window.
- `event.rs`: add `Action::CycleTargetStrategy` (key `w`) → cycles Equal→MarketCap→Custom→Equal, triggers recompute. (Existing `t` Band/Full and `r` refresh unchanged; `r` now also triggers a history fetch.)

### C3.3 Rebalance view (`src/ui/rebalance.rs`)
Add, above/below the existing banner + current-vs-target + trades table:
- **Strategy line:** active target strategy (`Equal`/`MarketCap`/`Custom`) + Band/Full + `(w: weighting, t: trade)`.
- **Risk panel:** per-coin `Daily %` / `Annual %` (from `vols_daily`/`vols_annual`); a "Portfolio daily volatility: X%" line when `portfolio_vol` is `Some`; "(need ≥2 coins with history)" otherwise.
- **Correlation matrix:** labeled matrix from `correlation` when present.
- **Backtest line:** `Backtest (Nd, buy&hold): current X% / target Y%`.
- Tests: `TestBackend` asserts the new panels render with a `MockSource` history fixture; empty-history degrades gracefully.

## C4 — History reconstruction & playback

### C4.1 Reconstruction (`src/perf.rs`)
- Extend `Snapshot` to `{ at: DateTime<Utc>, total_value: Decimal, cost: Decimal, pl: Decimal }` (add `cost`, `pl`; `#[serde(default)]` so old `history.json` rows still load). `record_snapshot` now also writes cost/pl (cost/pl come from the current valuation: `total_cost`, `total_unrealized`).
- New: `reconstruct_series(txs: &[Transaction], history: &HistoryData, method: CostBasisMethod) -> Vec<Snapshot>`:
  - Collect the sorted set of dates present in `history`.
  - For each date `d`: `txs_upto = txs where timestamp ≤ end-of-day(d)`; `day_prices: HashMap<String,Decimal>` = each coin's price on `d` (the matching `(date,price)` from `history`, f64→Decimal); `Portfolio::from_transactions(&txs_upto)?.valuation(method, &day_prices)?` → `Snapshot { at: d, total_value, cost: total_cost, pl: total_unrealized }`. Coins lacking a price on `d` are simply absent from `day_prices` (excluded that day, per coinbasis `missing_prices`).
  - Returns the daily series (the replay *is* the as-of holdings — no special coinbasis API needed).

### C4.2 Performance view (`src/ui/perf.rs`) + state
- `App` gains `perf_mode: PerfMode { Chart, Playback }` (default `Chart`) and reuses `selected` as the playback day cursor.
- **Data source:** when `history` is non-empty, the Performance view uses `reconstruct_series(...)` for its value/P&L series and metrics (existing `coinbasis::stats`/`cryptolytics` metrics over the reconstructed value series); otherwise it falls back to the forward-recorded `history.json` snapshots (today's behavior). Reconstruction result cached in `Derived` (`reconstructed: Vec<Snapshot>`), recomputed when method/history change.
- **Chart mode** (default): existing value+P/L sparkline/line chart + metrics, now over the reconstructed series.
- **Playback mode** (key `p` → `Action::TogglePlayback`): a scrollable **day-by-day list** — `date · value · P/L · ⟨P/L hbar⟩` per day — with the `selected` day highlighted; a **detail pane** shows the selected day's **per-coin breakdown** (asset · qty · price · value · P/L) computed from `txs_upto` valuation at that day. ↑/↓ move the day cursor. (This folds Python's `--play` and `--date` into one interactive mode; nearest-day fallback is unnecessary since only real history days are listed.) News-date markers are deferred to Phase 5 (News).
- `event.rs`: `Action::TogglePlayback` (`p`); ↑/↓ already mapped to selection.
- Tests: `reconstruct_series` unit tests (ledger replay across dates, coin missing a day's price, buy after a day excluded); `TestBackend` for chart mode over a reconstructed fixture and playback mode (day list + breakdown pane).

## Build ordering (for the plan)
C1 and C2 (depend only on coinbasis 0.2) can land first. C3.1 (fetch_history + channel enum) precedes C3.2/C3.3 and C4. C4 depends on C3.1's `fetch_history`. Each sub-part ends green (`fmt --check`, `clippy -D warnings`, `cargo test`).

## Testing strategy
Pure domain additions (`import_csv`, `reconstruct_series`, recompute analytics wiring) unit-tested with fixtures; `MockSource.set_history` drives history-dependent tests with no network; all views via `TestBackend`; the run-loop `fetch_history` path is verified by `cargo build` + the existing integration test extended with a `MockSource` history.

## Risks / notes
- **Rate limits:** per-coin `coin_market_chart` on startup/refresh can hit CoinGecko's public limit for large portfolios; mitigated by best-effort per-coin skips and history only on startup/`r` (not the tick). A future cache (like the price cache) is out of scope here.
- **Reconstruction cost:** rebuilding a `Portfolio` per day is O(days × ledger); fine for ~90 days and typical ledgers.
- **Snapshot format migration:** `#[serde(default)]` on new `cost`/`pl` keeps old `history.json` readable.
- **Publish gating:** C compiles only after A+B are on crates.io (or via temporary `[patch.crates-io]` path deps, removed before final commits).

## Cross-ecosystem parity (vs the Python app)
Validated 2026-06-03 against `~/.claude/tmp/crypto-python-parity/phase3-app/`. Confirmed parity: all 6 views, the Fifo/Lifo/Hifo/Average method switcher (SpecificId excluded from the live cycle in both), ledger schema (coinbasis 8-event multi-wallet), tax brackets, tax-aware rebalance, caching + offline. Two genuine **V2 gaps** the Python side exposes, plus cosmetic divergences:

- **GAP — CoinGecko API key + Demo/Pro + keyed-fallback (decision pending):** V2 only ever constructs `CoinGeckoClient::new(COINGECKO_API_DEMO_URL)` (keyless demo, no key, no Pro, no 429-retry-with-key). The Python client supports `COINGECKO_API_KEY` + plan (`demo`/`pro`) URL/header routing + keyless→429→keyed retry. If chosen, add to Part C: read an API key + plan from config/env, construct `CoinGeckoSource` accordingly (`new_with_demo_api_key` / `new_with_pro_api_key`), and optionally retry-with-key on 429. Folds naturally into C3 (already touching the prices layer).
- **DIVERGENCE — config surface (decision pending):** V2 nests everything in one `config.json` (CWD); Python uses separate files (`taxconfig.json`, `targets.json`, `staking.json`, `news.json`) + XDG `~/.config` + env. Cosmetic (no numeric impact); align V2 to the split/XDG layout only if cross-ecosystem config parity is wanted.
- **ALIGNMENT (applied in C4):** Python snapshots are `snapshots.jsonl` (JSONL, `{date, total_value, cost, pl}`). C4 already adds `cost`/`pl`; for parity, C4 will also rename the field to `date` and write **JSONL** (one object per line) instead of V2 v0.1's JSON array — append-friendlier and matches Python. (Old `history.json` array still readable via a one-time tolerant load.)
- **CONFIRMED intentional:** staking + news remain V2 roadmap Phases 4–5 (master §8); the broad-vs-pure `cryptolytics` scope split is per master §1 (see Part B §8).
