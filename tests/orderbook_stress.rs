// Integration tests: L2 order book stress tests — corrupted and out-of-order deltas.
//
// Verifies that the order book:
// 1. Rejects out-of-sequence deltas with `SequenceMismatch`.
// 2. Recovers correctly after a gap by re-applying deltas from the last good sequence.
// 3. Handles rapid bid/ask level churn without leaving inverted spreads.
// 4. Correctly processes duplicate sequence numbers (rejected as mismatch).
// 5. Handles large bursts of rapid-fire deltas without losing state consistency.

use fin_primitives::error::FinError;
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::types::{Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

// ── helpers ──────────────────────────────────────────────────────────────────

fn sym(s: &str) -> Symbol {
    Symbol::new(s).unwrap()
}

fn price(v: rust_decimal::Decimal) -> Price {
    Price::new(v).unwrap()
}

fn qty(v: rust_decimal::Decimal) -> Quantity {
    Quantity::new(v).unwrap()
}

fn set_bid(p: rust_decimal::Decimal, q: rust_decimal::Decimal, seq: u64) -> BookDelta {
    BookDelta {
        side: Side::Bid,
        price: price(p),
        quantity: qty(q),
        action: DeltaAction::Set,
        sequence: seq,
    }
}

fn set_ask(p: rust_decimal::Decimal, q: rust_decimal::Decimal, seq: u64) -> BookDelta {
    BookDelta {
        side: Side::Ask,
        price: price(p),
        quantity: qty(q),
        action: DeltaAction::Set,
        sequence: seq,
    }
}

fn remove_bid(p: rust_decimal::Decimal, seq: u64) -> BookDelta {
    BookDelta {
        side: Side::Bid,
        price: price(p),
        quantity: Quantity::zero(),
        action: DeltaAction::Remove,
        sequence: seq,
    }
}

fn remove_ask(p: rust_decimal::Decimal, seq: u64) -> BookDelta {
    BookDelta {
        side: Side::Ask,
        price: price(p),
        quantity: Quantity::zero(),
        action: DeltaAction::Remove,
        sequence: seq,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Out-of-order delta (sequence gap) must be rejected.
#[test]
fn stress_out_of_order_delta_rejected() {
    let mut book = OrderBook::new(sym("ETH"));
    book.apply_delta(set_bid(dec!(2000), dec!(5), 1)).unwrap();

    // Skip seq 2, jump to seq 3 — should be rejected
    let err = book.apply_delta(set_bid(dec!(1999), dec!(3), 3)).unwrap_err();
    assert!(
        matches!(err, FinError::SequenceMismatch { expected: 2, got: 3 }),
        "expected SequenceMismatch(2,3), got {err:?}"
    );

    // Book state must be unchanged: seq still at 1, best bid still 2000
    assert_eq!(book.sequence(), 1);
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(2000));
}

/// Duplicate (already-seen) sequence number must be rejected.
#[test]
fn stress_duplicate_sequence_rejected() {
    let mut book = OrderBook::new(sym("BTC"));
    book.apply_delta(set_bid(dec!(50000), dec!(1), 1)).unwrap();

    // Re-send seq 1 — must be rejected
    let err = book.apply_delta(set_bid(dec!(49999), dec!(2), 1)).unwrap_err();
    assert!(matches!(err, FinError::SequenceMismatch { expected: 2, got: 1 }));

    // Verify state is intact
    assert_eq!(book.sequence(), 1);
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(50000));
}

/// After a gap, re-applying the missed delta and continuing works correctly.
#[test]
fn stress_recovery_after_gap() {
    let mut book = OrderBook::new(sym("SOL"));

    // Apply seq 1..=3
    book.apply_delta(set_bid(dec!(100), dec!(10), 1)).unwrap();
    book.apply_delta(set_ask(dec!(101), dec!(10), 2)).unwrap();
    book.apply_delta(set_bid(dec!(99), dec!(5), 3)).unwrap();

    // Simulate a gap: seq 4 arrives out of order (seq 5 arrived first but we
    // detected the gap and dropped it). Now we receive seq 4 again and seq 5.
    let err = book.apply_delta(set_bid(dec!(98), dec!(3), 5)).unwrap_err();
    assert!(matches!(err, FinError::SequenceMismatch { expected: 4, got: 5 }));

    // Recovery: apply the missing seq 4, then seq 5
    book.apply_delta(set_ask(dec!(102), dec!(2), 4)).unwrap();
    book.apply_delta(set_bid(dec!(98), dec!(3), 5)).unwrap();

    assert_eq!(book.sequence(), 5);
    // Best bid is still 100 (highest bid seen)
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(100));
    // Two ask levels: 101 and 102
    let asks = book.top_asks(5);
    assert_eq!(asks.len(), 2);
    assert_eq!(asks[0].price.value(), dec!(101));
    assert_eq!(asks[1].price.value(), dec!(102));
}

/// Rapid alternating Set/Remove cycles leave the book in a consistent state.
#[test]
fn stress_rapid_churn_no_orphan_levels() {
    let mut book = OrderBook::new(sym("AAPL"));
    let mut seq = 1u64;

    let prices = [dec!(100), dec!(101), dec!(102), dec!(103), dec!(104)];

    // Round 1: set all five bid levels
    for &p in &prices {
        book.apply_delta(set_bid(p, dec!(10), seq)).unwrap();
        seq += 1;
    }
    assert_eq!(book.top_bids(10).len(), 5);

    // Round 2: remove them all
    for &p in &prices {
        book.apply_delta(remove_bid(p, seq)).unwrap();
        seq += 1;
    }
    assert_eq!(book.top_bids(10).len(), 0, "all bid levels should be removed");
    assert!(book.best_bid().is_none(), "book should be empty on bid side");

    // Round 3: re-add one bid level + one ask level
    book.apply_delta(set_bid(dec!(99), dec!(7), seq)).unwrap();
    seq += 1;
    book.apply_delta(set_ask(dec!(100), dec!(7), seq)).unwrap();
    seq += 1;

    assert_eq!(book.best_bid().unwrap().price.value(), dec!(99));
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(100));
    // Spread must be positive
    let spread = book.spread().unwrap();
    assert!(spread > dec!(0), "spread must be positive: {spread}");
    assert_eq!(book.sequence(), seq - 1);
}

/// Burst of 1 000 sequential deltas maintains consistent state.
#[test]
fn stress_bulk_delta_burst() {
    let mut book = OrderBook::new(sym("MSFT"));
    let mut seq = 1u64;

    // Populate 100 bid levels and 100 ask levels
    for i in 0u32..100 {
        let bid_price = dec!(200) - rust_decimal::Decimal::from(i);
        let ask_price = dec!(201) + rust_decimal::Decimal::from(i);
        book.apply_delta(set_bid(bid_price, dec!(1), seq)).unwrap();
        seq += 1;
        book.apply_delta(set_ask(ask_price, dec!(1), seq)).unwrap();
        seq += 1;
    }

    assert_eq!(book.top_bids(200).len(), 100);
    assert_eq!(book.top_asks(200).len(), 100);
    // Best bid = 200, best ask = 201 → spread = 1
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(200));
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(201));
    assert_eq!(book.spread().unwrap(), dec!(1));

    // Now update quantities in another burst
    for i in 0u32..100 {
        let bid_price = dec!(200) - rust_decimal::Decimal::from(i);
        let ask_price = dec!(201) + rust_decimal::Decimal::from(i);
        book.apply_delta(set_bid(bid_price, dec!(5), seq)).unwrap();
        seq += 1;
        book.apply_delta(set_ask(ask_price, dec!(5), seq)).unwrap();
        seq += 1;
    }

    // Quantities updated; level count unchanged
    assert_eq!(book.top_bids(200).len(), 100);
    assert_eq!(book.best_bid().unwrap().quantity.value(), dec!(5));
}

/// Removing a non-existent level is a no-op and does not corrupt state.
#[test]
fn stress_remove_nonexistent_level_is_safe() {
    let mut book = OrderBook::new(sym("NVDA"));
    book.apply_delta(set_bid(dec!(500), dec!(3), 1)).unwrap();
    // Remove a price level that was never set
    book.apply_delta(remove_bid(dec!(499), 2)).unwrap();

    // Original level still present
    assert_eq!(book.top_bids(5).len(), 1);
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(500));
    assert_eq!(book.sequence(), 2);
}

/// Inverted spread is detected and rolled back.
#[test]
fn stress_inverted_spread_rolled_back() {
    let mut book = OrderBook::new(sym("GOOG"));
    book.apply_delta(set_ask(dec!(2800), dec!(1), 1)).unwrap();

    // Try to add a bid above the best ask — should trigger InvertedSpread
    let err = book.apply_delta(set_bid(dec!(2850), dec!(1), 2)).unwrap_err();
    assert!(
        matches!(err, FinError::InvertedSpread { .. }),
        "expected InvertedSpread, got {err:?}"
    );

    // Sequence must have rolled back to 1 (the bid was not committed)
    assert_eq!(book.sequence(), 1);
    assert!(book.best_bid().is_none(), "bid side must remain empty after rollback");
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(2800));
}

/// Setting a level quantity then explicitly removing it clears the level.
#[test]
fn stress_set_then_remove_clears_level() {
    let mut book = OrderBook::new(sym("TSLA"));
    book.apply_delta(set_bid(dec!(250), dec!(10), 1)).unwrap();
    // Explicit Remove — should clear the level
    book.apply_delta(remove_bid(dec!(250), 2)).unwrap();

    // The level should no longer exist
    assert!(book.best_bid().is_none(), "bid level should be removed");
    assert_eq!(book.sequence(), 2);
}

/// Interleaved bid and ask updates maintain bid < ask invariant throughout.
#[test]
fn stress_interleaved_updates_no_inversion() {
    let mut book = OrderBook::new(sym("META"));
    let mut seq = 1u64;

    // Build a valid spread: bid 199, ask 201
    book.apply_delta(set_bid(dec!(199), dec!(1), seq)).unwrap();
    seq += 1;
    book.apply_delta(set_ask(dec!(201), dec!(1), seq)).unwrap();
    seq += 1;

    // Tighten the spread: move ask down to 200 (still valid)
    book.apply_delta(set_ask(dec!(200), dec!(2), seq)).unwrap();
    seq += 1;

    // Tighten bid up to 199.50 (still valid)
    book.apply_delta(set_bid(dec!(199.5), dec!(3), seq)).unwrap();
    seq += 1;

    let spread = book.spread().unwrap();
    assert!(spread > dec!(0), "spread must remain positive: {spread}");
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(199.5));
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(200));
}
