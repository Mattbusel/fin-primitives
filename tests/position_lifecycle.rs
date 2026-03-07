use fin_primitives::position::{Fill, PositionLedger};
use fin_primitives::types::*;
use rust_decimal_macros::dec;
use std::collections::HashMap;

#[test]
fn test_position_open_close_pnl() {
    let mut ledger = PositionLedger::new(dec!(100000));
    let sym = Symbol::new("TSLA").unwrap();
    ledger.apply_fill(Fill {
        symbol: sym.clone(),
        side: Side::Bid,
        quantity: Quantity::new(dec!(100)).unwrap(),
        price: Price::new(dec!(200)).unwrap(),
        timestamp: NanoTimestamp(0),
        commission: dec!(0),
    }).unwrap();
    assert_eq!(ledger.cash(), dec!(80000));
    ledger.apply_fill(Fill {
        symbol: sym.clone(),
        side: Side::Ask,
        quantity: Quantity::new(dec!(100)).unwrap(),
        price: Price::new(dec!(220)).unwrap(),
        timestamp: NanoTimestamp(1),
        commission: dec!(0),
    }).unwrap();
    assert_eq!(ledger.cash(), dec!(102000));
    assert_eq!(ledger.realized_pnl_total(), dec!(2000));
    assert!(ledger.position(&sym).unwrap().is_flat());
}

#[test]
fn test_position_equity_with_open_position() {
    let mut ledger = PositionLedger::new(dec!(50000));
    let sym = Symbol::new("NVDA").unwrap();
    ledger.apply_fill(Fill {
        symbol: sym.clone(),
        side: Side::Bid,
        quantity: Quantity::new(dec!(10)).unwrap(),
        price: Price::new(dec!(500)).unwrap(),
        timestamp: NanoTimestamp(0),
        commission: dec!(0),
    }).unwrap();
    let mut prices = HashMap::new();
    prices.insert("NVDA".to_string(), Price::new(dec!(550)).unwrap());
    let equity = ledger.equity(&prices).unwrap();
    // cash = 45000, unrealized = 10 * (550 - 500) = 500, equity = 45500
    assert_eq!(equity, dec!(45500));
}
