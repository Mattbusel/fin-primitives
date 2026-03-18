// Integration tests: OHLCV aggregation scenarios and bar series operations.

use fin_primitives::ohlcv::{OhlcvAggregator, OhlcvBar, OhlcvSeries, Timeframe};
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

fn tick(sym_s: &str, p: &str, q: &str, ts: i64) -> Tick {
    Tick::new(sym(sym_s), price(p), qty(q), Side::Ask, NanoTimestamp(ts))
}

fn make_bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
    OhlcvBar {
        symbol: sym("X"),
        open: price(o),
        high: price(h),
        low: price(l),
        close: price(c),
        volume: qty("100"),
        ts_open: NanoTimestamp(0),
        ts_close: NanoTimestamp(1),
        tick_count: 1,
    }
}

// ── Timeframe ─────────────────────────────────────────────────────────────

#[test]
fn timeframe_seconds_1_nanos() {
    assert_eq!(Timeframe::Seconds(1).to_nanos().unwrap(), 1_000_000_000);
}

#[test]
fn timeframe_minutes_5_nanos() {
    assert_eq!(Timeframe::Minutes(5).to_nanos().unwrap(), 300_000_000_000);
}

#[test]
fn timeframe_hours_1_nanos() {
    assert_eq!(Timeframe::Hours(1).to_nanos().unwrap(), 3_600_000_000_000);
}

#[test]
fn timeframe_days_1_nanos() {
    assert_eq!(Timeframe::Days(1).to_nanos().unwrap(), 86_400_000_000_000);
}

#[test]
fn timeframe_minutes_0_is_invalid() {
    assert!(Timeframe::Minutes(0).to_nanos().is_err());
}

#[test]
fn timeframe_hours_0_is_invalid() {
    assert!(Timeframe::Hours(0).to_nanos().is_err());
}

#[test]
fn timeframe_days_0_is_invalid() {
    assert!(Timeframe::Days(0).to_nanos().is_err());
}

#[test]
fn timeframe_bucket_start_aligns_correctly() {
    let tf = Timeframe::Minutes(1);
    let nanos_per_min = 60_000_000_000_i64;
    // Timestamp exactly at minute 2
    let ts = NanoTimestamp(2 * nanos_per_min + 7_000_000);
    assert_eq!(tf.bucket_start(ts).unwrap().0, 2 * nanos_per_min);
}

#[test]
fn timeframe_bucket_start_zero_ts() {
    let tf = Timeframe::Seconds(30);
    let ts = NanoTimestamp(0);
    assert_eq!(tf.bucket_start(ts).unwrap().0, 0);
}

// ── OhlcvBar invariants ───────────────────────────────────────────────────

#[test]
fn bar_validate_high_less_than_low_fails() {
    let mut b = make_bar("100", "110", "90", "105");
    b.high = price("80");
    assert!(b.validate().is_err());
}

#[test]
fn bar_validate_low_greater_than_close_fails() {
    let b = make_bar("100", "110", "95", "92");
    assert!(b.validate().is_err());
}

#[test]
fn bar_typical_price_calculation() {
    let b = make_bar("100", "120", "80", "110");
    // (120 + 80 + 110) / 3 = 103.333...
    let tp = b.typical_price();
    let expected = (dec!(120) + dec!(80) + dec!(110)) / dec!(3);
    assert_eq!(tp, expected);
}

#[test]
fn bar_range_zero_when_doji() {
    let b = make_bar("100", "100", "100", "100");
    assert_eq!(b.range(), dec!(0));
}

#[test]
fn bar_is_bullish_when_close_equals_open() {
    let b = make_bar("100", "110", "90", "100");
    assert!(b.is_bullish(), "close == open should be bullish");
}

#[test]
fn bar_is_bearish_when_close_below_open() {
    let b = make_bar("100", "110", "90", "99");
    assert!(!b.is_bullish());
}

// ── OhlcvAggregator: multi-bar sequence ──────────────────────────────────

#[test]
fn aggregator_three_consecutive_bars() {
    let nps = 60_000_000_000_i64; // nanos per 60-second bar
    let mut agg = OhlcvAggregator::new(sym("ETH"), Timeframe::Seconds(60)).unwrap();

    // Bar 0: ticks at 0s and 30s
    let r1 = agg.push_tick(&tick("ETH", "1000", "1", 0)).unwrap();
    assert!(r1.is_none());
    let r2 = agg.push_tick(&tick("ETH", "1010", "2", nps / 2)).unwrap();
    assert!(r2.is_none());

    // Bar 1 starts: triggers completion of bar 0
    let r3 = agg.push_tick(&tick("ETH", "1005", "1", nps + 1)).unwrap();
    let bar0 = r3.unwrap();
    assert_eq!(bar0.tick_count, 2);
    assert_eq!(bar0.open.value(), dec!(1000));
    assert_eq!(bar0.high.value(), dec!(1010));
    assert_eq!(bar0.close.value(), dec!(1010));

    // Another tick in bar 1
    agg.push_tick(&tick("ETH", "1020", "3", nps + nps / 3))
        .unwrap();

    // Bar 2 starts: triggers bar 1
    let r5 = agg
        .push_tick(&tick("ETH", "1000", "1", 2 * nps + 1))
        .unwrap();
    let bar1 = r5.unwrap();
    assert_eq!(bar1.tick_count, 2);
    assert_eq!(bar1.high.value(), dec!(1020));

    // Flush bar 2
    let bar2 = agg.flush().unwrap();
    assert_eq!(bar2.tick_count, 1);
    assert_eq!(bar2.open.value(), dec!(1000));
}

#[test]
fn aggregator_high_low_tracked_correctly() {
    let nps = 60_000_000_000_i64;
    let mut agg = OhlcvAggregator::new(sym("BTC"), Timeframe::Seconds(60)).unwrap();

    for (p, ts) in &[
        ("500", 0i64),
        ("450", nps / 4),
        ("600", nps / 2),
        ("550", 3 * nps / 4),
    ] {
        agg.push_tick(&tick("BTC", p, "1", *ts)).unwrap();
    }

    let bar = agg.flush().unwrap();
    assert_eq!(bar.open.value(), dec!(500));
    assert_eq!(bar.high.value(), dec!(600));
    assert_eq!(bar.low.value(), dec!(450));
    assert_eq!(bar.close.value(), dec!(550));
    assert_eq!(bar.tick_count, 4);
}

#[test]
fn aggregator_wrong_symbol_ignored() {
    let mut agg = OhlcvAggregator::new(sym("AAPL"), Timeframe::Seconds(60)).unwrap();
    agg.push_tick(&tick("AAPL", "100", "1", 0)).unwrap();
    let r = agg.push_tick(&tick("MSFT", "200", "1", 1)).unwrap();
    assert!(r.is_none());
    assert_eq!(agg.current_bar().unwrap().tick_count, 1);
}

#[test]
fn aggregator_volume_accumulates() {
    let nps = 60_000_000_000_i64;
    let mut agg = OhlcvAggregator::new(sym("X"), Timeframe::Seconds(60)).unwrap();
    agg.push_tick(&tick("X", "100", "5", 0)).unwrap();
    agg.push_tick(&tick("X", "101", "3", nps / 2)).unwrap();
    let bar = agg.flush().unwrap();
    assert_eq!(bar.volume.value(), dec!(8));
}

#[test]
fn aggregator_single_tick_per_bar() {
    let nps = 60_000_000_000_i64;
    let mut agg = OhlcvAggregator::new(sym("X"), Timeframe::Seconds(60)).unwrap();
    agg.push_tick(&tick("X", "100", "1", 0)).unwrap();
    let r = agg.push_tick(&tick("X", "200", "1", nps + 1)).unwrap();
    let bar = r.unwrap();
    // OHLCV all equal to single tick price
    assert_eq!(bar.open, bar.high);
    assert_eq!(bar.open, bar.low);
    assert_eq!(bar.open, bar.close);
}

// ── OhlcvSeries ───────────────────────────────────────────────────────────

#[test]
fn series_push_and_len() {
    let mut series = OhlcvSeries::new();
    for i in 1..=5u32 {
        let p = format!("{}", 100 + i);
        let h = format!("{}", 110 + i);
        let l = format!("{}", 90 + i);
        let c = format!("{}", 105 + i);
        series.push(make_bar(&p, &h, &l, &c)).unwrap();
    }
    assert_eq!(series.len(), 5);
}

#[test]
fn series_get_by_index() {
    let mut series = OhlcvSeries::new();
    series.push(make_bar("100", "110", "90", "105")).unwrap();
    series.push(make_bar("105", "115", "95", "110")).unwrap();
    let bar = series.get(1).unwrap();
    assert_eq!(bar.open.value(), dec!(105));
}

#[test]
fn series_get_out_of_bounds_returns_none() {
    let series = OhlcvSeries::new();
    assert!(series.get(0).is_none());
}

#[test]
fn series_last_reflects_most_recent() {
    let mut series = OhlcvSeries::new();
    series.push(make_bar("100", "110", "90", "105")).unwrap();
    series.push(make_bar("200", "220", "180", "210")).unwrap();
    assert_eq!(series.last().unwrap().open.value(), dec!(200));
}

#[test]
fn series_window_returns_all_when_small() {
    let mut series = OhlcvSeries::new();
    series.push(make_bar("100", "110", "90", "105")).unwrap();
    assert_eq!(series.window(100).len(), 1);
}

#[test]
fn series_volumes_correct() {
    let mut series = OhlcvSeries::new();
    let mut b1 = make_bar("100", "110", "90", "105");
    b1.volume = qty("50");
    let mut b2 = make_bar("110", "120", "100", "115");
    b2.volume = qty("75");
    series.push(b1).unwrap();
    series.push(b2).unwrap();
    let vols = series.volumes();
    assert_eq!(vols, vec![dec!(50), dec!(75)]);
}

#[test]
fn series_closes_matches_close_prices() {
    let mut series = OhlcvSeries::new();
    series.push(make_bar("100", "110", "90", "105")).unwrap();
    series.push(make_bar("105", "115", "95", "112")).unwrap();
    series.push(make_bar("112", "120", "108", "117")).unwrap();
    let closes = series.closes();
    assert_eq!(closes, vec![dec!(105), dec!(112), dec!(117)]);
}

#[test]
fn series_invalid_bar_rejected() {
    let mut series = OhlcvSeries::new();
    let bad = make_bar("100", "90", "80", "85"); // high < open
    assert!(series.push(bad).is_err());
    assert!(series.is_empty());
}

// ── Aggregator + Series integration ──────────────────────────────────────

#[test]
fn aggregator_feeds_series_correctly() {
    let nps = 60_000_000_000_i64;
    let mut agg = OhlcvAggregator::new(sym("SPY"), Timeframe::Seconds(60)).unwrap();
    let mut series = OhlcvSeries::new();

    let ticks = [
        ("SPY", "400", "1", 0i64),
        ("SPY", "401", "2", nps / 2),
        ("SPY", "402", "1", nps + 1),
        ("SPY", "403", "2", nps + nps / 2),
        ("SPY", "404", "1", 2 * nps + 1),
    ];

    for (s, p, q, ts) in &ticks {
        if let Some(bar) = agg.push_tick(&tick(s, p, q, *ts)).unwrap() {
            series.push(bar).unwrap();
        }
    }
    if let Some(bar) = agg.flush() {
        series.push(bar).unwrap();
    }

    assert_eq!(series.len(), 3);
    // First bar: opens at 400, closes at 401
    assert_eq!(series.get(0).unwrap().open.value(), dec!(400));
    assert_eq!(series.get(0).unwrap().close.value(), dec!(401));
}

#[test]
fn aggregator_5min_bar_test() {
    let nps = 300_000_000_000_i64; // 5 minutes
    let mut agg = OhlcvAggregator::new(sym("NVDA"), Timeframe::Minutes(5)).unwrap();

    // Fill first bar with 10 ticks
    for i in 0..10i64 {
        agg.push_tick(&tick("NVDA", "800", "1", i * (nps / 10)))
            .unwrap();
    }

    // Trigger completion
    let r = agg.push_tick(&tick("NVDA", "810", "1", nps + 1)).unwrap();
    let bar = r.unwrap();
    assert_eq!(bar.tick_count, 10);
}

#[test]
fn aggregator_flush_empty_returns_none() {
    let mut agg = OhlcvAggregator::new(sym("X"), Timeframe::Seconds(60)).unwrap();
    assert!(agg.flush().is_none());
    assert!(agg.current_bar().is_none());
}
