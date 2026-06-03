//! Ledger persistence: a JSON array of `coinbasis::Transaction`.

use std::collections::BTreeSet;

use coinbasis::Transaction;

use crate::error::AppError;

pub fn load_ledger(path: &str) -> Result<Vec<Transaction>, AppError> {
    let text = std::fs::read_to_string(path).map_err(|e| AppError::Ledger {
        path: path.to_string(),
        reason: e.to_string(),
    })?;
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
    fn malformed_ledger_surfaces_clear_error() {
        let dir = std::env::temp_dir().join("cpt2_ledger_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{ not an array }").unwrap();
        let err = load_ledger(path.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("bad.json"));
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
