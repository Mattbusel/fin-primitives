// Integration tests: JSON serialization round-trips for all public serializable types.

use fin_primitives::ohlcv::{OhlcvBar, Timeframe};
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook, PriceLevel};
use fin_primitives::tick::Tick;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

fn sym(s: &str) -> Symbol {
    Symbol::new(s).unwrap()
}

fn price(s: &str) -> Price {
    Price::new(s.parse().unwrap()).unwrap()
}

fn qty(s: &str) -> Quantity {
    Quantity::new(s.parse().unwrap()).unwrap()
}

// ── Symbol ────────────────────────────────────────────────────────────────

#[test]
fn symbol_serde_roundtrip() {
    let sym = sym("BTC");
    let json = serde_json::to_string(&sym).unwrap();
    let back: Symbol = serde_json::from_str(&json).unwrap();
    assert_eq!(sym, back);
}

// ── Price ─────────────────────────────────────────────────────────────────

#[test]
fn price_serde_roundtrip() {
    let p = price("12345.6789");
    let json = serde_json::to_string(&p).unwrap();
    let back: Price = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

// ── Quantity ──────────────────────────────────────────────────────────────

#[test]
fn quantity_serde_roundtrip() {
    let q = qty("99.999");
    let json = serde_json::to_string(&q).unwrap();
    let back: Quantity = serde_json::from_str(&json).unwrap();
    assert_eq!(q, back);
}

// ── Side ──────────────────────────────────────────────────────────────────

#[test]
fn side_bid_serde_roundtrip() {
    let json = serde_json::to_string(&Side::Bid).unwrap();
    let back: Side = serde_json::from_str(&json).unwrap();
    assert_eq!(Side::Bid, back);
}

#[test]
fn side_ask_serde_roundtrip() {
    let json = serde_json::to_string(&Side::Ask).unwrap();
    let back: Side = serde_json::from_str(&json).unwrap();
    assert_eq!(Side::Ask, back);
}

// ── NanoTimestamp ─────────────────────────────────────────────────────────

#[test]
fn nanotimestamp_serde_roundtrip() {
    let ts = NanoTimestamp::new(1_700_000_000_000_000_000);
    let json = serde_json::to_string(&ts).unwrap();
    let back: NanoTimestamp = serde_json::from_str(&json).unwrap();
    assert_eq!(ts, back);
}

// ── Tick ──────────────────────────────────────────────────────────────────

#[test]
fn tick_serde_roundtrip() {
    let tick = Tick::new(
        sym("ETH"),
        price("2050.50"),
        qty("3.75"),
        Side::Ask,
        NanoTimestamp::new(1_000_000_000),
    );
    let json = serde_json::to_string(&tick).unwrap();
    let back: Tick = serde_json::from_str(&json).unwrap();
    assert_eq!(back.symbol, tick.symbol);
    assert_eq!(back.price, tick.price);
    assert_eq!(back.quantity, tick.quantity);
    assert_eq!(back.side, tick.side);
    assert_eq!(back.timestamp, tick.timestamp);
}

// ── OhlcvBar ──────────────────────────────────────────────────────────────

#[test]
fn ohlcv_bar_serde_roundtrip() {
    let bar = OhlcvBar {
        symbol: sym("SPY"),
        open: price("440"),
        high: price("445"),
        low: price("438"),
        close: price("443"),
        volume: qty("100000"),
        ts_open: NanoTimestamp::new(0),
        ts_close: NanoTimestamp::new(60_000_000_000),
        tick_count: 1250,
    };
    let json = serde_json::to_string(&bar).unwrap();
    let back: OhlcvBar = serde_json::from_str(&json).unwrap();
    assert_eq!(bar, back);
}

// ── Timeframe ─────────────────────────────────────────────────────────────

#[test]
fn timeframe_serde_roundtrip_seconds() {
    let tf = Timeframe::Seconds(30);
    let json = serde_json::to_string(&tf).unwrap();
    let back: Timeframe = serde_json::from_str(&json).unwrap();
    assert_eq!(tf.to_nanos().unwrap(), back.to_nanos().unwrap());
}

#[test]
fn timeframe_serde_roundtrip_minutes() {
    let tf = Timeframe::Minutes(5);
    let json = serde_json::to_string(&tf).unwrap();
    let back: Timeframe = serde_json::from_str(&json).unwrap();
    assert_eq!(tf.to_nanos().unwrap(), back.to_nanos().unwrap());
}

#[test]
fn timeframe_serde_roundtrip_weeks() {
    let tf = Timeframe::Weeks(1);
    let json = serde_json::to_string(&tf).unwrap();
    let back: Timeframe = serde_json::from_str(&json).unwrap();
    assert_eq!(tf.to_nanos().unwrap(), back.to_nanos().unwrap());
}

// ── OrderBook / PriceLevel ────────────────────────────────────────────────

#[test]
fn price_level_serde_roundtrip() {
    let level = PriceLevel {
        price: price("100.5"),
        quantity: qty("25"),
    };
    let json = serde_json::to_string(&level).unwrap();
    let back: PriceLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(level.price, back.price);
    assert_eq!(level.quantity, back.quantity);
}

#[test]
fn book_delta_serde_roundtrip() {
    let delta = BookDelta {
        side: Side::Bid,
        price: price("200"),
        quantity: qty("10"),
        action: DeltaAction::Set,
        sequence: 42,
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: BookDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(back.price, delta.price);
    assert_eq!(back.quantity, delta.quantity);
    assert_eq!(back.sequence, delta.sequence);
    assert!(matches!(back.side, Side::Bid));
    assert!(matches!(back.action, DeltaAction::Set));
}

#[test]
fn order_book_snapshot_values_survive_serde() {
    // OrderBook itself is not Serialize, but PriceLevels from snapshot() are.
    let mut book = OrderBook::new(sym("BTC"));
    book.apply_delta(BookDelta {
        side: Side::Bid,
        price: price("50000"),
        quantity: qty("2"),
        action: DeltaAction::Set,
        sequence: 1,
    })
    .unwrap();
    let (bids, _asks) = book.snapshot(1);
    let json = serde_json::to_string(&bids).unwrap();
    let back: Vec<PriceLevel> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].price.value(), dec!(50000));
}
