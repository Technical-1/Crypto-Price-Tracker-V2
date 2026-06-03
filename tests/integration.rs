use crypto_price_tracker_v2 as app_crate;

use app_crate::app::App;
use app_crate::config::Config;
use app_crate::prices::mock::MockSource;
use app_crate::prices::PriceSource;

use chrono::{TimeZone, Utc};
use coinbasis::{CostBasisMethod, Transaction};
use rust_decimal_macros::dec;

#[tokio::test]
async fn end_to_end_recompute_under_method_and_prices() {
    let txs = vec![
        Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(30000),
            fee: dec!(0),
        },
        Transaction::Buy {
            timestamp: Utc.with_ymd_and_hms(2022, 1, 1, 0, 0, 0).unwrap(),
            wallet: "coinbase".into(),
            asset: "bitcoin".into(),
            quantity: dec!(1),
            unit_price: dec!(40000),
            fee: dec!(0),
        },
    ];
    let mut app = App::new(Config::example(), &txs).unwrap();

    let mut src = MockSource::new();
    src.set("bitcoin", dec!(50000), dec!(2.0));
    let assets = vec!["bitcoin".to_string()];
    let book = src.fetch(&assets, "usd").await.unwrap();
    app.set_prices(book);

    let report = app.derived.valuation.as_ref().unwrap();
    assert_eq!(report.total_value, dec!(100000)); // 2 BTC * 50000

    app.cycle_method();
    assert_eq!(app.method, CostBasisMethod::Lifo);
    assert!(app.derived.valuation.is_some());

    app.set_year(-1);
    assert!(app.derived.capital_gains.is_some());
}
