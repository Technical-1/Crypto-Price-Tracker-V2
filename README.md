# Crypto-Price-Tracker-V2

A Rust terminal UI for tracking a crypto portfolio: live prices and unrealized P&L,
open lot holdings, valuation and allocation, bracketed capital-gains tax estimates,
tax-aware rebalancing with risk analytics, and reconstructed historical performance
with chart and day-by-day playback.

Cost-basis and tax math are handled by [`coinbasis`](https://crates.io/crates/coinbasis),
portfolio analytics by [`cryptolytics`](https://crates.io/crates/cryptolytics), and
live prices come from [`coingecko`](https://crates.io/crates/coingecko).

## Features

- **Prices / P&L view** — live prices, 24h/7d change, unrealized gain per coin,
  sorted by value or profit. Sparklines and market cap from CoinGecko.
- **Holdings view** — open cost-basis lots per wallet with current market value,
  unrealized P&L, and optional wallet grouping.
- **Valuation / Allocation view** — total portfolio value, cost, return, and
  per-coin allocation percentages.
- **Tax view** — capital-gains report for a selectable tax year with short-term,
  long-term, and total figures; progressive bracket estimation via `coinbasis::tax`.
  Export to CSV + JSON with `e`.
- **Rebalance view** — drift vs. configured target weights, buy/sell suggestions
  (Band or Full strategy), per-coin volatility and annualized vol, pairwise return
  correlation matrix, value-weighted portfolio volatility, and a buy-and-hold
  backtest comparing current vs. target weights over the history window. Cycle
  target strategy (Custom / Equal / MarketCap) with `w`.
- **Performance view** — reconstructs daily portfolio snapshots by replaying the
  ledger against per-coin price history; chart mode shows value + P&L over time
  with volatility, Sharpe ratio, max drawdown, and cumulative return. Playback
  mode (`p`) gives a scrollable day-by-day list with per-coin holdings breakdown.
- **Global cost-basis method switcher** — cycle FIFO / LIFO / HIFO / Average with
  `m`; all derived reports recompute instantly.
- **On-disk price cache** — configurable TTL; falls back to last-good cache when
  a fetch fails or when running with `--offline`.
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

- Rust toolchain (stable, 2021 edition or later)
- Optional: a [CoinGecko](https://www.coingecko.com/en/api) API key for price
  history (the market-chart endpoint requires a key)

### Installation

```bash
git clone https://github.com/Technical-1/Crypto-Price-Tracker-V2
cd Crypto-Price-Tracker-V2
cp config.example.json config.json
cp ledger.example.json ledger.json
```

Edit `config.json` to set your ledger path, display currency, tax brackets,
target allocation weights, and CoinGecko key/plan. Asset IDs are CoinGecko
coin IDs (`bitcoin`, `ethereum`, `solana`, …).

### Usage

```bash
# Launch the TUI (fetches live prices on startup)
cargo run --release

# Run offline — serve from cache only, no network calls
cargo run --release -- --offline

# Import transactions from a CSV, then exit
cargo run --release -- --import transactions.example.csv --ledger ledger.json
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
| `tax` | `coinbasis::TaxConfig` — jurisdiction, long-term threshold, bracket list |
| `targets` | Per-coin allocation weights for Custom strategy |
| `rebalance` | Band tolerance, minimum trade size, default strategy |
| `symbols` | Display ticker overrides (falls back to uppercased coin ID) |
| `cache` | Cache directory and TTL in seconds |
| `coingecko` | `api_key` and `plan` (`Demo` or `Pro`) |
| `history_days` | Days of daily price history fetched per coin (default `90`) |

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
cargo fmt

# Lint
cargo clippy -- -D warnings

# Run all tests (unit + integration)
cargo test

# Optimized build
cargo build --release
```

## Project Structure

```
src/
├── main.rs          # Entry point: CLI parsing, terminal setup, async event loop
├── app.rs           # App state and recompute logic (method/year/prices → reports)
├── event.rs         # Key → Action mapping and action dispatch
├── config.rs        # Config deserialization (config.json)
├── ledger.rs        # JSON ledger I/O and CSV import
├── portfolio.rs     # Thin coinbasis::Portfolio wrapper + HoldingValue enrichment
├── perf.rs          # Snapshot persistence (JSONL), metrics, ledger-replay reconstruction
├── rebalance.rs     # Drift computation, trade suggestions, Band/Full strategies
├── export.rs        # CSV + JSON export for Tax and Holdings views
├── error.rs         # AppError enum
├── lib.rs           # Crate root
├── prices/
│   ├── mod.rs       # PriceSource trait, PriceBook, Quote, HistoryData types
│   ├── coingecko.rs # CoinGeckoSource: coins_markets + coin_market_chart fetches
│   ├── cache.rs     # On-disk PriceBook cache with TTL and last-good fallback
│   └── mock.rs      # MockSource for tests
└── ui/
    ├── mod.rs       # Frame layout: tab bar, active view, status bar, help overlay
    ├── prices.rs    # Prices / P&L view
    ├── holdings.rs  # Holdings view
    ├── valuation.rs # Valuation / Allocation view
    ├── tax.rs       # Tax view
    ├── rebalance.rs # Rebalance view
    └── perf.rs      # Performance chart + playback view
```

## Keybindings

| Key | Action |
|---|---|
| `Tab` / `→` | Next view |
| `Shift-Tab` / `←` | Previous view |
| `↑` / `↓` | Move selection |
| `m` | Cycle cost-basis method (FIFO → LIFO → HIFO → AVG) |
| `s` | Cycle sort column |
| `g` | Toggle wallet grouping |
| `[` / `]` | Previous / next tax year |
| `t` | Toggle rebalance strategy (Band / Full) |
| `w` | Cycle target strategy (Custom / Equal / MarketCap) |
| `p` | Toggle Performance chart / playback mode |
| `r` | Refresh prices now |
| `e` | Export to CSV + JSON (Tax or Holdings view) |
| `?` | Toggle help overlay |
| `q` / `Esc` | Quit |

## License

MIT OR Apache-2.0

## Author

Jacob Kanfer — [GitHub](https://github.com/Technical-1)
