// Integration tests: Order book advanced scenarios and cross-tick validation.

use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::tick::{Tick, TickFilter, TickReplayer};
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

fn set_delta(side: Side, p: &str, q: &str, seq: u64) -> BookDelta {
    BookDelta {
        side,
        price: price(p),
        quantity: qty(q),
        action: DeltaAction::Set,
        sequence: seq,
    }
}

fn remove_delta(side: Side, p: &str, seq: u64) -> BookDelta {
    BookDelta {
        side,
        price: price(p),
        quantity: Quantity::zero(),
        action: DeltaAction::Remove,
        sequence: seq,
    }
}

fn mk_tick(sym_s: &str, p: &str, q: &str, side: Side, ts: i64) -> Tick {
    Tick::new(sym(sym_s), price(p), qty(q), side, NanoTimestamp::new(ts))
}

// ── OrderBook: basic operations ──────────────────────────────────────────

#[test]
fn order_book_apply_single_bid_ask_delta() {
    let mut book = OrderBook::new(sym("BTC"));
    book.apply_delta(set_delta(Side::Bid, "50000", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "50100", "2", 2))
        .unwrap();

    assert_eq!(book.best_bid().unwrap().price.value(), dec!(50000));
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(50100));
}

#[test]
fn order_book_spread_calculation() {
    let mut book = OrderBook::new(sym("ETH"));
    book.apply_delta(set_delta(Side::Bid, "1000", "5", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "1002", "3", 2))
        .unwrap();
    assert_eq!(book.spread().unwrap(), dec!(2));
}

#[test]
fn order_book_mid_price() {
    let mut book = OrderBook::new(sym("SOL"));
    book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "102", "1", 2))
        .unwrap();
    let mid = book.mid_price().unwrap();
    assert_eq!(mid, dec!(101));
}

#[test]
fn order_book_sequence_validation_rejects_out_of_order() {
    let mut book = OrderBook::new(sym("X"));
    let result = book.apply_delta(set_delta(Side::Bid, "100", "1", 5));
    assert!(result.is_err(), "sequence 5 when expecting 1 should fail");
}

#[test]
fn order_book_sequence_must_be_sequential() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
        .unwrap();
    // Skip to 3 (should fail, expecting 2)
    let result = book.apply_delta(set_delta(Side::Bid, "101", "1", 3));
    assert!(result.is_err());
}

#[test]
fn order_book_remove_level_by_remove_action() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
        .unwrap();
    book.apply_delta(remove_delta(Side::Bid, "100", 2)).unwrap();
    assert!(book.best_bid().is_none());
}

#[test]
fn order_book_multiple_bid_levels_best_is_highest() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Bid, "99", "2", 2))
        .unwrap();
    book.apply_delta(set_delta(Side::Bid, "101", "3", 3))
        .unwrap();
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(101));
}

#[test]
fn order_book_multiple_ask_levels_best_is_lowest() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "200", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "201", "2", 2))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "199", "3", 3))
        .unwrap();
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(199));
}

#[test]
fn order_book_top_n_bids_ordered_descending() {
    let mut book = OrderBook::new(sym("X"));
    for (p, seq) in &[("100", 1u64), ("99", 2), ("98", 3), ("97", 4), ("96", 5)] {
        book.apply_delta(set_delta(Side::Bid, p, "1", *seq))
            .unwrap();
    }
    let top3 = book.top_bids(3);
    assert_eq!(top3.len(), 3);
    assert!(top3[0].price.value() > top3[1].price.value());
    assert!(top3[1].price.value() > top3[2].price.value());
}

#[test]
fn order_book_top_n_asks_ordered_ascending() {
    let mut book = OrderBook::new(sym("X"));
    for (p, seq) in &[("200", 1u64), ("201", 2), ("202", 3), ("203", 4)] {
        book.apply_delta(set_delta(Side::Ask, p, "1", *seq))
            .unwrap();
    }
    let top2 = book.top_asks(2);
    assert_eq!(top2.len(), 2);
    assert!(top2[0].price.value() < top2[1].price.value());
}

#[test]
fn order_book_vwap_zero_qty_returns_zero() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "100", "5", 1))
        .unwrap();
    let vwap = book.vwap_for_qty(Side::Ask, Quantity::zero()).unwrap();
    assert_eq!(vwap, dec!(0));
}

#[test]
fn order_book_vwap_single_level() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "200", "10", 1))
        .unwrap();
    let vwap = book.vwap_for_qty(Side::Ask, qty("5")).unwrap();
    assert_eq!(vwap, dec!(200));
}

#[test]
fn order_book_vwap_two_levels() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "100", "5", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "110", "5", 2))
        .unwrap();
    // 5 @ 100 + 5 @ 110 = (500 + 550) / 10 = 105
    let vwap = book.vwap_for_qty(Side::Ask, qty("10")).unwrap();
    assert_eq!(vwap, dec!(105));
}

#[test]
fn order_book_vwap_insufficient_liquidity() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "100", "3", 1))
        .unwrap();
    let result = book.vwap_for_qty(Side::Ask, qty("10"));
    assert!(result.is_err());
}

#[test]
fn order_book_empty_book_best_bid_none() {
    let book = OrderBook::new(sym("X"));
    assert!(book.best_bid().is_none());
}

#[test]
fn order_book_empty_book_spread_none() {
    let book = OrderBook::new(sym("X"));
    assert!(book.spread().is_none());
}

#[test]
fn order_book_sequence_advances_correctly() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
        .unwrap();
    assert_eq!(book.sequence(), 1);
    book.apply_delta(set_delta(Side::Bid, "100", "2", 2))
        .unwrap();
    assert_eq!(book.sequence(), 2);
    book.apply_delta(set_delta(Side::Bid, "100", "3", 3))
        .unwrap();
    assert_eq!(book.sequence(), 3);
}

#[test]
fn order_book_bid_count_tracks_levels() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Bid, "99", "1", 2))
        .unwrap();
    assert_eq!(book.bid_count(), 2);
}

#[test]
fn order_book_ask_count_tracks_levels() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "101", "1", 1))
        .unwrap();
    assert_eq!(book.ask_count(), 1);
}

// ── TickFilter scenarios ──────────────────────────────────────────────────

#[test]
fn tick_filter_no_predicates_matches_everything() {
    let filter = TickFilter::new();
    let t = mk_tick("AAPL", "100", "1", Side::Bid, 0);
    assert!(filter.matches(&t));
}

#[test]
fn tick_filter_by_symbol_passes_matching() {
    let filter = TickFilter::new().symbol(sym("AAPL"));
    let t = mk_tick("AAPL", "100", "1", Side::Bid, 0);
    assert!(filter.matches(&t));
}

#[test]
fn tick_filter_by_symbol_rejects_other() {
    let filter = TickFilter::new().symbol(sym("AAPL"));
    let t = mk_tick("MSFT", "100", "1", Side::Bid, 0);
    assert!(!filter.matches(&t));
}

#[test]
fn tick_filter_by_side_bid_passes() {
    let filter = TickFilter::new().side(Side::Bid);
    let t = mk_tick("X", "100", "1", Side::Bid, 0);
    assert!(filter.matches(&t));
}

#[test]
fn tick_filter_by_side_bid_rejects_ask() {
    let filter = TickFilter::new().side(Side::Bid);
    let t = mk_tick("X", "100", "1", Side::Ask, 0);
    assert!(!filter.matches(&t));
}

#[test]
fn tick_filter_min_quantity_passes_equal() {
    let filter = TickFilter::new().min_quantity(qty("5"));
    let t = mk_tick("X", "100", "5", Side::Ask, 0);
    assert!(filter.matches(&t));
}

#[test]
fn tick_filter_min_quantity_rejects_below() {
    let filter = TickFilter::new().min_quantity(qty("10"));
    let t = mk_tick("X", "100", "5", Side::Ask, 0);
    assert!(!filter.matches(&t));
}

#[test]
fn tick_filter_combined_all_conditions() {
    let filter = TickFilter::new()
        .symbol(sym("ETH"))
        .side(Side::Ask)
        .min_quantity(qty("3"));
    let pass = mk_tick("ETH", "2000", "5", Side::Ask, 0);
    let fail_sym = mk_tick("BTC", "2000", "5", Side::Ask, 0);
    let fail_side = mk_tick("ETH", "2000", "5", Side::Bid, 0);
    let fail_qty = mk_tick("ETH", "2000", "2", Side::Ask, 0);
    assert!(filter.matches(&pass));
    assert!(!filter.matches(&fail_sym));
    assert!(!filter.matches(&fail_side));
    assert!(!filter.matches(&fail_qty));
}

// ── TickReplayer scenarios ────────────────────────────────────────────────

#[test]
fn tick_replayer_returns_ticks_in_timestamp_order() {
    let ticks = vec![
        mk_tick("X", "100", "1", Side::Bid, 300),
        mk_tick("X", "101", "1", Side::Ask, 100),
        mk_tick("X", "102", "1", Side::Bid, 200),
    ];
    let mut replayer = TickReplayer::new(ticks);
    // Collect timestamps without holding references across calls
    let ts1 = replayer.next_tick().map(|t| t.timestamp.nanos());
    let ts2 = replayer.next_tick().map(|t| t.timestamp.nanos());
    let ts3 = replayer.next_tick().map(|t| t.timestamp.nanos());
    let done = replayer.next_tick().is_none();
    assert_eq!(ts1, Some(100));
    assert_eq!(ts2, Some(200));
    assert_eq!(ts3, Some(300));
    assert!(done);
}

#[test]
fn tick_replayer_remaining_decrements() {
    let ticks: Vec<Tick> = (0..5i64)
        .map(|i| mk_tick("X", "100", "1", Side::Ask, i))
        .collect();
    let mut replayer = TickReplayer::new(ticks);
    assert_eq!(replayer.remaining(), 5);
    let _ = replayer.next_tick();
    assert_eq!(replayer.remaining(), 4);
    let _ = replayer.next_tick();
    assert_eq!(replayer.remaining(), 3);
}

#[test]
fn tick_replayer_reset_replays_from_start() {
    let ticks = vec![
        mk_tick("X", "100", "1", Side::Bid, 1),
        mk_tick("X", "200", "1", Side::Ask, 2),
    ];
    let mut replayer = TickReplayer::new(ticks);
    let first = replayer.next_tick().map(|t| t.price.value());
    replayer.reset();
    let again = replayer.next_tick().map(|t| t.price.value());
    assert_eq!(first, again);
    assert_eq!(replayer.remaining(), 1);
}

#[test]
fn tick_replayer_empty_replay() {
    let mut replayer = TickReplayer::new(vec![]);
    assert!(replayer.next_tick().is_none());
    assert_eq!(replayer.remaining(), 0);
}

#[test]
fn tick_replayer_feeds_order_book() {
    let ticks = vec![
        mk_tick("ETH", "2000", "1", Side::Bid, 1),
        mk_tick("ETH", "2010", "2", Side::Ask, 2),
    ];
    let mut replayer = TickReplayer::new(ticks);
    let mut book = OrderBook::new(sym("ETH"));
    let mut seq = 1u64;

    while let Some(tick) = replayer.next_tick() {
        let delta = BookDelta {
            side: tick.side,
            price: tick.price,
            quantity: tick.quantity,
            action: DeltaAction::Set,
            sequence: seq,
        };
        book.apply_delta(delta).unwrap();
        seq += 1;
    }

    assert_eq!(book.best_bid().unwrap().price.value(), dec!(2000));
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(2010));
}

// ── Partial fill sequences ────────────────────────────────────────────────

/// A "partial fill" in the order book sense is a VWAP query that exhausts some
/// but not all levels. We verify the weighted average is correct.
#[test]
fn order_book_partial_fill_vwap_uses_only_required_levels() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "100", "5", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "200", "5", 2))
        .unwrap();
    // Buy 3: only touches level 1 (5 available at 100, only need 3)
    let vwap = book.vwap_for_qty(Side::Ask, qty("3")).unwrap();
    assert_eq!(
        vwap,
        dec!(100),
        "partial fill within first level should VWAP at that level's price"
    );
}

#[test]
fn order_book_partial_fill_crosses_multiple_levels() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "100", "2", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "110", "2", 2))
        .unwrap();
    book.apply_delta(set_delta(Side::Ask, "120", "2", 3))
        .unwrap();
    // Buy 5: 2@100 + 2@110 + 1@120 = (200+220+120)/5 = 540/5 = 108
    let vwap = book.vwap_for_qty(Side::Ask, qty("5")).unwrap();
    let expected = (dec!(2) * dec!(100) + dec!(2) * dec!(110) + dec!(1) * dec!(120)) / dec!(5);
    assert_eq!(vwap, expected);
}

#[test]
fn order_book_partial_fill_exactly_exhausts_book() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "50", "10", 1))
        .unwrap();
    // Requesting exactly all available quantity must succeed.
    let vwap = book.vwap_for_qty(Side::Ask, qty("10")).unwrap();
    assert_eq!(vwap, dec!(50));
}

#[test]
fn order_book_partial_fill_exceeds_book_returns_error() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "50", "3", 1))
        .unwrap();
    let result = book.vwap_for_qty(Side::Ask, qty("4"));
    assert!(
        result.is_err(),
        "requesting more than available liquidity must return InsufficientLiquidity"
    );
}

// ── Order cancellation (level removal) ───────────────────────────────────

#[test]
fn order_book_cancel_best_bid_reveals_next_level() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "102", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Bid, "100", "5", 2))
        .unwrap();
    // Cancel the best bid.
    book.apply_delta(remove_delta(Side::Bid, "102", 3)).unwrap();
    assert_eq!(
        book.best_bid().unwrap().price.value(),
        dec!(100),
        "after cancelling best bid the next level should become best"
    );
}

#[test]
fn order_book_cancel_non_best_level_does_not_change_best() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "102", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Bid, "100", "5", 2))
        .unwrap();
    // Cancel a non-best level.
    book.apply_delta(remove_delta(Side::Bid, "100", 3)).unwrap();
    assert_eq!(
        book.best_bid().unwrap().price.value(),
        dec!(102),
        "cancelling a non-best level must not change best bid"
    );
    assert_eq!(book.bid_count(), 1);
}

#[test]
fn order_book_cancel_all_bids_leaves_empty_side() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "100", "1", 1))
        .unwrap();
    book.apply_delta(set_delta(Side::Bid, "99", "1", 2))
        .unwrap();
    book.apply_delta(remove_delta(Side::Bid, "100", 3)).unwrap();
    book.apply_delta(remove_delta(Side::Bid, "99", 4)).unwrap();
    assert!(book.best_bid().is_none());
    assert_eq!(book.bid_count(), 0);
}

#[test]
fn order_book_cancel_then_rebook_same_level() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "200", "5", 1))
        .unwrap();
    book.apply_delta(remove_delta(Side::Ask, "200", 2)).unwrap();
    assert!(book.best_ask().is_none());
    // Re-book the same price level.
    book.apply_delta(set_delta(Side::Ask, "200", "3", 3))
        .unwrap();
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(200));
    assert_eq!(book.best_ask().unwrap().quantity.value(), dec!(3));
}

// ── Book reconstruction from tick stream ─────────────────────────────────

#[test]
fn order_book_reconstruction_from_tick_stream_yields_correct_spread() {
    // Simulate a full book snapshot arriving as sequential deltas.
    let mut book = OrderBook::new(sym("AMZN"));
    let bid_levels = [("3500", "10"), ("3499", "20"), ("3498", "30")];
    let ask_levels = [("3501", "8"), ("3502", "15"), ("3503", "25")];
    let mut seq = 1u64;
    for (p, q) in &bid_levels {
        book.apply_delta(set_delta(Side::Bid, p, q, seq)).unwrap();
        seq += 1;
    }
    for (p, q) in &ask_levels {
        book.apply_delta(set_delta(Side::Ask, p, q, seq)).unwrap();
        seq += 1;
    }
    assert_eq!(book.best_bid().unwrap().price.value(), dec!(3500));
    assert_eq!(book.best_ask().unwrap().price.value(), dec!(3501));
    assert_eq!(book.spread().unwrap(), dec!(1));
}

#[test]
fn order_book_reconstruction_update_existing_level_changes_quantity() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Bid, "100", "10", 1))
        .unwrap();
    // Update quantity at same price.
    book.apply_delta(set_delta(Side::Bid, "100", "25", 2))
        .unwrap();
    assert_eq!(book.best_bid().unwrap().quantity.value(), dec!(25));
    assert_eq!(
        book.bid_count(),
        1,
        "update at existing price must not add a new level"
    );
}

// ── Symbol/Price/Quantity validation ─────────────────────────────────────

#[test]
fn symbol_empty_is_invalid() {
    assert!(Symbol::new("").is_err());
}

#[test]
fn symbol_whitespace_is_invalid() {
    assert!(Symbol::new("AA PL").is_err());
}

#[test]
fn symbol_valid_alphanumeric() {
    assert!(Symbol::new("BTC-USD").is_ok());
    assert!(Symbol::new("AAPL").is_ok());
    assert!(Symbol::new("sp500").is_ok());
}

#[test]
fn price_zero_is_invalid() {
    assert!(Price::new(dec!(0)).is_err());
}

#[test]
fn price_negative_is_invalid() {
    assert!(Price::new(dec!(-1)).is_err());
}

#[test]
fn price_small_positive_is_valid() {
    assert!(Price::new(dec!(0.001)).is_ok());
}

#[test]
fn quantity_zero_is_valid() {
    assert!(Quantity::new(dec!(0)).is_ok());
}

#[test]
fn quantity_negative_is_invalid() {
    assert!(Quantity::new(dec!(-1)).is_err());
}

#[test]
fn quantity_zero_method() {
    assert_eq!(Quantity::zero().value(), dec!(0));
}

#[test]
fn price_level_quantities_accessible() {
    let mut book = OrderBook::new(sym("TEST"));
    book.apply_delta(set_delta(Side::Bid, "100", "42", 1))
        .unwrap();
    let level = book.best_bid().unwrap();
    assert_eq!(level.price.value(), dec!(100));
    assert_eq!(level.quantity.value(), dec!(42));
}

#[test]
fn tick_notional_is_price_times_quantity() {
    let t = mk_tick("AAPL", "150", "10", Side::Ask, 0);
    assert_eq!(t.notional(), dec!(1500));
}

#[test]
fn tick_notional_zero_quantity() {
    let t = mk_tick("X", "100", "0", Side::Ask, 0);
    assert_eq!(t.notional(), dec!(0));
}
