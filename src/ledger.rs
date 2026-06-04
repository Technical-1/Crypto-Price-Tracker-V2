//! Ledger persistence: a JSON array of `coinbasis::Transaction`.

use std::collections::BTreeSet;
use std::str::FromStr;

use chrono::{NaiveDate, TimeZone, Utc};
use coinbasis::Transaction;
use rust_decimal::Decimal;

use crate::error::AppError;

pub fn load_ledger(path: &str) -> Result<Vec<Transaction>, AppError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        // A missing ledger just means "no transactions yet" (e.g. first run).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(AppError::Ledger {
                path: path.to_string(),
                reason: e.to_string(),
            })
        }
    };
    serde_json::from_str(&text).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })
}

pub fn save_ledger(path: &str, txs: &[Transaction]) -> Result<(), AppError> {
    let text = serde_json::to_string_pretty(txs).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(path, text).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })
}

/// Import `date,coin,action,quantity,price_usd,fee_usd` (+ optional `wallet`)
/// CSV into the JSON ledger, appending non-duplicate rows. `(added, skipped)`.
pub fn import_csv(csv_path: &str, ledger_path: &str) -> Result<(usize, usize), AppError> {
    let text = std::fs::read_to_string(csv_path).map_err(|e| AppError::Ledger {
        path: csv_path.to_string(),
        reason: e.to_string(),
    })?;
    let mut existing = load_ledger(ledger_path).unwrap_or_default();
    let (mut added, mut skipped) = (0usize, 0usize);
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        match parse_csv_row(line) {
            Some(tx) if existing.contains(&tx) => skipped += 1,
            Some(tx) => {
                existing.push(tx);
                added += 1;
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
    let wallet = f
        .get(6)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string());
    match f[2].to_lowercase().as_str() {
        "buy" => Some(Transaction::Buy {
            timestamp,
            wallet,
            asset: asset.to_string(),
            quantity,
            unit_price,
            fee,
        }),
        "sell" => Some(Transaction::Sell {
            timestamp,
            wallet,
            asset: asset.to_string(),
            quantity,
            unit_price,
            fee,
        }),
        _ => None,
    }
}

/// Distinct asset ids referenced by any transaction, sorted.
pub fn assets(txs: &[Transaction]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for tx in txs {
        match tx {
            Transaction::Buy { asset, .. }
            | Transaction::Sell { asset, .. }
            | Transaction::Income { asset, .. }
            | Transaction::Spend { asset, .. }
            | Transaction::Transfer { asset, .. }
            | Transaction::GiftSent { asset, .. }
            | Transaction::GiftReceived { asset, .. } => {
                set.insert(asset.clone());
            }
            Transaction::Trade {
                from_asset,
                to_asset,
                ..
            } => {
                set.insert(from_asset.clone());
                set.insert(to_asset.clone());
            }
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        r#"[
          { "Buy": { "timestamp": "2021-01-01T00:00:00Z", "wallet": "coinbase",
                     "asset": "bitcoin", "quantity": "0.5", "unit_price": "30000", "fee": "5" } },
          { "Income": { "timestamp": "2021-06-01T00:00:00Z", "wallet": "kraken",
                        "asset": "ethereum", "quantity": "1.2", "value": "2400", "source": "Staking" } }
        ]"#
    }

    #[test]
    fn loads_transactions() {
        let dir = std::env::temp_dir().join("cpt2_ledger_load");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.json");
        std::fs::write(&path, sample()).unwrap();
        let txs = load_ledger(path.to_str().unwrap()).unwrap();
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn derives_distinct_assets_sorted() {
        let txs: Vec<coinbasis::Transaction> = serde_json::from_str(sample()).unwrap();
        let assets = assets(&txs);
        let v: Vec<_> = assets.into_iter().collect();
        assert_eq!(v, vec!["bitcoin".to_string(), "ethereum".to_string()]);
    }

    #[test]
    fn missing_ledger_is_empty_not_an_error() {
        let dir = std::env::temp_dir().join("cpt2_ledger_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("ledger.json"); // never created
        let txs = load_ledger(path.to_str().unwrap()).unwrap();
        assert!(txs.is_empty());
    }

    #[test]
    fn malformed_ledger_surfaces_clear_error() {
        let dir = std::env::temp_dir().join("cpt2_ledger_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{ not an array }").unwrap();
        let err = load_ledger(path.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("bad.json"));
    }

    #[test]
    fn import_maps_buys_sells_optional_wallet() {
        let dir = std::env::temp_dir().join("cpt2_imp_basic");
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv");
        let led = dir.join("l.json");
        let _ = std::fs::remove_file(&led);
        std::fs::write(
            &csv,
            "date,coin,action,quantity,price_usd,fee_usd,wallet\n\
            2021-01-01,bitcoin,buy,0.5,30000,5,coinbase\n2021-06-01,bitcoin,sell,0.2,40000,2\n",
        )
        .unwrap();
        assert_eq!(
            import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(),
            (2, 0)
        );
        let txs = load_ledger(led.to_str().unwrap()).unwrap();
        match &txs[0] {
            coinbasis::Transaction::Buy { wallet, asset, .. } => {
                assert_eq!(wallet, "coinbase");
                assert_eq!(asset, "bitcoin");
            }
            _ => panic!(),
        }
        match &txs[1] {
            coinbasis::Transaction::Sell { wallet, .. } => assert_eq!(wallet, "default"),
            _ => panic!(),
        }
    }

    #[test]
    fn import_dedups() {
        let dir = std::env::temp_dir().join("cpt2_imp_dedup");
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv");
        let led = dir.join("l.json");
        let _ = std::fs::remove_file(&led);
        std::fs::write(
            &csv,
            "date,coin,action,quantity,price_usd,fee_usd\n2021-01-01,bitcoin,buy,1,30000,0\n",
        )
        .unwrap();
        assert_eq!(
            import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(),
            (1, 0)
        );
        assert_eq!(
            import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(),
            (0, 1)
        );
    }

    #[test]
    fn import_skips_invalid_and_defaults_fee() {
        let dir = std::env::temp_dir().join("cpt2_imp_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("t.csv");
        let led = dir.join("l.json");
        let _ = std::fs::remove_file(&led);
        std::fs::write(
            &csv,
            "date,coin,action,quantity,price_usd,fee_usd\n\
            2021-01-01,bitcoin,buy,1,30000,\nbad,bitcoin,buy,1,30000,0\n2021-01-02,bitcoin,hodl,1,30000,0\n2021-01-03,bitcoin,buy,-1,30000,0\n",
        )
        .unwrap();
        assert_eq!(
            import_csv(csv.to_str().unwrap(), led.to_str().unwrap()).unwrap(),
            (1, 3)
        );
    }

    #[test]
    fn save_then_load_roundtrips() {
        let txs: Vec<coinbasis::Transaction> = serde_json::from_str(sample()).unwrap();
        let dir = std::env::temp_dir().join("cpt2_ledger_rt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.json");
        save_ledger(path.to_str().unwrap(), &txs).unwrap();
        let back = load_ledger(path.to_str().unwrap()).unwrap();
        assert_eq!(back, txs);
    }
}
