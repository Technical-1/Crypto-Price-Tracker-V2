# Crypto-Price-Tracker-V2

A fast Rust terminal UI for tracking a crypto portfolio: live prices and P&L,
holdings, valuation and allocation, capital-gains tax, tax-aware rebalancing,
and performance statistics. Cost-basis and tax math are delegated to the
[`coinbasis`](https://crates.io/crates/coinbasis) crate; live prices come from
[`coingecko`](https://crates.io/crates/coingecko).

## Quick start

```bash
cp config.example.json config.json
cp ledger.example.json ledger.json
cargo run --release
```

Run offline (cache only, no network): `cargo run --release -- --offline`.

## Configuration

`config.json` (see `config.example.json`): ledger path, default cost-basis
method, display currency, refresh interval, tax rates (estimates only), target
allocation weights, rebalance band/min-trade/strategy, display symbols, and
cache TTL/dir.

## Ledger

`ledger.json` is a JSON array of `coinbasis` transactions. Asset ids are
**CoinGecko coin ids** (`bitcoin`, `ethereum`, …). See `ledger.example.json`.

## Keybindings

`Tab`/`->` next view · `Shift-Tab`/`<-` prev view · `Up/Down` select · `m` cycle method
· `s` cycle sort · `g` toggle grouping · `[`/`]` change tax year · `t` toggle
rebalance strategy · `r` refresh · `e` export (Tax/Holdings) · `?` help · `q`/`Esc` quit.

## Notes

All cost-basis and tax figures are in USD. Tax estimates use user-supplied rates
and are not tax advice. The method switcher cycles FIFO/LIFO/HIFO/Average.
