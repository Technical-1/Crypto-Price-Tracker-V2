# Crypto-Price-Tracker-V2

A Rust terminal UI for tracking a crypto portfolio: live prices and unrealized P&L,
open lot holdings, valuation and allocation, bracketed capital-gains tax estimates,
tax-aware rebalancing with risk analytics, and reconstructed historical performance
with chart and day-by-day playback.

Cost-basis and tax math are handled by [`coinbasis`](https://crates.io/crates/coinbasis),
portfolio analytics by [`cryptolytics`](https://crates.io/crates/cryptolytics), and
live prices come from [`coingecko`](https://crates.io/crates/coingecko).

## Features

- **Prices / P&L view** — live price, 24h/7d change, holdings, cost, value,
  unrealized profit and allocation per coin, with a 7-day sparkline column and a
  totals row. Sortable by symbol, price, 24h change, value, or profit (`s`); a
  detail pane shows market cap, 24h volume, and all-time high for the selected coin.
- **Holdings view** — open cost-basis positions per asset and wallet, with
  average cost, current market value, unrealized P&L, and a totals row.
- **Valuation / Allocation view** — total portfolio value, cost, unrealized P&L,
  and return, a per-coin allocation bar chart, and a warning listing any held
  assets with no price.
- **Tax view** — realized gains for a selectable tax year (`[` / `]`), one row
  per disposal with acquired/disposed dates and term, plus short-term, long-term,
  and income totals and a bracketed tax estimate via `coinbasis::tax`. Export to
  CSV + JSON with `e`.
- **Rebalance view** — drift vs. configured target weights, buy/sell suggestions
  (Band or Full strategy), per-coin volatility and annualized vol, pairwise return
  correlation matrix, value-weighted portfolio volatility, and a buy-and-hold
  backtest comparing current vs. target weights over the history window. Cycle
  target strategy (Custom / Equal / MarketCap) with `w`.
- **Performance view** — reconstructs daily portfolio snapshots by replaying the
  ledger against per-coin price history (falling back to snapshots recorded on
  each refresh); chart mode shows value + P&L over time with volatility, Sharpe
  ratio, max drawdown, and cumulative return. Playback mode (`p`) gives a
  scrollable list with a P&L bar per point and a per-coin breakdown for the
  selected date.
- **Global cost-basis method switcher** — cycle FIFO / LIFO / HIFO / Average with
  `m`; all derived reports recompute instantly.
- **On-disk price cache** — configurable TTL; on startup the last cached prices
  are shown (marked `[stale]` once past the TTL) before the network responds, and
  `--offline` runs from the cache alone.
- **CSV ledger import** — `--import <file>` appends non-duplicate rows from a
  `date,coin,action,quantity,price_usd,fee_usd[,wallet]` CSV without launching
  the TUI.

## Tech Stack

| Category | Technology | Version |
|---|---|---|
| Language | Rust | 2021 edition |
| TUI framework | ratatui | 0.29 |
| Terminal backend | crossterm | 0.28 |
| Async runtime | tokio | 1.36 |
| Cost-basis / tax | coinbasis | 0.2 |
| Portfolio analytics | cryptolytics | 0.1 |
| Price data | coingecko | 1.1 |
| Decimal arithmetic | rust_decimal | 1 |
| CLI parsing | clap | 4 |
| Serialization | serde + serde_json | 1 |
| Date/time | chrono | 0.4 |

## Getting Started

### Prerequisites

- A free [CoinGecko](https://www.coingecko.com/en/api) API key — the public API
  now requires a Demo key. Set `COINGECKO_API_KEY` or put it in the config.
- Rust toolchain only if building from source (the Cargo/Homebrew installs build
  from source and bring their own).

### Installation

```bash
# With Cargo
cargo install crypto-price-tracker-v2

# Or with Homebrew
brew install Technical-1/tap/crypto-price-tracker-v2
```

Or run from source:

```bash
git clone https://github.com/Technical-1/Crypto-Price-Tracker-V2
cd Crypto-Price-Tracker-V2
cargo run --release
```

If no config file is found (see the lookup order under Configuration), the app
writes a starter config to `~/.config/crypto-price-tracker-v2/config.json` and
prints the path. Edit that file to set your ledger path, display currency, tax
brackets, target allocation weights, and CoinGecko key/plan. A missing ledger
file is treated as an empty portfolio, so the app starts cleanly before you have
added any transactions. Asset IDs are CoinGecko coin IDs (`bitcoin`, `ethereum`,
`solana`, …).

### Usage

```bash
# Launch the TUI (fetches live prices on startup)
crypto-price-tracker-v2

# Run offline — serve from cache only, no network calls
crypto-price-tracker-v2 --offline

# Use a specific config file
crypto-price-tracker-v2 --config /path/to/config.json

# Import transactions from a CSV, then exit
crypto-price-tracker-v2 --import transactions.example.csv --ledger ledger.json
```

The CSV format is `date,coin,action,quantity,price_usd,fee_usd` with an optional
`wallet` column. `action` is `buy` or `sell`; `date` is `YYYY-MM-DD`; a blank
fee defaults to `0`. Invalid rows are skipped and reported to stderr.

## Configuration

`config.json` (see `config.example.json`) controls all behavior:

| Key | Purpose |
|---|---|
| `ledger_path` | Path to the JSON transaction ledger |
| `default_method` | Starting cost-basis method: `Fifo`, `Lifo`, `Hifo`, `Average` |
| `display_currency` | Quote currency (e.g. `usd`) |
| `refresh_seconds` | Auto-refresh interval |
| `tax` | `coinbasis::tax::TaxConfig` — jurisdiction, long-term threshold in days, flat short-term rate, long-term bracket list |
| `targets` | Per-coin allocation weights for the Custom target strategy (normalized, so they need not sum to 1) |
| `rebalance` | Band tolerance, minimum trade size in USD, default strategy (`Band` or `Full`) |
| `symbols` | Display ticker overrides (falls back to uppercased coin ID) |
| `cache` | Cache directory and TTL in seconds |
| `coingecko` | `api_key` and `plan` (`Demo` or `Pro`) |
| `history_days` | Days of price history fetched per coin for analytics and reconstruction (default `90`) |

The app looks for a config file in this order: `--config <path>` if given, then
`./config.json` in the current directory, then
`~/.config/crypto-price-tracker-v2/config.json` (respecting `$XDG_CONFIG_HOME`).
A relative `ledger_path` resolves against the config file's directory, so a
global config works from any working directory; `--ledger <path>` overrides it.
Recorded performance snapshots are written to `history.jsonl` in the same
directory as the config file.

The `COINGECKO_API_KEY` environment variable takes precedence over the config value.

### Tax brackets example

```json
"tax": {
  "jurisdiction": "US",
  "long_term_threshold_days": 365,
  "short_term_rate": "0.35",
  "long_term_brackets": [
    {"up_to": "47025",  "rate": "0.0"},
    {"up_to": "518900", "rate": "0.15"},
    {"up_to": null,     "rate": "0.20"}
  ]
}
```

Tax figures shown in the Tax view are estimates only and are not tax advice.

## Development

```bash
# Format
cargo fmt --all

# Lint (as run in CI)
cargo clippy --all-targets --all-features -- -D warnings

# Run all tests (unit + integration)
cargo test

# Optimized build (LTO enabled in the release profile)
cargo build --release
```

GitHub Actions runs format, clippy, tests, and `cargo doc` on every push and pull
request. Pushing a `v*` tag publishes the crate to crates.io, creates a GitHub
Release, and updates the Homebrew formula in `Technical-1/homebrew-tap`.

## Project Structure

```
Crypto-Price-Tracker-V2/
├── src/
│   ├── main.rs          # Entry point: CLI parsing, terminal setup, async event loop
│   ├── app.rs           # App state and recompute logic (method/year/prices → reports)
│   ├── event.rs         # Key → Action mapping and action dispatch
│   ├── config.rs        # Config types, lookup order, first-run starter config
│   ├── ledger.rs        # JSON ledger I/O and CSV import
│   ├── portfolio.rs     # Thin coinbasis::Portfolio wrapper + HoldingValue enrichment
│   ├── perf.rs          # Snapshot persistence (JSONL), metrics, ledger-replay reconstruction
│   ├── rebalance.rs     # Drift computation, trade suggestions, Band/Full strategies
│   ├── export.rs        # CSV + JSON export for Tax and Holdings views
│   ├── error.rs         # AppError enum
│   ├── lib.rs           # Crate root
│   ├── prices/
│   │   ├── mod.rs       # PriceSource trait, PriceBook, Quote, HistoryData types
│   │   ├── coingecko.rs # CoinGeckoSource: coins_markets + coin_market_chart fetches
│   │   ├── cache.rs     # On-disk PriceBook cache with TTL and last-good fallback
│   │   └── mock.rs      # MockSource for tests
│   └── ui/
│       ├── mod.rs       # Frame layout: tab bar, active view, status bar, help overlay
│       ├── prices.rs    # Prices / P&L view
│       ├── holdings.rs  # Holdings view
│       ├── valuation.rs # Valuation / Allocation view
│       ├── tax.rs       # Tax view
│       ├── rebalance.rs # Rebalance view
│       └── perf.rs      # Performance chart + playback view
├── tests/
│   └── integration.rs   # End-to-end recompute against MockSource prices
├── .github/workflows/   # CI, crates.io release, Homebrew formula bump
├── config.example.json  # Annotated starting config
├── ledger.example.json  # Minimal Buy + Income ledger
└── transactions.example.csv # Sample input for --import
```

## Keybindings

| Key | Action |
|---|---|
| `Tab` / `→` | Next view |
| `Shift-Tab` / `←` | Previous view |
| `↑` / `↓` | Move selection (Prices detail pane, Performance playback) |
| `m` | Cycle cost-basis method (FIFO → LIFO → HIFO → AVG) |
| `s` | Cycle Prices sort column (Symbol / Price / 24h / Value / Profit) |
| `g` | Toggle the Holdings grouping label (by asset / by wallet) |
| `[` / `]` | Previous / next tax year |
| `t` | Toggle rebalance strategy (Band / Full) |
| `w` | Cycle target strategy (Custom / Equal / MarketCap) |
| `p` | Toggle Performance chart / playback mode |
| `r` | Refresh prices and price history now |
| `e` | Export to CSV + JSON (Tax or Holdings view) |
| `?` | Toggle help overlay |
| `q` / `Esc` | Quit |

## License

MIT OR Apache-2.0

## Author

Jacob Kanfer — [GitHub](https://github.com/Technical-1)
