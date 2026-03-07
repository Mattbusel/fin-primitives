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
    prices.iter().map(|p| signal.update(&bar(p)).unwrap()).collect()
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
    assert!(v1 < v2 && v2 < v3, "EMA should increase: {} {} {}", v1, v2, v3);
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
        "44.34", "44.09", "44.15", "43.61", "44.83", "45.10",
        "45.15", "43.61", "44.33", "44.83", "45.10", "43.15",
        "42.90", "43.00", "44.00", "43.50", "44.50",
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
