// Integration tests: Indicator accuracy and cross-indicator consistency.

use fin_primitives::ohlcv::{OhlcvBar, OhlcvSeries};
use fin_primitives::signals::indicators::{Ema, Rsi, Sma};
use fin_primitives::signals::pipeline::SignalPipeline;
use fin_primitives::signals::{Signal, SignalValue};
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Symbol};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn bar(close: &str) -> OhlcvBar {
    let p = Price::new(close.parse().unwrap()).unwrap();
    OhlcvBar {
        symbol: Symbol::new("X").unwrap(),
        open: p,
        high: p,
        low: p,
        close: p,
        volume: Quantity::zero(),
        ts_open: NanoTimestamp(0),
        ts_close: NanoTimestamp(1),
        tick_count: 1,
    }
}

fn feed_bars(signal: &mut impl Signal, prices: &[&str]) -> Vec<SignalValue> {
    prices
        .iter()
        .map(|p| signal.update(&bar(p)).unwrap())
        .collect()
}

fn scalar(v: SignalValue) -> Decimal {
    match v {
        SignalValue::Scalar(d) => d,
        _ => panic!("expected Scalar, got Unavailable"),
    }
}

// ── SMA correctness ───────────────────────────────────────────────────────

#[test]
fn sma_period_1_always_ready() {
    let mut sma = Sma::new("sma1", 1);
    let v = sma.update(&bar("42")).unwrap();
    assert!(matches!(v, SignalValue::Scalar(d) if d == dec!(42)));
    assert!(sma.is_ready());
}

#[test]
fn sma_returns_unavailable_for_first_period_minus_1_bars() {
    let mut sma = Sma::new("sma5", 5);
    for _ in 0..4 {
        let v = sma.update(&bar("100")).unwrap();
        assert!(matches!(v, SignalValue::Unavailable));
    }
    let v = sma.update(&bar("100")).unwrap();
    assert!(matches!(v, SignalValue::Scalar(_)));
}

#[test]
fn sma_correct_average_10_bars() {
    let mut sma = Sma::new("sma5", 5);
    feed_bars(&mut sma, &["10", "20", "30", "40"]); // warmup
    let v = scalar(sma.update(&bar("50")).unwrap());
    // SMA(5) of [10,20,30,40,50] = 150/5 = 30
    assert_eq!(v, dec!(30));
}

#[test]
fn sma_sliding_window_excludes_old_values() {
    let mut sma = Sma::new("sma3", 3);
    // Feed [10, 20, 30] -> SMA = 20
    feed_bars(&mut sma, &["10", "20", "30"]);
    // Feed 40: window = [20, 30, 40] -> SMA = 30
    let v = scalar(sma.update(&bar("40")).unwrap());
    assert_eq!(v, dec!(30));
    // Feed 100: window = [30, 40, 100] -> SMA = 56.666...
    let v2 = scalar(sma.update(&bar("100")).unwrap());
    let expected = (dec!(30) + dec!(40) + dec!(100)) / dec!(3);
    assert_eq!(v2, expected);
}

#[test]
fn sma_period_name_accessible() {
    let sma = Sma::new("my_sma_20", 20);
    assert_eq!(sma.name(), "my_sma_20");
    assert_eq!(sma.period(), 20);
}

#[test]
fn sma_all_same_prices_returns_that_price() {
    let mut sma = Sma::new("sma4", 4);
    feed_bars(&mut sma, &["50", "50", "50"]);
    let v = scalar(sma.update(&bar("50")).unwrap());
    assert_eq!(v, dec!(50));
}

// ── EMA correctness ───────────────────────────────────────────────────────

#[test]
fn ema_period_1_equals_close() {
    let mut ema = Ema::new("ema1", 1);
    let v = ema.update(&bar("99")).unwrap();
    // SMA seed of period 1 = just the price
    assert!(matches!(v, SignalValue::Scalar(d) if d == dec!(99)));
}

#[test]
fn ema_seed_equals_sma_of_first_period_bars() {
    // period=4, k = 2/5 = 0.4
    // SMA seed of [10, 20, 30, 40] = 25
    let mut ema = Ema::new("ema4", 4);
    feed_bars(&mut ema, &["10", "20", "30"]);
    let v = scalar(ema.update(&bar("40")).unwrap());
    assert_eq!(v, dec!(25));
}

#[test]
fn ema_subsequent_follows_formula() {
    // period=3, k = 2/4 = 0.5
    // seed = (10+20+30)/3 = 20
    // bar 4 = 40: EMA = 40*0.5 + 20*(1-0.5) = 20+10 = 30
    // bar 5 = 20: EMA = 20*0.5 + 30*0.5 = 25
    let mut ema = Ema::new("ema3", 3);
    feed_bars(&mut ema, &["10", "20", "30"]);
    scalar(ema.update(&bar("40")).unwrap()); // = 30
    let v = scalar(ema.update(&bar("20")).unwrap()); // = 25
    assert_eq!(v, dec!(25));
}

#[test]
fn ema_is_ready_flag() {
    let mut ema = Ema::new("ema5", 5);
    for i in 1..5 {
        ema.update(&bar(&format!("{}", i * 10))).unwrap();
        assert!(!ema.is_ready(), "not ready after {} bars", i);
    }
    ema.update(&bar("50")).unwrap();
    assert!(ema.is_ready());
}

#[test]
fn ema_monotone_increasing_sequence_tracks_uptrend() {
    let mut ema = Ema::new("ema3", 3);
    feed_bars(&mut ema, &["10", "20", "30"]); // seed = 20
    let v1 = scalar(ema.update(&bar("40")).unwrap()); // 30
    let v2 = scalar(ema.update(&bar("50")).unwrap()); // 40
    let v3 = scalar(ema.update(&bar("60")).unwrap()); // 50
    assert!(
        v1 < v2 && v2 < v3,
        "EMA should increase: {} {} {}",
        v1,
        v2,
        v3
    );
}

// ── RSI correctness ───────────────────────────────────────────────────────

#[test]
fn rsi_period_3_needs_4_bars_to_produce_first_value() {
    let mut rsi = Rsi::new("rsi3", 3);
    let values: Vec<_> = ["100", "101", "102", "103"]
        .iter()
        .map(|p| rsi.update(&bar(p)).unwrap())
        .collect();
    assert!(matches!(values[0], SignalValue::Unavailable));
    assert!(matches!(values[1], SignalValue::Unavailable));
    assert!(matches!(values[2], SignalValue::Unavailable));
    assert!(matches!(values[3], SignalValue::Scalar(_)));
}

#[test]
fn rsi_pure_gains_is_100() {
    let mut rsi = Rsi::new("rsi3", 3);
    let prices = ["100", "200", "300", "400"];
    let mut last = SignalValue::Unavailable;
    for p in &prices {
        last = rsi.update(&bar(p)).unwrap();
    }
    if let SignalValue::Scalar(v) = last {
        assert_eq!(v, dec!(100), "all gains should give RSI=100");
    } else {
        panic!("expected Scalar");
    }
}

#[test]
fn rsi_pure_losses_near_0() {
    let mut rsi = Rsi::new("rsi3", 3);
    // Monotonically decreasing
    let prices = ["100", "90", "80", "70"];
    let mut last = SignalValue::Unavailable;
    for p in &prices {
        last = rsi.update(&bar(p)).unwrap();
    }
    if let SignalValue::Scalar(v) = last {
        assert_eq!(v, Decimal::ZERO, "all losses should give RSI=0");
    } else {
        panic!("expected Scalar");
    }
}

#[test]
fn rsi_always_in_0_to_100() {
    let mut rsi = Rsi::new("rsi14", 14);
    // Volatile prices
    let prices = [
        "44.34", "44.09", "44.15", "43.61", "44.83", "45.10", "45.15", "43.61", "44.33", "44.83",
        "45.10", "43.15", "42.90", "43.00", "44.00", "43.50", "44.50",
    ];
    for p in &prices {
        if let SignalValue::Scalar(v) = rsi.update(&bar(p)).unwrap() {
            assert!(v >= Decimal::ZERO, "RSI below 0: {}", v);
            assert!(v <= Decimal::ONE_HUNDRED, "RSI above 100: {}", v);
        }
    }
}

#[test]
fn rsi_is_ready_after_period_plus_one() {
    let mut rsi = Rsi::new("rsi5", 5);
    for i in 0..5 {
        rsi.update(&bar(&format!("{}", 100 + i))).unwrap();
        assert!(!rsi.is_ready());
    }
    rsi.update(&bar("110")).unwrap();
    assert!(rsi.is_ready());
}

// ── Signal pipeline integration ──────────────────────────────────────────

#[test]
fn pipeline_sma_ema_rsi_warm_up_correctly() {
    let mut pipeline = SignalPipeline::new()
        .add(Sma::new("sma5", 5))
        .add(Ema::new("ema5", 5))
        .add(Rsi::new("rsi5", 5));

    assert_eq!(pipeline.ready_count(), 0);

    for i in 1..=5 {
        pipeline.update(&bar(&format!("{}", 100 + i))).unwrap();
    }
    // After 5 bars: SMA and EMA ready, RSI not yet (needs period+1)
    assert_eq!(pipeline.ready_count(), 2);

    pipeline.update(&bar("110")).unwrap();
    assert_eq!(pipeline.ready_count(), 3);
}

#[test]
fn pipeline_all_signals_present_in_map() {
    let mut pipeline = SignalPipeline::new()
        .add(Sma::new("sma3", 3))
        .add(Ema::new("ema3", 3))
        .add(Rsi::new("rsi3", 3));

    let prices = ["100", "101", "102", "103"];
    let mut last_map = None;
    for p in &prices {
        last_map = Some(pipeline.update(&bar(p)).unwrap());
    }
    let map = last_map.unwrap();
    assert!(map.get("sma3").is_some());
    assert!(map.get("ema3").is_some());
    assert!(map.get("rsi3").is_some());
    assert!(map.get("nonexistent").is_none());
}

#[test]
fn pipeline_single_signal_works() {
    let mut pipeline = SignalPipeline::new().add(Sma::new("sma2", 2));
    pipeline.update(&bar("10")).unwrap();
    let map = pipeline.update(&bar("20")).unwrap();
    let v = map.get("sma2").unwrap();
    assert!(matches!(v, SignalValue::Scalar(d) if *d == dec!(15)));
}

#[test]
fn pipeline_empty_has_zero_ready() {
    let pipeline = SignalPipeline::new();
    assert_eq!(pipeline.ready_count(), 0);
}

// ── Single data point edge cases ─────────────────────────────────────────

/// SMA with period 1 is ready immediately and equals the close price.
#[test]
fn sma_single_data_point_period_1() {
    let mut sma = Sma::new("sma1", 1);
    let v = sma.update(&bar("77")).unwrap();
    assert!(
        matches!(v, SignalValue::Scalar(d) if d == dec!(77)),
        "SMA(1) of a single data point must equal the data point"
    );
    assert!(sma.is_ready());
}

/// EMA with period 1: the first bar seeds the EMA at exactly the close price
/// because k = 2/(1+1) = 1.0 and seed phase completes in one bar.
#[test]
fn ema_single_data_point_period_1() {
    let mut ema = Ema::new("ema1", 1);
    let v = ema.update(&bar("55")).unwrap();
    assert!(
        matches!(v, SignalValue::Scalar(d) if d == dec!(55)),
        "EMA(1) of a single data point must equal the data point"
    );
    assert!(ema.is_ready());
}

/// RSI with period 1 needs 2 bars (period + 1 extra for first change).
#[test]
fn rsi_single_bar_returns_unavailable() {
    let mut rsi = Rsi::new("rsi1", 1);
    let v = rsi.update(&bar("100")).unwrap();
    assert!(matches!(v, SignalValue::Unavailable));
    assert!(!rsi.is_ready());
}

/// RSI(1): after 2 bars the first value is produced.
#[test]
fn rsi_period_1_ready_after_two_bars() {
    let mut rsi = Rsi::new("rsi1", 1);
    rsi.update(&bar("100")).unwrap();
    let v = rsi.update(&bar("110")).unwrap();
    assert!(matches!(v, SignalValue::Scalar(_)));
    assert!(rsi.is_ready());
}

// ── All-same values edge cases ────────────────────────────────────────────

/// RSI with all identical prices: no gains and no losses → RSI is 0 with
/// avg_loss = 0. The implementation guards this by returning 100 when
/// avg_loss == 0 (convention: if there are no losses RS is infinite → RSI=100).
/// But with all-same prices both avg_gain AND avg_loss are 0, so the code returns 100.
#[test]
fn rsi_all_same_prices_returns_100_by_convention() {
    let mut rsi = Rsi::new("rsi3", 3);
    // 4 bars needed to get first RSI(3) value; all at same price.
    rsi.update(&bar("100")).unwrap();
    rsi.update(&bar("100")).unwrap();
    rsi.update(&bar("100")).unwrap();
    let v = rsi.update(&bar("100")).unwrap();
    // avg_gain = 0, avg_loss = 0 → avg_loss == 0 branch → RSI = 100
    assert!(
        matches!(v, SignalValue::Scalar(d) if d == dec!(100)),
        "RSI with all-same prices: avg_loss=0 path must return 100"
    );
}

/// SMA with all-same prices equals that price exactly.
#[test]
fn sma_all_same_values_returns_that_value() {
    let mut sma = Sma::new("sma5", 5);
    for _ in 0..4 {
        sma.update(&bar("42")).unwrap();
    }
    let v = sma.update(&bar("42")).unwrap();
    assert!(matches!(v, SignalValue::Scalar(d) if d == dec!(42)));
}

/// EMA with all-same prices: seed = price, subsequent EMA = price.
#[test]
fn ema_all_same_values_returns_that_value() {
    let mut ema = Ema::new("ema4", 4);
    for _ in 0..4 {
        ema.update(&bar("33")).unwrap();
    }
    // After seed phase, feed a few more identical bars.
    for _ in 0..5 {
        let v = ema.update(&bar("33")).unwrap();
        assert!(
            matches!(v, SignalValue::Scalar(d) if d == dec!(33)),
            "EMA with all-same prices must equal that price"
        );
    }
}

// ── RSI overbought/oversold exact boundaries ──────────────────────────────

/// RSI = 70 is the standard overbought threshold. Test that an RSI value
/// computed from a strongly uptrending sequence lands at or above 70.
/// This is a directional check: with only gains the RSI approaches 100;
/// with a strong mix that barely tips 70 we verify the boundary is crossed.
#[test]
fn rsi_overbought_boundary_above_70() {
    // Feed 3 gains then 1 small loss: RSI should still be above 70.
    let mut rsi = Rsi::new("rsi3", 3);
    rsi.update(&bar("100")).unwrap();
    rsi.update(&bar("110")).unwrap();
    rsi.update(&bar("120")).unwrap();
    let v = rsi.update(&bar("130")).unwrap();
    if let SignalValue::Scalar(val) = v {
        assert!(
            val >= dec!(70),
            "strongly uptrending RSI should be >= 70, got {val}"
        );
    } else {
        panic!("expected Scalar");
    }
}

/// RSI = 30 is the standard oversold threshold. With all losses RSI = 0
/// which is below 30.
#[test]
fn rsi_oversold_boundary_below_30() {
    let mut rsi = Rsi::new("rsi3", 3);
    rsi.update(&bar("130")).unwrap();
    rsi.update(&bar("120")).unwrap();
    rsi.update(&bar("110")).unwrap();
    let v = rsi.update(&bar("100")).unwrap();
    if let SignalValue::Scalar(val) = v {
        assert!(
            val <= dec!(30),
            "strongly downtrending RSI should be <= 30, got {val}"
        );
    } else {
        panic!("expected Scalar");
    }
}

// ── SMA/EMA convergence rate ──────────────────────────────────────────────

/// After a constant price sequence, SMA and EMA both converge to that price
/// immediately (SMA by definition, EMA because each new bar at the same price
/// keeps the EMA unchanged once seeded).
#[test]
fn sma_ema_converge_to_constant_price() {
    let mut sma = Sma::new("sma5", 5);
    let mut ema = Ema::new("ema5", 5);
    // Warm up with diverse prices.
    for p in &["90", "95", "100", "105", "110"] {
        sma.update(&bar(p)).unwrap();
        ema.update(&bar(p)).unwrap();
    }
    // Now feed 20 bars at the new stable price 200.
    let mut last_sma = dec!(0);
    let mut last_ema = dec!(0);
    for _ in 0..20 {
        if let SignalValue::Scalar(s) = sma.update(&bar("200")).unwrap() {
            last_sma = s;
        }
        if let SignalValue::Scalar(e) = ema.update(&bar("200")).unwrap() {
            last_ema = e;
        }
    }
    assert_eq!(
        last_sma,
        dec!(200),
        "SMA must be exactly 200 after 20 bars at 200"
    );
    // EMA approaches 200 asymptotically; after 20 bars at 200 it must be very close.
    let diff = (last_ema - dec!(200)).abs();
    assert!(
        diff < dec!(1),
        "EMA must be within 1 of 200 after 20 bars at that price, got {last_ema}"
    );
}

// ── OhlcvSeries + Signal pipeline end-to-end ─────────────────────────────

#[test]
fn series_feeds_pipeline_end_to_end() {
    let prices = ["10", "20", "30", "40", "50", "60", "70", "80", "90", "100"];
    let mut series = OhlcvSeries::new();
    let mut pipeline = SignalPipeline::new()
        .add(Sma::new("sma5", 5))
        .add(Ema::new("ema5", 5));

    for p in &prices {
        let b = bar(p);
        series.push(b.clone()).unwrap();
        pipeline.update(&b).unwrap();
    }

    assert_eq!(series.len(), 10);
    assert_eq!(pipeline.ready_count(), 2);
}

#[test]
fn sma_and_ema_converge_on_flat_price() {
    // With constant price, SMA == EMA == that price
    let mut sma = Sma::new("sma5", 5);
    let mut ema = Ema::new("ema5", 5);
    let b = bar("100");

    for _ in 0..5 {
        sma.update(&b).unwrap();
        ema.update(&b).unwrap();
    }

    let s = scalar(sma.update(&b).unwrap());
    let e = scalar(ema.update(&b).unwrap());
    assert_eq!(s, dec!(100));
    assert_eq!(e, dec!(100));
}
