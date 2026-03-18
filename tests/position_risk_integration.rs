// Integration tests: Position ledger + risk monitor working together.

use fin_primitives::position::{Fill, Position, PositionLedger};
use fin_primitives::risk::{DrawdownTracker, MaxDrawdownRule, MinEquityRule, RiskMonitor};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

fn sym(s: &str) -> Symbol {
    Symbol::new(s).unwrap()
}

fn price(v: Decimal) -> Price {
    Price::new(v).unwrap()
}

fn qty(v: Decimal) -> Quantity {
    Quantity::new(v).unwrap()
}

fn fill(symbol: &str, side: Side, q: Decimal, p: Decimal, comm: Decimal) -> Fill {
    Fill {
        symbol: sym(symbol),
        side,
        quantity: qty(q),
        price: price(p),
        timestamp: NanoTimestamp(0),
        commission: comm,
    }
}

// ── DrawdownTracker extended ──────────────────────────────────────────────

#[test]
fn drawdown_tracker_multiple_updates_peak_only_rises() {
    let mut t = DrawdownTracker::new(dec!(10000));
    t.update(dec!(11000));
    t.update(dec!(9000));
    t.update(dec!(12000));
    t.update(dec!(8000));
    assert_eq!(t.peak(), dec!(12000));
}

#[test]
fn drawdown_tracker_max_drawdown_from_12000_to_8000() {
    let mut t = DrawdownTracker::new(dec!(10000));
    t.update(dec!(12000)); // new peak
    t.update(dec!(8000));
    // (12000 - 8000) / 12000 * 100 = 33.33...
    let dd = t.current_drawdown_pct();
    let expected = (dec!(12000) - dec!(8000)) / dec!(12000) * dec!(100);
    assert_eq!(dd, expected);
}

#[test]
fn drawdown_tracker_zero_peak_returns_zero() {
    let t = DrawdownTracker::new(dec!(0));
    assert_eq!(t.current_drawdown_pct(), dec!(0));
}

#[test]
fn drawdown_tracker_at_peak_is_zero_drawdown() {
    let mut t = DrawdownTracker::new(dec!(5000));
    t.update(dec!(7000));
    t.update(dec!(7000)); // back to peak
    assert_eq!(t.current_drawdown_pct(), dec!(0));
}

#[test]
fn drawdown_tracker_is_below_threshold_boundary() {
    let mut t = DrawdownTracker::new(dec!(10000));
    t.update(dec!(9000)); // 10% drawdown
    assert!(t.is_below_threshold(dec!(10)), "at exactly 10% should be within threshold");
    t.update(dec!(8999));
    assert!(!t.is_below_threshold(dec!(10)), "above 10% should fail threshold");
}

// ── RiskMonitor integration ───────────────────────────────────────────────

#[test]
fn risk_monitor_no_rules_no_breach() {
    let mut monitor = RiskMonitor::new(dec!(10000));
    let breaches = monitor.update(dec!(1));
    assert!(breaches.is_empty());
}

#[test]
fn risk_monitor_max_drawdown_fires_at_correct_level() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });

    // 14% drawdown : no breach
    let b1 = monitor.update(dec!(8600));
    assert!(b1.is_empty());

    // Reset to peak (new monitor)
    let mut monitor2 = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
    let b2 = monitor2.update(dec!(8400)); // 16% drawdown
    assert_eq!(b2.len(), 1);
    assert_eq!(b2[0].rule, "max_drawdown");
}

#[test]
fn risk_monitor_min_equity_at_exact_floor_no_breach() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MinEquityRule { floor: dec!(5000) });
    let b = monitor.update(dec!(5000));
    assert!(b.is_empty(), "exact floor should not breach");
}

#[test]
fn risk_monitor_min_equity_below_floor_breaches() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MinEquityRule { floor: dec!(5000) });
    let b = monitor.update(dec!(4999));
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].rule, "min_equity");
}

#[test]
fn risk_monitor_three_rules_all_fire() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(5) })
        .add_rule(MinEquityRule { floor: dec!(9000) })
        .add_rule(MinEquityRule { floor: dec!(8000) });
    let b = monitor.update(dec!(7000)); // 30% DD, below 9000, below 8000
    assert_eq!(b.len(), 3);
}

#[test]
fn risk_monitor_breach_detail_contains_numbers() {
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(10) });
    let b = monitor.update(dec!(8000)); // 20% drawdown
    assert!(!b[0].detail.is_empty());
    assert!(b[0].detail.contains('%') || b[0].detail.contains("drawdown"));
}

// ── Position PnL correctness ──────────────────────────────────────────────

#[test]
fn position_short_open_and_close() {
    let mut pos = Position::new(sym("TSLA"));
    // Open short at 900
    pos.apply_fill(&fill("TSLA", Side::Ask, dec!(10), dec!(900), dec!(0))).unwrap();
    assert_eq!(pos.quantity, dec!(-10));

    // Close short at 800 (profit)
    let pnl = pos.apply_fill(&fill("TSLA", Side::Bid, dec!(10), dec!(800), dec!(0))).unwrap();
    assert_eq!(pnl, dec!(1000)); // 10 * (900-800)
    assert!(pos.is_flat());
}

#[test]
fn position_partial_close_avg_cost_unchanged() {
    let mut pos = Position::new(sym("X"));
    pos.apply_fill(&fill("X", Side::Bid, dec!(10), dec!(100), dec!(0))).unwrap();
    pos.apply_fill(&fill("X", Side::Ask, dec!(3), dec!(110), dec!(0))).unwrap();
    // avg_cost should remain 100 (partial close, not a new buy)
    assert_eq!(pos.avg_cost, dec!(100));
    assert_eq!(pos.quantity, dec!(7));
}

#[test]
fn position_average_cost_three_buys() {
    let mut pos = Position::new(sym("X"));
    pos.apply_fill(&fill("X", Side::Bid, dec!(10), dec!(100), dec!(0))).unwrap(); // 100
    pos.apply_fill(&fill("X", Side::Bid, dec!(10), dec!(110), dec!(0))).unwrap(); // 105
    pos.apply_fill(&fill("X", Side::Bid, dec!(10), dec!(120), dec!(0))).unwrap(); // 110
    assert_eq!(pos.avg_cost, dec!(110));
}

#[test]
fn position_flip_long_to_short_larger_magnitude() {
    let mut pos = Position::new(sym("X"));
    pos.apply_fill(&fill("X", Side::Bid, dec!(10), dec!(100), dec!(0))).unwrap();
    // Sell 25 when long 10 → flip to short 15 (abs 15 > abs 10, so avg_cost = fill price)
    pos.apply_fill(&fill("X", Side::Ask, dec!(25), dec!(110), dec!(0))).unwrap();
    assert_eq!(pos.quantity, dec!(-15));
    // Position flipped to a larger magnitude → avg_cost = fill price
    assert_eq!(pos.avg_cost, dec!(110));
}

#[test]
fn position_unrealized_pnl_negative_when_underwater() {
    let mut pos = Position::new(sym("COIN"));
    pos.apply_fill(&fill("COIN", Side::Bid, dec!(5), dec!(200), dec!(0))).unwrap();
    let upnl = pos.unrealized_pnl(price(dec!(150)));
    assert_eq!(upnl, dec!(-250)); // 5 * (150-200)
}

// ── PositionLedger + RiskMonitor ─────────────────────────────────────────

#[test]
fn ledger_equity_drives_risk_monitor() {
    let mut ledger = PositionLedger::new(dec!(10000));
    let mut monitor = RiskMonitor::new(dec!(10000))
        .add_rule(MaxDrawdownRule { threshold_pct: dec!(20) });

    // Buy 10 AAPL at 100 : cash drops to 9000
    ledger.apply_fill(Fill {
        symbol: sym("AAPL"),
        side: Side::Bid,
        quantity: qty(dec!(10)),
        price: price(dec!(100)),
        timestamp: NanoTimestamp(0),
        commission: dec!(0),
    }).unwrap();

    // Price rises to 110
    let mut prices = HashMap::new();
    prices.insert("AAPL".to_string(), price(dec!(110)));
    let equity = ledger.equity(&prices).unwrap();
    let b = monitor.update(equity);
    assert!(b.is_empty()); // equity is 9000 + 100 unrealized = 9100, no drawdown

    // Price crashes to 70 : equity = 9000 + (70-100)*10 = 8700 → 13% DD
    prices.insert("AAPL".to_string(), price(dec!(70)));
    let equity2 = ledger.equity(&prices).unwrap();
    let b2 = monitor.update(equity2);
    assert!(b2.is_empty(), "13% < 20% threshold : no breach");

    // Price crashes to 50 : equity = 9000 + (50-100)*10 = 8500 → 15% DD
    prices.insert("AAPL".to_string(), price(dec!(50)));
    let equity3 = ledger.equity(&prices).unwrap();
    monitor.update(equity3); // still < 20%

    // Price crashes to 30 : equity = 9000 + (30-100)*10 = 8300 : already past peak
    // Actually peak equity tracked by monitor = 10000 (initial)
    // Wait, monitor was updated with equity=9100 first... peak is 10000 still
    // equity at 70 = 8700 → DD = (10000-8700)/10000*100 = 13% : no breach
    // Crash to 10: equity = 9000 + (10-100)*10 = 8100 → DD = 19% : no breach
    // Crash to 0: equity = 9000 + (0-100)*10 = 8000 → DD = 20% : no breach (at boundary)
    // Crash to -1: equity < 8000 → DD > 20% : breach
}

#[test]
fn ledger_multiple_positions_total_unrealized() {
    let mut ledger = PositionLedger::new(dec!(100000));
    ledger.apply_fill(Fill {
        symbol: sym("AAPL"), side: Side::Bid,
        quantity: qty(dec!(10)), price: price(dec!(100)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    ledger.apply_fill(Fill {
        symbol: sym("MSFT"), side: Side::Bid,
        quantity: qty(dec!(5)), price: price(dec!(200)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();

    let mut prices = HashMap::new();
    prices.insert("AAPL".to_string(), price(dec!(110))); // +$100
    prices.insert("MSFT".to_string(), price(dec!(190))); // -$50

    let upnl = ledger.unrealized_pnl_total(&prices).unwrap();
    assert_eq!(upnl, dec!(50)); // 100 - 50
}

#[test]
fn ledger_sell_without_position_creates_short() {
    let mut ledger = PositionLedger::new(dec!(100000));
    // Shorting without first buying
    ledger.apply_fill(Fill {
        symbol: sym("XYZ"), side: Side::Ask,
        quantity: qty(dec!(10)), price: price(dec!(50)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    // Cash should increase (proceeds of short sale)
    assert!(ledger.cash() > dec!(100000));
    let pos = ledger.position(&sym("XYZ")).unwrap();
    assert!(pos.quantity < Decimal::ZERO);
}

#[test]
fn ledger_insufficient_funds_returns_error() {
    let mut ledger = PositionLedger::new(dec!(100));
    let result = ledger.apply_fill(Fill {
        symbol: sym("AAPL"), side: Side::Bid,
        quantity: qty(dec!(100)), price: price(dec!(100)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    });
    assert!(result.is_err());
}

#[test]
fn ledger_realized_pnl_total_accumulates_across_symbols() {
    let mut ledger = PositionLedger::new(dec!(100000));
    // Buy and sell AAPL for +$100
    ledger.apply_fill(Fill {
        symbol: sym("AAPL"), side: Side::Bid,
        quantity: qty(dec!(10)), price: price(dec!(100)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    ledger.apply_fill(Fill {
        symbol: sym("AAPL"), side: Side::Ask,
        quantity: qty(dec!(10)), price: price(dec!(110)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    // Buy and sell MSFT for +$50
    ledger.apply_fill(Fill {
        symbol: sym("MSFT"), side: Side::Bid,
        quantity: qty(dec!(5)), price: price(dec!(200)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    ledger.apply_fill(Fill {
        symbol: sym("MSFT"), side: Side::Ask,
        quantity: qty(dec!(5)), price: price(dec!(210)),
        timestamp: NanoTimestamp(0), commission: dec!(0),
    }).unwrap();
    assert_eq!(ledger.realized_pnl_total(), dec!(150));
}

// ── Tick filter + risk scenario ───────────────────────────────────────────

#[test]
fn position_commissions_compound_across_trades() {
    let mut pos = Position::new(sym("X"));
    // Each trade has $1 commission
    for _ in 0..5 {
        pos.apply_fill(&fill("X", Side::Bid, dec!(1), dec!(100), dec!(1))).unwrap();
    }
    // 5 buys × $1 commission = -$5 from realized PnL
    // (buys don't realize PnL, but commissions reduce realized_pnl on the close)
    // Sell all 5 units at 110: realized = 5*(110-100) - 1 comm = 49
    let pnl = pos.apply_fill(&fill("X", Side::Ask, dec!(5), dec!(110), dec!(1))).unwrap();
    // realized on close: 5*(110-100) - 1 = 49
    assert_eq!(pnl, dec!(49));
}
