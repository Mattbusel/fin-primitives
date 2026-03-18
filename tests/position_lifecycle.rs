use fin_primitives::position::{Fill, Position, PositionLedger};
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

// ── Average-cost basis across multiple fills at different prices ──────────

/// Buy 10 @ 100, then 5 @ 130: weighted avg = (10*100 + 5*130) / 15 = 1650/15 = 110.
#[test]
fn test_avg_cost_two_buys_different_prices() {
    let sym = Symbol::new("X").unwrap();
    let mut pos = Position::new(sym.clone());
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Bid,
        quantity: Quantity::new(dec!(10)).unwrap(),
        price: Price::new(dec!(100)).unwrap(),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Bid,
        quantity: Quantity::new(dec!(5)).unwrap(),
        price: Price::new(dec!(130)).unwrap(),
        timestamp: NanoTimestamp(1), commission: dec!(0),
    }).unwrap();
    assert_eq!(pos.avg_cost, dec!(110));
}

/// Three buys at 100, 200, 300 with equal size: avg = (100+200+300)/3 = 200.
#[test]
fn test_avg_cost_three_buys_equal_size() {
    let sym = Symbol::new("X").unwrap();
    let mut pos = Position::new(sym.clone());
    for p in [dec!(100), dec!(200), dec!(300)] {
        pos.apply_fill(&Fill {
            symbol: sym.clone(), side: Side::Bid,
            quantity: Quantity::new(dec!(1)).unwrap(),
            price: Price::new(p).unwrap(),
            timestamp: NanoTimestamp(0), commission: dec!(0),
        }).unwrap();
    }
    assert_eq!(pos.avg_cost, dec!(200));
}

// ── Short position PnL ────────────────────────────────────────────────────

#[test]
fn test_short_position_unrealized_pnl_below_entry() {
    let sym = Symbol::new("X").unwrap();
    let mut pos = Position::new(sym.clone());
    // Short 5 @ 100
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Ask,
        quantity: Quantity::new(dec!(5)).unwrap(),
        price: Price::new(dec!(100)).unwrap(),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    // Price drops to 80: profit for a short = 5*(100-80) = 100
    let upnl = pos.unrealized_pnl(Price::new(dec!(80)).unwrap());
    assert_eq!(upnl, dec!(100));
}

#[test]
fn test_short_position_unrealized_pnl_above_entry_is_negative() {
    let sym = Symbol::new("X").unwrap();
    let mut pos = Position::new(sym.clone());
    // Short 5 @ 100
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Ask,
        quantity: Quantity::new(dec!(5)).unwrap(),
        price: Price::new(dec!(100)).unwrap(),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    // Price rises to 110: loss for a short = 5*(100-110) = -50
    let upnl = pos.unrealized_pnl(Price::new(dec!(110)).unwrap());
    assert_eq!(upnl, dec!(-50));
}

// ── Flat → long → short transitions ──────────────────────────────────────

/// Start flat, go long, fully close, then open a short.
#[test]
fn test_flat_to_long_to_flat_to_short() {
    let sym = Symbol::new("X").unwrap();
    let mut pos = Position::new(sym.clone());
    assert!(pos.is_flat());

    // Go long 10 @ 100.
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Bid,
        quantity: Quantity::new(dec!(10)).unwrap(),
        price: Price::new(dec!(100)).unwrap(),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    assert_eq!(pos.quantity, dec!(10));
    assert!(!pos.is_flat());

    // Close long at 110 (realized = 100).
    let pnl = pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Ask,
        quantity: Quantity::new(dec!(10)).unwrap(),
        price: Price::new(dec!(110)).unwrap(),
        timestamp: NanoTimestamp(1), commission: dec!(0),
    }).unwrap();
    assert_eq!(pnl, dec!(100));
    assert!(pos.is_flat());
    assert_eq!(pos.avg_cost, dec!(0));

    // Open a short 5 @ 120.
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Ask,
        quantity: Quantity::new(dec!(5)).unwrap(),
        price: Price::new(dec!(120)).unwrap(),
        timestamp: NanoTimestamp(2), commission: dec!(0),
    }).unwrap();
    assert_eq!(pos.quantity, dec!(-5));
    assert_eq!(pos.avg_cost, dec!(120));
}

/// Directly flip from long to short in one fill larger than current position.
#[test]
fn test_long_to_short_in_one_fill() {
    let sym = Symbol::new("X").unwrap();
    let mut pos = Position::new(sym.clone());
    // Long 5 @ 100.
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Bid,
        quantity: Quantity::new(dec!(5)).unwrap(),
        price: Price::new(dec!(100)).unwrap(),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();

    // Sell 15 @ 110: closes the 5 long (realized = 5*10=50) and opens a short of 10.
    pos.apply_fill(&Fill {
        symbol: sym.clone(), side: Side::Ask,
        quantity: Quantity::new(dec!(15)).unwrap(),
        price: Price::new(dec!(110)).unwrap(),
        timestamp: NanoTimestamp(1), commission: dec!(0),
    }).unwrap();

    assert_eq!(pos.quantity, dec!(-10), "position should be short 10 after flip");
    assert_eq!(pos.avg_cost, dec!(110), "avg_cost of new short = fill price");
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
