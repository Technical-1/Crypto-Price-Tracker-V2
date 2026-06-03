# Crypto-Price-Tracker-V2 — Design Spec

**Author:** Jacob Kanfer
**Date:** 2026-06-03
**Status:** Approved (pending spec review)

> **Note:** This `docs/` directory holds the design/implementation planning for V2 and will be removed from the repository once the build is complete and verified.

---

## 1. Goal

Build **Crypto-Price-Tracker-V2**, a fast, full-featured Rust **terminal UI (TUI)**
for tracking a crypto portfolio: live prices and per-coin profit/loss, cost-basis
and tax reporting, portfolio valuation and allocation, rebalancing suggestions,
and historical performance statistics. It is the successor to the Python
Crypto-Price-Tracker CLI, with all cost-basis/tax math delegated to the published
[`coinbasis`](https://crates.io/crates/coinbasis) crate rather than reimplemented,
and live prices from the [`coingecko`](https://crates.io/crates/coingecko) crate.

## 2. Background

- **V1** is a Python CLI that fetches CoinGecko prices and prints a per-coin
  profit table, with bolt-on tax/cost-basis and rebalancing. V2 is a ground-up
  Rust rewrite as an interactive TUI. V1's CSV/data formats are **not** carried
  over.
- **`coinbasis` 0.1** (just published) provides the cost-basis engine: FIFO/LIFO/
  HIFO/Average/Specific-ID, per-wallet lots, realized gains, income, holdings,
  valuation, and Form-8949-shaped tax-year reports, plus pure portfolio
  statistics. V2 is a presentation/orchestration layer over it.
- **`coingecko` 1.x** is an async (reqwest 0.12 + tokio 1.36) client for the
  CoinGecko V3 API. V2 uses it for current prices, 24h/7d change, market data,
  and short price history (sparklines).

## 3. Scope (v0.1) and Non-Goals

**In scope (v0.1 ships all six views):**
1. Live prices / profit & loss table
2. Holdings (open lots per wallet)
3. Valuation & allocation
4. Tax (capital-gains + income, per year, with estimated tax + export)
5. Rebalancing suggestions (tax-aware)
6. Performance / statistics over a persisted value history
7. A global **cost-basis method switcher** that recomputes every view
8. Full transaction ledger (all 8 `coinbasis` event types, multi-wallet, JSON)
9. Price caching + rate-limit handling + offline last-good fallback
10. Config file, exports, help overlay, graceful error/empty states

**Non-goals (v0.1):**
- No trade execution / exchange API writes (read-only analysis tool).
- No portfolio editing inside the TUI beyond what's needed (ledger is edited as
  JSON; in-TUI "add transaction" is a future enhancement).
- No multi-fiat cost basis (cost basis is USD per `coinbasis`; display currency
  may differ for *current price* readouts only — see §10).
- No web/GUI; terminal only.

## 4. Tech Stack

| Concern | Choice | Rationale |
|---|---|---|
| Language | Rust 2021 | Performance, safety, shares the `coinbasis` ecosystem |
| TUI | `ratatui` + `crossterm` | The de-facto Rust TUI stack; `crossterm` is the portable backend |
| Async runtime | `tokio` | Required by `coingecko`; drives the price-fetch task |
| Prices | `coingecko` 1.x | Maintained async CoinGecko V3 client |
| Cost basis / tax | `coinbasis` 0.1 (feature `serde`) | Our engine; serde lets the ledger deserialize straight into `Transaction`s |
| Money | `rust_decimal` | Exact amounts end-to-end; matches `coinbasis` |
| Serialization | `serde`, `serde_json` | Ledger, config, value-history persistence |
| Time | `chrono` | Timestamps for ledger + snapshots |
| Errors | `anyhow` (app) + `thiserror` (typed domain errors) | `anyhow` for top-level ergonomics, `thiserror` where callers branch on error kind |
| CLI args | `clap` | `--config`, `--ledger`, `--offline`, etc. |
| Testing | built-in + `ratatui::TestBackend` + a mock price source | Pure domain logic + rendered-buffer assertions |

## 5. Architecture

**Principle: pure domain core + thin TUI.** All computation (ledger loading,
pricing, portfolio wrapping, rebalancing, statistics) is plain, synchronous,
testable Rust with no terminal dependencies. The TUI is a render-and-events shell
on top. Network I/O happens in one async task that pushes results to app state
through a channel; rendering reads a consistent snapshot of state each frame.

**Single binary crate** with this module layout:

```
crypto-price-tracker-v2/
├── Cargo.toml
├── README.md
├── config.example.json
├── src/
│   ├── main.rs        # tokio runtime, CLI args, terminal setup/teardown, run loop
│   ├── error.rs       # AppError (thiserror) for domain failures
│   ├── config.rs      # Config: coin-id map, targets, tax rates, refresh, defaults
│   ├── ledger.rs      # load/save Vec<coinbasis::Transaction> as JSON; derive assets
│   ├── prices/
│   │   ├── mod.rs     # PriceSource trait, Quote type, PriceBook
│   │   ├── coingecko.rs # CoinGeckoSource (async) — current prices, 24h/7d, market chart
│   │   ├── cache.rs   # on-disk price/market-chart cache with TTL
│   │   └── mock.rs    # MockSource for tests
│   ├── portfolio.rs   # PortfolioModel: wraps coinbasis queries under a chosen method
│   ├── rebalance.rs   # drift vs targets → suggested trades (tax-aware)
│   ├── perf.rs        # value-history snapshots + coinbasis::stats metrics
│   ├── export.rs      # CSV/JSON export of tax + holdings reports
│   ├── app.rs         # App state: active view, method, tax year, sort, selection, status
│   ├── event.rs       # crossterm event loop + refresh tick; maps input → app actions
│   └── ui/
│       ├── mod.rs     # frame layout, tab bar, status bar, help overlay dispatch
│       ├── prices.rs  # view 1
│       ├── holdings.rs# view 2
│       ├── valuation.rs # view 3
│       ├── tax.rs     # view 4
│       ├── rebalance.rs # view 5
│       └── perf.rs    # view 6
└── tests/             # integration tests (TestBackend snapshots, end-to-end domain)
```

**Async model:** `main` starts the tokio runtime, builds the initial `App` from
config + ledger, then runs a loop that: (a) reads crossterm input events
(non-blocking, via `crossterm::event::poll`/`EventStream`), (b) on a refresh tick
or manual `r`, spawns/awaits a price fetch through the `PriceSource`, (c) updates
`App` state (prices, recomputed reports), (d) renders the active view. Network
results never block input; a fetch in flight shows a spinner and the previous
prices remain visible.

## 6. Data Model

### 6.1 Ledger (`ledger.json`)
A JSON array of `coinbasis::Transaction` values (the crate's `serde` feature
provides the representation). Example:
```json
[
  { "Buy": { "timestamp": "2021-01-01T00:00:00Z", "wallet": "coinbase",
             "asset": "bitcoin", "quantity": "0.5", "unit_price": "30000", "fee": "5" } },
  { "Income": { "timestamp": "2021-06-01T00:00:00Z", "wallet": "kraken",
                "asset": "ethereum", "quantity": "1.2", "value": "2400", "source": "Staking" } }
]
```
- `asset` strings are the **CoinGecko coin ids** (e.g. `bitcoin`, `ethereum`) so
  pricing needs no separate mapping for assets that are already ids; the config
  symbol map (§6.2) covers display symbols and any aliases.
- Amounts are decimal strings (rust_decimal serde). `ledger.rs` reads the file,
  deserializes to `Vec<Transaction>`, and surfaces a clear error on malformed
  input (it does NOT call `coinbasis` validation directly — `PortfolioModel`
  does that at construction).

### 6.2 Config (`config.json`)
```json
{
  "ledger_path": "ledger.json",
  "default_method": "Fifo",
  "display_currency": "usd",
  "refresh_seconds": 60,
  "tax": { "short_term_rate": 0.24, "long_term_rate": 0.15 },
  "targets": { "bitcoin": 0.6, "ethereum": 0.3, "solana": 0.1 },
  "rebalance": { "band": 0.05, "min_trade_usd": 25, "strategy": "Band" },
  "symbols": { "bitcoin": "BTC", "ethereum": "ETH", "solana": "SOL" },
  "cache": { "ttl_seconds": 30, "dir": "~/.cache/crypto-price-tracker-v2" }
}
```
- `targets` weights should sum to ~1.0 (validated; a non-sum is reported, not
  fatal — it normalizes with a warning).
- `tax` rates drive the *estimated* tax figure only; they are user-supplied and
  clearly labeled an estimate.

### 6.3 Value history (`history.json`)
Append-only snapshots used by the Performance view:
```json
[ { "at": "2026-06-01T12:00:00Z", "total_value": "41230.55" }, ... ]
```
A snapshot is appended when a successful valuation is computed and at most once
per `refresh_seconds` (deduped by timestamp bucket). `perf.rs` reads the values
as `f64` for `coinbasis::stats`.

## 7. Module Specifications

### 7.1 `config`
- **Type:** `Config` (+ nested `TaxConfig`, `RebalanceConfig`, `CacheConfig`).
- **Interface:** `Config::load(path) -> Result<Config>`, `Config::example() ->
  Config` (for `config.example.json`), helpers `symbol(asset) -> &str`,
  `target_weight(asset) -> Decimal`.
- **Depends on:** serde, rust_decimal.

### 7.2 `ledger`
- **Interface:** `load_ledger(path) -> Result<Vec<Transaction>>`,
  `save_ledger(path, &[Transaction])`, `assets(&[Transaction]) -> BTreeSet<String>`
  (distinct asset ids, to know what to price).
- **Depends on:** coinbasis (serde), serde_json.

### 7.3 `prices`
- **Types:**
  - `Quote { price: Decimal, change_24h: Decimal, change_7d: Option<Decimal>,
    market_cap: Option<Decimal>, volume_24h: Option<Decimal> }`
  - `PriceBook { quotes: HashMap<String, Quote>, fetched_at: DateTime<Utc>,
    sparklines: HashMap<String, Vec<f64>> }` and a helper
    `price_map() -> HashMap<String, Decimal>` for `coinbasis::valuation`.
- **Trait:** `#[async_trait] trait PriceSource { async fn fetch(&self, ids: &[String],
  vs: &str) -> Result<PriceBook>; }`
- **Impls:** `CoinGeckoSource` (wraps `coingecko::CoinGeckoClient`; one batched
  simple-price call for prices + 24h change + market cap + volume; optional
  per-asset market-chart calls for 7d sparklines, rate-limited and cached);
  `MockSource` (deterministic quotes for tests).
- **Cache:** `cache.rs` stores the last `PriceBook` and market charts on disk with
  a TTL; on a fetch within TTL it serves cache; on network failure it serves the
  last good cache and flags staleness.
- **f64/Decimal boundary:** CoinGecko returns `f64`; convert to `Decimal` via
  `Decimal::from_f64_retain(x).unwrap_or_default()` at this boundary so the rest
  of the app is exact. Sparkline series stay `f64` (chart only).

### 7.4 `portfolio`
- **Type:** `PortfolioModel { portfolio: coinbasis::Portfolio }` built once from
  the ledger (`Portfolio::from_transactions`, surfacing validation errors).
- **Interface (all take the active `CostBasisMethod`, except where the crate
  doesn't):**
  - `holdings(method) -> Result<Vec<Holding>>`
  - `realized_gains(method) -> Result<Vec<RealizedGain>>`
  - `capital_gains(method, year) -> Result<CapitalGainsReport>`
  - `income(year) -> IncomeReport`
  - `valuation(method, &price_map) -> Result<PortfolioReport>`
- **Specific-ID note:** v0.1's method switcher cycles the four automatic methods
  (Fifo/Lifo/Hifo/Average) for live recomputation; Specific-ID requires a
  caller-supplied selection and is out of scope for the interactive switcher
  (documented; the engine still supports it for future use).
- **Depends on:** coinbasis.

### 7.5 `rebalance`
- **Input:** the `PortfolioReport` (current per-asset market values + total),
  target weights, `band`, `min_trade_usd`, `strategy` (`Band` | `Full`).
- **Output:** `Vec<RebalanceAction { asset, current_value, target_value, drift,
  side: Buy|Sell, amount_usd }>` plus a summary (total buys/sells, whether in
  balance).
- **Logic:** target_value = total × weight; drift = current − target. `Full`
  proposes a trade for every asset off-target by ≥ `min_trade_usd`; `Band` only
  for assets whose |drift| / total exceeds `band`. Sells are capped at the held
  value.
- **Tax-aware extra:** for each suggested **sell**, estimate the realized gain if
  executed, by drawing the sold value from that asset's lots under the
  gain-minimizing method (HIFO) using a `coinbasis` simulation (append a
  hypothetical `Sell` and read the realized gain). Display the estimate and note
  HIFO minimizes it. This is an *estimate*, clearly labeled.
- **Pure** and unit-tested with fixed inputs.

### 7.6 `perf`
- **Interface:** `record_snapshot(path, total_value, now)` (deduped append);
  `load_history(path) -> Vec<Snapshot>`; `metrics(&[Snapshot]) -> PerfMetrics`
  where `PerfMetrics { volatility, sharpe, max_drawdown, cumulative_return,
  period_returns }` computed via `coinbasis::stats` over the `f64` value series.
- Handles short series gracefully (the stats fns return `None`; the view shows
  "not enough history yet").

### 7.7 `export`
- `export_capital_gains_csv/json(&CapitalGainsReport, path)` and
  `export_holdings_csv/json(&[Holding], path)`. Triggered from the Tax/Holdings
  views (`e` key) writing to a timestamped file; the status bar reports the path.

### 7.8 `app`
- **Type:** `App { config, model: PortfolioModel, method: CostBasisMethod,
  view: View, tax_year: i32, prices: Option<PriceBook>, last_report:
  Option<PortfolioReport>, sort: SortKey, selected: usize, status: Status,
  loading: bool, should_quit: bool }`.
- **Actions:** `next_view/prev_view`, `cycle_method`, `set_year(+/-)`, `sort_by`,
  `select_next/prev`, `request_refresh`, `export`, `toggle_help`, `quit`.
- Recomputation is centralized: whenever `method`, `tax_year`, or `prices`
  change, derived reports are recomputed once and cached on `App`.

### 7.9 `event`
- A non-blocking loop: poll crossterm for key events (mapped to `Action`s via a
  keymap), fire a refresh `Action` on the interval tick or `r`, and signal
  redraw. Keeps the UI responsive while a fetch is in flight.

### 7.10 `ui`
- `mod.rs` builds the frame: a top tab bar (the six views + active method +
  refresh countdown), the active view's widget, and a bottom status/help line; a
  `?` overlay lists keybindings. Each `ui/<view>.rs` is a pure function
  `render(frame, area, &App)` — no state mutation, so each is testable with
  `TestBackend`.

## 8. The Six Views (detail)

1. **Prices / P&L** — table: SYMBOL · PRICE · 24H% · 7D% · HELD · COST · VALUE ·
   PROFIT · PROFIT% · ALLOC; a sparkline column (7d); a totals row; sortable by
   any column (`s` cycles, or column hotkeys); a detail pane for the selected
   coin (market cap, 24h volume, ATH if available). Colors: green/red by sign.
2. **Holdings** — open lots per wallet from `holdings(method)`: ASSET · WALLET ·
   QTY · COST BASIS · AVG COST · CURRENT VALUE · UNREALIZED; toggle grouping by
   asset vs wallet (`g`); totals.
3. **Valuation** — headline total value / cost / unrealized / total-return%; a
   horizontal `BarChart` of per-asset allocation; a `missing_prices` warning when
   a held asset has no price.
4. **Tax** — `capital_gains(method, year)` rows (ASSET · ACQUIRED · DISPOSED ·
   QTY · PROCEEDS · BASIS · GAIN · TERM), short/long subtotals + total; the
   income report total; an **estimated tax** line (short×rate + long×rate);
   year selector (`[`/`]`); `e` exports CSV/JSON.
5. **Rebalance** — current vs target weights (bar comparison), the suggested
   `RebalanceAction` list (SIDE · ASSET · AMOUNT$ · DRIFT% · est. realized gain
   for sells), an in-balance/out-of-balance banner, and a strategy toggle (`t`:
   Band/Full).
6. **Performance** — a `Chart` line of value history over time, plus the
   `PerfMetrics` (volatility, Sharpe, max drawdown, cumulative & period returns),
   with a "need more history" state for short series.

## 9. Cross-Cutting Concerns

- **Global method switcher:** `m` cycles Fifo→Lifo→Hifo→Average; the status bar
  shows the active method; all derived views recompute from cached state (no
  refetch).
- **Error handling:** network errors are non-fatal — shown in the status bar,
  last-good prices retained, and `--offline` skips fetching entirely (cache
  only). Ledger/validation errors at startup are fatal with a clear message.
  A panic hook restores the terminal (leave raw mode, show cursor) before
  printing the panic, so a crash never leaves a broken terminal.
- **Persistence:** `ledger.json` (read; written only by future edit features),
  `config.json`, `history.json` (appended), and the on-disk price cache.
- **Exports:** timestamped CSV/JSON in the working dir; path echoed to status.
- **UX:** status bar (method · countdown · loading spinner · last message), `?`
  help overlay, empty states ("no transactions yet", "not enough history"),
  consistent color scheme, responsive to terminal resize.

## 10. Display Currency vs. Cost Basis

Cost basis and all tax/valuation math are in **USD** (coinbasis's quote
currency). `display_currency` only affects the *current price* and *current
value* readouts in the Prices view when the user chooses a non-USD vs_currency
from CoinGecko; tax and realized figures remain USD and are labeled as such. For
v0.1 the default and recommended setting is `usd` throughout to avoid mixing
units; non-USD display is a clearly-scoped convenience.

## 11. Keybindings (summary)

`Tab`/`Shift-Tab` or `←/→` switch views · `↑/↓` move selection · `m` cycle method
· `s` cycle sort · `g` toggle grouping · `[`/`]` change tax year · `t` toggle
rebalance strategy · `r` refresh now · `e` export (Tax/Holdings) · `?` help ·
`q`/`Esc` quit.

## 12. Testing Strategy

- **Domain (pure, high coverage):** `config` parse + validation, `ledger`
  load/derive-assets, `prices` via `MockSource` (incl. cache TTL + last-good
  fallback), `portfolio` wrapping against a fixed ledger (gains/holdings/
  valuation under each method), `rebalance` math (band vs full, min-trade, sell
  caps, tax-aware estimate), `perf` metrics incl. short-series `None` handling,
  `export` round-trips.
- **TUI:** `ratatui::TestBackend` renders each view from a fixed `App` and asserts
  on the buffer (presence/positions of key cells, totals, colors). No live
  network.
- **Integration:** an end-to-end test builds an `App` from a fixture ledger +
  `MockSource`, exercises actions (cycle method, change year, refresh), and
  asserts the recomputed reports.
- **Tooling:** `cargo fmt`, `cargo clippy -D warnings`, `cargo-llvm-cov` for
  coverage on the domain modules. TDD throughout, matching the coinbasis build.

## 13. coinbasis Integration Points (exact APIs)

- Construct: `Portfolio::from_transactions(&txs)`.
- Queries: `realized_gains`, `holdings`, `valuation(method, &HashMap<String,
  Decimal>)`, `capital_gains_report`, `income_report`.
- Stats: `stats::{returns_from_values, volatility, sharpe_ratio, max_drawdown,
  cumulative_return}` over the value-history `f64` series.
- Types reused directly in the UI: `Holding`, `RealizedGain`, `Term`,
  `AssetValuation`, `PortfolioReport`, `CapitalGainsReport`, `IncomeReport`,
  `CostBasisMethod`, `Transaction`, `IncomeSource`.
- Dependency: `coinbasis = { version = "0.1", features = ["serde"] }`.

## 14. CoinGecko Integration

- Client: `coingecko::CoinGeckoClient` (async). One batched **simple price** call
  per refresh for all held ids → price + 24h change + market cap + 24h volume.
  Optional **market chart** calls (7-day) for sparklines, throttled and cached to
  respect the public rate limit. The exact method names/signatures are pinned
  against docs.rs during implementation; the `prices` module isolates them behind
  `PriceSource`.

## 15. Build Ordering (for the plan)

1. Scaffold (Cargo.toml, deps, `main` that opens/closes a blank TUI, panic hook).
2. `error`, `config` (+ example), `ledger`.
3. `prices` trait + `MockSource` + cache; then `CoinGeckoSource`.
4. `portfolio` wrapper; `perf`; `rebalance`; `export`.
5. `app` state + `event` loop + `ui` shell (tab bar, status bar, help).
6. Views in order: Prices → Holdings → Valuation → Tax → Rebalance → Performance.
7. Wire async refresh + caching + offline mode.
8. Coverage pass, README + examples of config/ledger, publish/release readiness.

## 16. Future (post-v0.1)

In-TUI transaction entry; Specific-ID selection UI; multi-fiat cost basis;
configurable color themes; alerting/price targets; more CoinGecko data
(categories, ATH/ATL detail); optional use of the Crypto-Proxy-coingecko Worker
as an alternate `PriceSource`.

---

## Self-review checklist (to run after writing)
- No placeholders/TBDs in module interfaces or data formats.
- Architecture (pure core + thin TUI) is consistent across §5/§7/§12.
- Scope is a single coherent app, decomposable into the §15 ordering.
- Decimal/f64 boundary stated once (§7.3) and referenced where used.
