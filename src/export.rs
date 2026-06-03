//! CSV/JSON export of tax and holdings reports. Hand-rolled CSV (no crate)
//! since the rows are simple and fully numeric/string.

use coinbasis::{CapitalGainsReport, Holding, Term};

use crate::error::AppError;

fn write(path: &str, text: &str) -> Result<(), AppError> {
    std::fs::write(path, text).map_err(|e| AppError::Export(e.to_string()))
}

pub fn export_capital_gains_json(rep: &CapitalGainsReport, path: &str) -> Result<(), AppError> {
    let text = serde_json::to_string_pretty(rep).map_err(|e| AppError::Export(e.to_string()))?;
    write(path, &text)
}

pub fn export_capital_gains_csv(rep: &CapitalGainsReport, path: &str) -> Result<(), AppError> {
    let mut out = String::from(
        "asset,wallet,acquired,disposed,quantity,proceeds,cost_basis,gain,term\n",
    );
    for r in &rep.rows {
        let acquired = r.acquired_at.map(|d| d.to_rfc3339()).unwrap_or_default();
        let term = match r.term {
            Some(Term::Short) => "Short",
            Some(Term::Long) => "Long",
            None => "",
        };
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            r.asset,
            r.wallet,
            acquired,
            r.disposed_at.to_rfc3339(),
            r.quantity,
            r.proceeds,
            r.cost_basis,
            r.gain,
            term,
        ));
    }
    write(path, &out)
}

pub fn export_holdings_json(holdings: &[Holding], path: &str) -> Result<(), AppError> {
    let text =
        serde_json::to_string_pretty(holdings).map_err(|e| AppError::Export(e.to_string()))?;
    write(path, &text)
}

pub fn export_holdings_csv(holdings: &[Holding], path: &str) -> Result<(), AppError> {
    let mut out = String::from("asset,wallet,quantity,cost_basis,average_cost\n");
    for h in holdings {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            h.asset, h.wallet, h.quantity, h.cost_basis, h.average_cost,
        ));
    }
    write(path, &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use coinbasis::{CapitalGainsReport, Holding, RealizedGain, Term};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn cg() -> CapitalGainsReport {
        CapitalGainsReport {
            tax_year: 2023,
            rows: vec![RealizedGain {
                asset: "bitcoin".into(),
                wallet: "coinbase".into(),
                disposed_at: Utc.with_ymd_and_hms(2023, 6, 1, 0, 0, 0).unwrap(),
                acquired_at: Some(Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap()),
                quantity: dec!(0.5),
                proceeds: dec!(25000),
                cost_basis: dec!(15000),
                gain: dec!(10000),
                term: Some(Term::Long),
            }],
            short_term_gain: dec!(0),
            long_term_gain: dec!(10000),
            total_gain: dec!(10000),
        }
    }

    #[test]
    fn capital_gains_csv_has_header_and_row() {
        let dir = std::env::temp_dir().join("cpt2_export");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cg.csv");
        export_capital_gains_csv(&cg(), path.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("asset,wallet,acquired,disposed,quantity,proceeds,cost_basis,gain,term"));
        assert!(text.contains("bitcoin"));
        assert!(text.contains("Long"));
    }

    #[test]
    fn capital_gains_json_roundtrips() {
        let dir = std::env::temp_dir().join("cpt2_export");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cg.json");
        export_capital_gains_json(&cg(), path.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let back: CapitalGainsReport = serde_json::from_str(&text).unwrap();
        assert_eq!(back.total_gain, dec!(10000));
    }

    #[test]
    fn holdings_csv_has_header() {
        let dir = std::env::temp_dir().join("cpt2_export");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("h.csv");
        let holdings = vec![Holding {
            asset: "bitcoin".into(),
            wallet: "coinbase".into(),
            quantity: dec!(1.5),
            cost_basis: dec!(55000),
            average_cost: dec!(36666.67),
        }];
        export_holdings_csv(&holdings, path.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("asset,wallet,quantity,cost_basis,average_cost"));
        assert!(text.contains("bitcoin"));
    }
}
