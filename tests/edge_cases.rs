/// Edge case tests for fin-primitives.
///
/// Covers:
/// - Zero-quantity fills (buy and sell)
/// - Crossed book state rejection
/// - RSI with fewer than 14 periods of data
/// - PnL accounting identity: sum of fills equals position value
/// - Invalid price construction
/// - Duplicate price level (order "ID" equivalents)

use fin_primitives::error::FinError;
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::position::{Fill, Position, PositionLedger};
use fin_primitives::signals::indicators::Rsi;
use fin_primitives::signals::{Signal, SignalValue};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn sym(s: &str) -> Symbol {
    Symbol::new(s).unwrap()
}

fn p(v: Decimal) -> Price {
    Price::new(v).unwrap()
}

fn q(v: Decimal) -> Quantity {
    Quantity::new(v).unwrap()
}

fn fill(symbol: &str, side: Side, qty: Decimal, price: Decimal, comm: Decimal) -> Fill {
    Fill {
        symbol: sym(symbol),
        side,
        quantity: q(qty),
        price: p(price),
        timestamp: NanoTimestamp(0),
        commission: comm,
    }
}

fn set_delta(side: Side, price: &str, qty: &str, seq: u64) -> BookDelta {
    BookDelta {
        side,
        price: Price::new(price.parse().unwrap()).unwrap(),
        quantity: Quantity::new(qty.parse().unwrap()).unwrap(),
        action: DeltaAction::Set,
        sequence: seq,
    }
}

fn remove_delta(side: Side, price: &str, seq: u64) -> BookDelta {
    BookDelta {
        side,
        price: Price::new(price.parse().unwrap()).unwrap(),
        quantity: Quantity::zero(),
        action: DeltaAction::Remove,
        sequence: seq,
    }
}

fn bar(close: Decimal) -> fin_primitives::ohlcv::OhlcvBar {
    let pr = p(close);
    fin_primitives::ohlcv::OhlcvBar {
        symbol: sym("X"),
        open: pr,
        high: pr,
        low: pr,
        close: pr,
        volume: q(dec!(0)),
        ts_open: NanoTimestamp(0),
        ts_close: NanoTimestamp(1),
        tick_count: 1,
    }
}

// ── Zero-quantity fills ───────────────────────────────────────────────────────

/// A buy fill of zero quantity leaves the position flat and changes no cash.
#[test]
fn zero_quantity_buy_fill_leaves_position_flat() {
    let mut pos = Position::new(sym("AAPL"));
    let pnl = pos.apply_fill(&fill("AAPL", Side::Bid, dec!(0), dec!(100), dec!(0))).unwrap();
    assert_eq!(pnl, dec!(0));
    assert!(pos.is_flat());
    assert_eq!(pos.quantity, dec!(0));
}

/// A sell fill of zero quantity on an existing long position should not change the position.
#[test]
fn zero_quantity_sell_fill_does_not_change_long_position() {
    let mut pos = Position::new(sym("AAPL"));
    pos.apply_fill(&fill("AAPL", Side::Bid, dec!(10), dec!(100), dec!(0))).unwrap();
    let qty_before = pos.quantity;
    pos.apply_fill(&fill("AAPL", Side::Ask, dec!(0), dec!(110), dec!(0))).unwrap();
    assert_eq!(pos.quantity, qty_before, "zero-quantity sell must not reduce position");
}

/// A zero-quantity fill via the ledger still succeeds and keeps cash unchanged.
#[test]
fn zero_quantity_fill_ledger_cash_unchanged() {
    let mut ledger = PositionLedger::new(dec!(10_000));
    let cash_before = ledger.cash();
    ledger.apply_fill(Fill {
        symbol: sym("BTC"),
        side: Side::Bid,
        quantity: q(dec!(0)),
        price: p(dec!(50_000)),
        timestamp: NanoTimestamp(0),
        commission: dec!(0),
    }).unwrap();
    assert_eq!(ledger.cash(), cash_before, "zero-quantity buy must not debit cash");
}

// ── Crossed book state rejection ──────────────────────────────────────────────

/// Setting a bid at or above the existing ask must be rejected.
#[test]
fn crossed_book_bid_above_ask_rejected() {
    let mut book = OrderBook::new(sym("BTC"));
    book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
    let result = book.apply_delta(set_delta(Side::Bid, "100", "5", 2));
    assert!(
        matches!(result, Err(FinError::InvertedSpread { .. })),
        "bid at same price as ask must produce InvertedSpread, got {:?}",
        result
    );
}

/// Setting an ask at or below the existing bid must be rejected.
#[test]
fn crossed_book_ask_below_bid_rejected() {
    let mut book = OrderBook::new(sym("ETH"));
    book.apply_delta(set_delta(Side::Bid, "200", "3", 1)).unwrap();
    let result = book.apply_delta(set_delta(Side::Ask, "200", "3", 2));
    assert!(
        matches!(result, Err(FinError::InvertedSpread { .. })),
        "ask at same price as bid must produce InvertedSpread, got {:?}",
        result
    );
}

/// The book sequence counter must not advance when a delta is rejected.
#[test]
fn crossed_book_sequence_does_not_advance_on_rejection() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
    let _ = book.apply_delta(set_delta(Side::Bid, "101", "5", 2));
    assert_eq!(
        book.sequence(),
        1,
        "sequence must stay at 1 after rejected delta"
    );
}

/// The rejected level must not persist in the book state.
#[test]
fn crossed_book_rejected_bid_not_in_book() {
    let mut book = OrderBook::new(sym("X"));
    book.apply_delta(set_delta(Side::Ask, "100", "5", 1)).unwrap();
    let _ = book.apply_delta(set_delta(Side::Bid, "101", "5", 2));
    assert!(
        book.best_bid().is_none(),
        "rejected bid must not appear in book after rollback"
    );
}

// ── Duplicate price-level updates (analogous to duplicate order at same price) ──

/// Setting the same bid price twice is valid; the second call updates the quantity.
#[test]
fn duplicate_price_level_update_replaces_quantity() {
    let mut book = OrderBook::new(sym("AAPL"));
    book.apply_delta(set_delta(Side::Bid, "150", "10", 1)).unwrap();
    book.apply_delta(set_delta(Side::Bid, "150", "25", 2)).unwrap();
    let best = book.best_bid().unwrap();
    assert_eq!(best.price.value(), dec!(150));
    assert_eq!(best.quantity.value(), dec!(25), "second set at same price must overwrite quantity");
    assert_eq!(book.bid_count(), 1, "should still have exactly one bid level");
}

/// Setting the same ask price twice is valid; the second call updates the quantity.
#[test]
fn duplicate_ask_price_level_update_replaces_quantity() {
    let mut book = OrderBook::new(sym("MSFT"));
    book.apply_delta(set_delta(Side::Ask, "300", "5", 1)).unwrap();
    book.apply_delta(set_delta(Side::Ask, "300", "20", 2)).unwrap();
    let best = book.best_ask().unwrap();
    assert_eq!(best.price.value(), dec!(300));
    assert_eq!(best.quantity.value(), dec!(20));
    assert_eq!(book.ask_count(), 1);
}

// ── Invalid price construction ────────────────────────────────────────────────

#[test]
fn price_zero_returns_invalid_price_error() {
    let result = Price::new(dec!(0));
    assert!(matches!(result, Err(FinError::InvalidPrice(_))));
}

#[test]
fn price_negative_returns_invalid_price_error() {
    let result = Price::new(dec!(-0.01));
    assert!(matches!(result, Err(FinError::InvalidPrice(_))));
}

#[test]
fn price_very_small_positive_is_valid() {
    assert!(Price::new(dec!(0.000001)).is_ok());
}

#[test]
fn quantity_negative_returns_invalid_quantity_error() {
    let result = Quantity::new(dec!(-1));
    assert!(matches!(result, Err(FinError::InvalidQuantity(_))));
}

#[test]
fn symbol_empty_string_returns_invalid_symbol_error() {
    let result = Symbol::new("");
    assert!(matches!(result, Err(FinError::InvalidSymbol(_))));
}

#[test]
fn symbol_tab_character_returns_invalid_symbol_error() {
    let result = Symbol::new("AA\tPL");
    assert!(matches!(result, Err(FinError::InvalidSymbol(_))));
}

// ── RSI with fewer than `period` data points ─────────────────────────────────

/// RSI with period 14 must return Unavailable for each of the first 14 bars
/// (the first close establishes prev_close, then 14 changes are needed to seed
/// the Wilder average). Bars 1 through 14 produce Unavailable; bar 15 first
/// produces a Scalar.
#[test]
fn rsi_period_14_unavailable_for_first_14_bars() {
    let mut rsi = Rsi::new("rsi14", 14);
    let prices = [
        dec!(44), dec!(44), dec!(44), dec!(44), dec!(44),
        dec!(44), dec!(44), dec!(44), dec!(44), dec!(44),
        dec!(44), dec!(44), dec!(44), dec!(44),
    ];
    for price in &prices {
        let val = rsi.update(&bar(*price)).unwrap();
        assert!(
            matches!(val, SignalValue::Unavailable),
            "expected Unavailable for < period bars, got {:?}", val
        );
    }
    assert!(!rsi.is_ready(), "RSI must not be ready after exactly 14 bars (need period+1 changes)");
}

/// After period + 1 bars the RSI must return a Scalar.
#[test]
fn rsi_period_14_produces_scalar_after_period_plus_one_bars() {
    let mut rsi = Rsi::new("rsi14", 14);
    let prices: Vec<Decimal> = (0..15).map(|i| dec!(100) + Decimal::from(i)).collect();
    let mut last = SignalValue::Unavailable;
    for p in &prices {
        last = rsi.update(&bar(*p)).unwrap();
    }
    assert!(
        matches!(last, SignalValue::Scalar(_)),
        "RSI must produce Scalar after period+1 bars"
    );
    assert!(rsi.is_ready());
}

/// RSI with period 3 needs 4 bars (3 changes for seed, 1 more to confirm ready).
#[test]
fn rsi_period_3_unavailable_for_first_3_bars() {
    let mut rsi = Rsi::new("rsi3", 3);
    let _ = rsi.update(&bar(dec!(100))).unwrap();
    let v1 = rsi.update(&bar(dec!(101))).unwrap();
    let v2 = rsi.update(&bar(dec!(102))).unwrap();
    assert!(matches!(v1, SignalValue::Unavailable));
    assert!(matches!(v2, SignalValue::Unavailable));
    assert!(!rsi.is_ready());
    let v3 = rsi.update(&bar(dec!(103))).unwrap();
    assert!(matches!(v3, SignalValue::Scalar(_)));
    assert!(rsi.is_ready());
}

/// All-loss scenario: RSI should be 0 (or very close) when all bars are down.
#[test]
fn rsi_all_losses_approaches_zero() {
    let mut rsi = Rsi::new("rsi3", 3);
    // 5 bars, each lower than the last → all losses, no gains.
    let prices = [dec!(100), dec!(90), dec!(80), dec!(70), dec!(60)];
    let mut last_val: Option<Decimal> = None;
    for price in &prices {
        if let SignalValue::Scalar(v) = rsi.update(&bar(*price)).unwrap() {
            last_val = Some(v);
        }
    }
    let val = last_val.expect("RSI must be Scalar after period+1 bars");
    assert_eq!(val, dec!(0), "all losses should produce RSI = 0");
}

// ── PnL accounting identity ───────────────────────────────────────────────────

/// The accounting identity: cash + market_value(position) = initial_cash + realized_pnl
/// when the price equals avg_cost (i.e., unrealized PnL is zero).
///
/// Stated another way: after buying N units at cost P and selling M of them at price P2,
/// the ledger equity at mark price P equals:
///   initial_cash - (N * P + commission_buy) + (M * P2 - commission_sell)
///                + (N - M) * P (unrealized, mark = P2 if we use P2)
///
/// We verify the simpler identity: sum of all fill notionals net of commissions
/// equals the net change in cash.
#[test]
fn pnl_accounting_identity_buy_then_sell_net_cash_change() {
    let initial_cash = dec!(10_000);
    let mut ledger = PositionLedger::new(initial_cash);

    // Buy 10 shares at 100 with $1 commission.
    ledger.apply_fill(Fill {
        symbol: sym("AAPL"),
        side: Side::Bid,
        quantity: q(dec!(10)),
        price: p(dec!(100)),
        timestamp: NanoTimestamp(0),
        commission: dec!(1),
    }).unwrap();

    // Sell 10 shares at 110 with $1 commission.
    ledger.apply_fill(Fill {
        symbol: sym("AAPL"),
        side: Side::Ask,
        quantity: q(dec!(10)),
        price: p(dec!(110)),
        timestamp: NanoTimestamp(0),
        commission: dec!(1),
    }).unwrap();

    // Expected: cash = 10_000 - (10*100 + 1) + (10*110 - 1) = 10_000 - 1001 + 1099 = 10_098
    let expected_cash = initial_cash - (dec!(10) * dec!(100) + dec!(1)) + (dec!(10) * dec!(110) - dec!(1));
    assert_eq!(ledger.cash(), expected_cash);

    // Position is flat.
    let pos = ledger.position(&sym("AAPL")).unwrap();
    assert!(pos.is_flat());

    // Equity at any price = cash when flat.
    let prices: HashMap<String, Price> = HashMap::new();
    let equity = ledger.equity(&prices).unwrap();
    assert_eq!(equity, expected_cash);

    // Realized PnL = (110 - 100) * 10 - 1 (buy commission) - 1 (sell commission) = 98.
    // Both buy and sell commissions are charged against realized_pnl via Position::apply_fill.
    assert_eq!(ledger.realized_pnl_total(), dec!(98));
}

/// Multiple partial fills sum correctly: total notional bought equals qty * avg_cost.
#[test]
fn pnl_accounting_identity_multiple_buys_avg_cost_invariant() {
    let mut pos = Position::new(sym("X"));

    // Three buys: 10@100, 5@120, 15@110.
    let fills: Vec<(Decimal, Decimal)> = vec![
        (dec!(10), dec!(100)),
        (dec!(5), dec!(120)),
        (dec!(15), dec!(110)),
    ];

    let mut total_notional = dec!(0);
    let mut total_qty = dec!(0);
    for (qty_val, price_val) in &fills {
        total_notional += qty_val * price_val;
        total_qty += qty_val;
        pos.apply_fill(&Fill {
            symbol: sym("X"),
            side: Side::Bid,
            quantity: q(*qty_val),
            price: p(*price_val),
            timestamp: NanoTimestamp(0),
            commission: dec!(0),
        }).unwrap();
    }

    // avg_cost must equal total_notional / total_qty.
    let expected_avg_cost = total_notional / total_qty;
    assert_eq!(
        pos.avg_cost, expected_avg_cost,
        "average cost must equal total notional / total quantity"
    );
    assert_eq!(pos.quantity, total_qty);

    // market_value at avg_cost ≈ total_notional (within Decimal rounding of repeating fraction).
    let mv = pos.market_value(p(expected_avg_cost));
    let mv_diff = (mv - total_notional).abs();
    assert!(mv_diff < dec!(0.01), "market_value must be within 0.01 of total_notional, diff={mv_diff}");

    // unrealized_pnl at avg_cost ≈ 0 (within Decimal rounding of repeating fraction).
    let upnl = pos.unrealized_pnl(p(expected_avg_cost));
    assert!(upnl.abs() < dec!(0.01), "unrealized_pnl at avg_cost must be near 0, got {upnl}");
}

/// Realized PnL identity: the sum of all per-fill realized P&L equals the
/// total realized_pnl field on the position.
#[test]
fn pnl_accounting_identity_realized_pnl_sums_correctly() {
    let mut pos = Position::new(sym("X"));
    pos.apply_fill(&Fill {
        symbol: sym("X"), side: Side::Bid,
        quantity: q(dec!(20)), price: p(dec!(50)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();

    // Sell in three batches.
    let sell_fills: Vec<(Decimal, Decimal)> = vec![
        (dec!(5), dec!(60)),
        (dec!(10), dec!(55)),
        (dec!(5), dec!(65)),
    ];

    let mut expected_realized = dec!(0);
    for (qty_val, price_val) in &sell_fills {
        let pnl = pos.apply_fill(&Fill {
            symbol: sym("X"), side: Side::Ask,
            quantity: q(*qty_val), price: p(*price_val),
            timestamp: NanoTimestamp(0), commission: dec!(0),
        }).unwrap();
        expected_realized += pnl;
    }

    assert_eq!(
        pos.realized_pnl, expected_realized,
        "sum of per-fill realized PnL must equal cumulative realized_pnl field"
    );

    // Manually compute: each sell realizes qty * (sell_price - avg_cost).
    // avg_cost = 50 throughout (only sells here).
    let manual: Decimal = sell_fills.iter()
        .map(|(qty_val, price_val)| qty_val * (price_val - dec!(50)))
        .sum();
    assert_eq!(pos.realized_pnl, manual);
}
