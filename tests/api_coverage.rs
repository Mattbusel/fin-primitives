//! Additional tests covering public API paths not exercised by the existing test suite.
//!
//! Targets:
//! - `NanoTimestamp::now()` produces a positive value
//! - `NanoTimestamp::to_datetime()` round-trips correctly
//! - `TickFilter::default()` is equivalent to `TickFilter::new()`
//! - `OhlcvSeries::last()` and `OhlcvSeries::get()`
//! - `Timeframe::Days` variant and `to_nanos`
//! - `DrawdownTracker::current_drawdown_pct()` with zero peak equity
//! - `PositionLedger::realized_pnl_total()` with no fills
//! - `PositionLedger::position()` returns `None` for unknown symbol
//! - `RiskMonitor::update()` with no rules returns empty vec

use fin_primitives::ohlcv::{OhlcvBar, OhlcvSeries, Timeframe};
use fin_primitives::position::PositionLedger;
use fin_primitives::risk::{DrawdownTracker, RiskMonitor};
use fin_primitives::tick::{Tick, TickFilter};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

fn make_bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
    let op = Price::new(o.parse().unwrap()).unwrap();
    let hp = Price::new(h.parse().unwrap()).unwrap();
    let lp = Price::new(l.parse().unwrap()).unwrap();
    let cp = Price::new(c.parse().unwrap()).unwrap();
    OhlcvBar {
        symbol: Symbol::new("X").unwrap(),
        open: op,
        high: hp,
        low: lp,
        close: cp,
        volume: Quantity::zero(),
        ts_open: NanoTimestamp::new(0),
        ts_close: NanoTimestamp::new(1),
        tick_count: 1,
    }
}

// ── NanoTimestamp ─────────────────────────────────────────────────────────────

#[test]
fn test_nano_timestamp_now_is_positive() {
    let ts = NanoTimestamp::now();
    assert!(
        ts.nanos() > 0,
        "NanoTimestamp::now() should return a positive value"
    );
}

#[test]
fn test_nano_timestamp_to_datetime_epoch_zero() {
    let ts = NanoTimestamp::new(0);
    let dt = ts.to_datetime();
    assert_eq!(dt.timestamp(), 0);
    assert_eq!(dt.timestamp_subsec_nanos(), 0);
}

#[test]
fn test_nano_timestamp_ordering_is_correct() {
    let a = NanoTimestamp::new(100);
    let b = NanoTimestamp::new(200);
    assert!(a < b);
    assert!(b > a);
    assert_eq!(a, NanoTimestamp::new(100));
}

// ── Symbol ───────────────────────────────────────────────────────────────────

#[test]
fn test_symbol_display_matches_inner() {
    let sym = Symbol::new("ETH").unwrap();
    assert_eq!(sym.to_string(), "ETH");
    assert_eq!(sym.as_str(), "ETH");
}

#[test]
fn test_symbol_hash_is_stable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(Symbol::new("BTC").unwrap());
    set.insert(Symbol::new("ETH").unwrap());
    set.insert(Symbol::new("BTC").unwrap()); // duplicate
    assert_eq!(set.len(), 2);
}

// ── TickFilter ───────────────────────────────────────────────────────────────

#[test]
fn test_tick_filter_default_matches_all() {
    let f = TickFilter::default();
    let t = Tick::new(
        Symbol::new("X").unwrap(),
        Price::new(dec!(1)).unwrap(),
        Quantity::zero(),
        Side::Bid,
        NanoTimestamp::new(0),
    );
    assert!(f.matches(&t), "default TickFilter must match any tick");
}

#[test]
fn test_tick_filter_min_quantity_boundary() {
    let min = Quantity::new(dec!(5)).unwrap();
    let f = TickFilter::new().min_quantity(min);

    let exactly_min = Tick::new(
        Symbol::new("X").unwrap(),
        Price::new(dec!(1)).unwrap(),
        Quantity::new(dec!(5)).unwrap(),
        Side::Bid,
        NanoTimestamp::new(0),
    );
    let below_min = Tick::new(
        Symbol::new("X").unwrap(),
        Price::new(dec!(1)).unwrap(),
        Quantity::new(dec!(4.99)).unwrap(),
        Side::Bid,
        NanoTimestamp::new(0),
    );
    assert!(f.matches(&exactly_min), "exactly min_qty must match");
    assert!(!f.matches(&below_min), "below min_qty must not match");
}

// ── OhlcvSeries ──────────────────────────────────────────────────────────────

#[test]
fn test_ohlcv_series_last_returns_most_recent() {
    let mut s = OhlcvSeries::new();
    s.push(make_bar("100", "110", "90", "105")).unwrap();
    s.push(make_bar("105", "115", "95", "112")).unwrap();
    let last = s.last().unwrap();
    assert_eq!(last.open.value(), dec!(105));
    assert_eq!(last.close.value(), dec!(112));
}

#[test]
fn test_ohlcv_series_last_empty_returns_none() {
    let s = OhlcvSeries::new();
    assert!(s.last().is_none());
}

#[test]
fn test_ohlcv_series_get_in_bounds() {
    let mut s = OhlcvSeries::new();
    s.push(make_bar("100", "110", "90", "105")).unwrap();
    s.push(make_bar("105", "115", "95", "112")).unwrap();
    assert!(s.get(0).is_some());
    assert!(s.get(1).is_some());
    assert!(s.get(2).is_none());
}

#[test]
fn test_ohlcv_series_volumes() {
    let mut s = OhlcvSeries::default();
    let mut b1 = make_bar("100", "110", "90", "105");
    b1.volume = Quantity::new(dec!(10)).unwrap();
    let mut b2 = make_bar("105", "115", "95", "112");
    b2.volume = Quantity::new(dec!(20)).unwrap();
    s.push(b1).unwrap();
    s.push(b2).unwrap();
    assert_eq!(s.volumes(), vec![dec!(10), dec!(20)]);
}

// ── Timeframe::Days ──────────────────────────────────────────────────────────

#[test]
fn test_timeframe_days_to_nanos() {
    let tf = Timeframe::Days(1);
    assert_eq!(tf.to_nanos().unwrap(), 86_400_000_000_000_i64);
}

#[test]
fn test_timeframe_days_bucket_start() {
    let tf = Timeframe::Days(1);
    let nanos_per_day = 86_400_000_000_000_i64;
    let ts = NanoTimestamp::new(nanos_per_day + 3_600_000_000_000_i64); // 1h into day 2
    let bucket = tf.bucket_start(ts).unwrap();
    assert_eq!(bucket.nanos(), nanos_per_day);
}

// ── DrawdownTracker edge case ─────────────────────────────────────────────────

#[test]
fn test_drawdown_tracker_zero_peak_returns_zero() {
    let t = DrawdownTracker::new(dec!(0));
    // peak is 0: formula returns 0 (avoids division by zero)
    assert_eq!(t.current_drawdown_pct(), dec!(0));
}

// ── PositionLedger ────────────────────────────────────────────────────────────

#[test]
fn test_position_ledger_realized_pnl_total_starts_at_zero() {
    let ledger = PositionLedger::new(dec!(10000));
    assert_eq!(ledger.realized_pnl_total(), dec!(0));
}

#[test]
fn test_position_ledger_position_none_for_unknown_symbol() {
    let ledger = PositionLedger::new(dec!(10000));
    let sym = Symbol::new("UNKNOWN").unwrap();
    assert!(ledger.position(&sym).is_none());
}

// ── RiskMonitor with no rules ─────────────────────────────────────────────────

#[test]
fn test_risk_monitor_no_rules_returns_empty_vec() {
    let mut monitor = RiskMonitor::new(dec!(10000));
    let breaches = monitor.update(dec!(5000));
    assert!(
        breaches.is_empty(),
        "a RiskMonitor with no rules must always return an empty breach vec"
    );
}

// ── Tick notional edge cases ──────────────────────────────────────────────────

#[test]
fn test_tick_notional_fractional_price_and_qty() {
    let t = Tick::new(
        Symbol::new("BTC").unwrap(),
        Price::new(dec!(65000.50)).unwrap(),
        Quantity::new(dec!(0.001)).unwrap(),
        Side::Ask,
        NanoTimestamp::new(0),
    );
    // 65000.50 * 0.001 = 65.0005
    assert_eq!(t.notional(), dec!(65.0005));
}
