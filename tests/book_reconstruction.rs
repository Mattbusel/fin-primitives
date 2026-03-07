use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::types::*;
use rust_decimal_macros::dec;

fn set_delta(side: Side, price: &str, qty: &str, seq: u64) -> BookDelta {
    BookDelta {
        side,
        price: Price::new(price.parse().unwrap()).unwrap(),
        quantity: Quantity::new(qty.parse().unwrap()).unwrap(),
        action: DeltaAction::Set,
        sequence: seq,
    }
}

#[test]
fn test_book_reconstruction_best_bid_ask() {
    let mut book = OrderBook::new(Symbol::new("BTC").unwrap());
    for i in 1u64..=10 {
        let bid_price = format!("{}", 100 - i + 1);
        let ask_price = format!("{}", 100 + i);
        book.apply_delta(set_delta(Side::Bid, &bid_price, "1", i * 2 - 1)).unwrap();
        book.apply_delta(set_delta(Side::Ask, &ask_price, "1", i * 2)).unwrap();
    }
    // Best bid = 100, best ask = 101
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(100));
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(101));
}

#[test]
fn test_book_reconstruction_spread() {
    let mut book = OrderBook::new(Symbol::new("ETH").unwrap());
    book.apply_delta(set_delta(Side::Bid, "1000", "5", 1)).unwrap();
    book.apply_delta(set_delta(Side::Ask, "1005", "5", 2)).unwrap();
    assert_eq!(book.spread().unwrap(), dec!(5));
}

#[test]
fn test_book_reconstruction_vwap() {
    let mut book = OrderBook::new(Symbol::new("SOL").unwrap());
    // Set 5 ask levels
    book.apply_delta(set_delta(Side::Ask, "50", "10", 1)).unwrap();
    book.apply_delta(set_delta(Side::Ask, "51", "10", 2)).unwrap();
    book.apply_delta(set_delta(Side::Ask, "52", "10", 3)).unwrap();

    // Buy 15 units: 10@50 + 5@51 = 755/15 = 50.333...
    let vwap = book.vwap_for_qty(Side::Ask, Quantity::new(dec!(15)).unwrap()).unwrap();
    let expected = dec!(755) / dec!(15);
    assert_eq!(vwap, expected);
}

#[test]
fn test_book_reconstruction_sequence_enforced() {
    let mut book = OrderBook::new(Symbol::new("LINK").unwrap());
    book.apply_delta(set_delta(Side::Bid, "10", "1", 1)).unwrap();
    // Applying sequence 3 when expected 2 must fail.
    let result = book.apply_delta(set_delta(Side::Bid, "11", "1", 3));
    assert!(result.is_err());
}
