# Crypto-Price-Tracker-V2

A fast Rust terminal UI for tracking a crypto portfolio: live prices and P&L,
holdings, valuation and allocation, bracketed capital-gains tax, tax-aware
rebalancing with risk/correlation/backtest analytics, and reconstructed
performance history with playback. Cost-basis and tax math are delegated to the
[`coinbasis`](https://crates.io/crates/coinbasis) crate, portfolio analytics to
[`cryptolytics`](https://crates.io/crates/cryptolytics), and live prices come
from [`coingecko`](https://crates.io/crates/coingecko).

## Quick start

```bash
cp config.example.json config.json
cp ledger.example.json ledger.json
cargo run --release
```

Run offline (cache only, no network): `cargo run --release -- --offline`.

## Configuration

`config.json` (see `config.example.json`): ledger path, default cost-basis
method, display currency, refresh interval, tax config, target allocation
weights, rebalance band/min-trade/strategy, display symbols, cache TTL/dir,
CoinGecko key/plan, and `history_days`.

### Tax brackets

The `tax` block is a `coinbasis::tax::TaxConfig`: a `jurisdiction` label, a
`long_term_threshold_days` (e.g. `365`), a flat `short_term_rate`, and a
progressive `long_term_brackets` list. Each bracket is `{ "up_to": <amount or
null>, "rate": <decimal> }`, ordered ascending; the final bracket uses `"up_to":
null` for the top tier:

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

The Tax view shows the bracketed estimate (short-term, long-term, and total tax)
via `coinbasis::tax::estimate`. Tax figures are estimates, not tax advice.

### CoinGecko API key and plan

```json
"coingecko": { "api_key": "", "plan": "Demo" }
```

`plan` is `Demo` or `Pro`. The key is also read from the `COINGECKO_API_KEY`
environment variable, which takes precedence over the config value. With no key,
the public Demo endpoint is used (rate-limited, no price history).

### Price history

`history_days` (default `90`) controls how many days of daily price history are
fetched per asset on startup and on manual refresh (`r`). History powers the
rebalance analytics and the Performance reconstruction/playback. A CoinGecko key
is generally required for the market-chart endpoint.

## CSV import

Import transactions from a CSV into the JSON ledger (appends non-duplicate
rows), then exit without launching the TUI:

```bash
cargo run -- --import transactions.example.csv --ledger ledger.json
```

The CSV header is `date,coin,action,quantity,price_usd,fee_usd` with an optional
trailing `wallet` column (defaults to `default`). `action` is `buy` or `sell`;
`date` is `YYYY-MM-DD`; a blank fee defaults to `0`. Invalid rows are skipped and
reported. See `transactions.example.csv`.

## Ledger

`ledger.json` is a JSON array of `coinbasis` transactions. Asset ids are
**CoinGecko coin ids** (`bitcoin`, `ethereum`, …). See `ledger.example.json`.

## Rebalance analytics

The Rebalance view derives target weights from a cycleable strategy (`w`):
`Custom` (config `targets`), `Equal`, or `MarketCap` (live market caps). When
price history is available it also shows, via `cryptolytics`, per-coin daily and
annualized volatility, a value-weighted portfolio volatility, a return
correlation matrix, and a buy-and-hold backtest comparing current vs. target
weights over the history window.

## Performance reconstruction and playback

The Performance view reconstructs daily portfolio snapshots by replaying the
ledger as-of each price-history date (value, cost, and P&L). Press `p` to toggle
between the chart (value + P&L over time, with volatility/Sharpe/drawdown/return
metrics) and playback (a scrollable per-day list with a P&L bar; `Up`/`Down`
selects a day and the pane below shows that day's per-coin holdings breakdown).
When no reconstructed history is available the view falls back to the
forward-recorded snapshots in `history.jsonl`.

## Keybindings

`Tab`/`->` next view · `Shift-Tab`/`<-` prev view · `Up/Down` select · `m` cycle method
· `s` cycle sort · `g` toggle grouping · `[`/`]` change tax year · `t` toggle
rebalance strategy · `w` cycle target strategy · `p` toggle Performance playback ·
`r` refresh · `e` export (Tax/Holdings) · `?` help · `q`/`Esc` quit.

## Notes

All cost-basis and tax figures are in USD. Tax estimates use user-supplied rates
and are not tax advice. The method switcher cycles FIFO/LIFO/HIFO/Average.
