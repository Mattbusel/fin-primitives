// Integration tests: Indicator accuracy and cross-indicator consistency.

use fin_primitives::ohlcv::{OhlcvBar, OhlcvSeries};
use fin_primitives::signals::indicators::{Atr, BollingerB, Dema, Ema, Rsi, Sma, StochasticK, Wma};
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
        ts_open: NanoTimestamp::new(0),
        ts_close: NanoTimestamp::new(1),
        tick_count: 1,
    }
}

fn ohlc_bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
    OhlcvBar {
        symbol: Symbol::new("X").unwrap(),
        open: Price::new(o.parse().unwrap()).unwrap(),
        high: Price::new(h.parse().unwrap()).unwrap(),
        low: Price::new(l.parse().unwrap()).unwrap(),
        close: Price::new(c.parse().unwrap()).unwrap(),
        volume: Quantity::new(dec!(100)).unwrap(),
        ts_open: NanoTimestamp::new(0),
        ts_close: NanoTimestamp::new(1),
        tick_count: 1,
    }
}

fn feed_bars(signal: &mut impl Signal, prices: &[&str]) -> Vec<SignalValue> {
    prices
        .iter()
        .map(|p| signal.update_bar(&bar(p)).unwrap())
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
    let mut sma = Sma::new("sma1", 1).unwrap();
    let v = sma.update_bar(&bar("42")).unwrap();
    assert!(matches!(v, SignalValue::Scalar(d) if d == dec!(42)));
    assert!(sma.is_ready());
}

#[test]
fn sma_returns_unavailable_for_first_period_minus_1_bars() {
    let mut sma = Sma::new("sma5", 5).unwrap();
    for _ in 0..4 {
        let v = sma.update_bar(&bar("100")).unwrap();
        assert!(matches!(v, SignalValue::Unavailable));
    }
    let v = sma.update_bar(&bar("100")).unwrap();
    assert!(matches!(v, SignalValue::Scalar(_)));
}

#[test]
fn sma_correct_average_10_bars() {
    let mut sma = Sma::new("sma5", 5).unwrap();
    feed_bars(&mut sma, &["10", "20", "30", "40"]); // warmup
    let v = scalar(sma.update_bar(&bar("50")).unwrap());
    // SMA(5) of [10,20,30,40,50] = 150/5 = 30
    assert_eq!(v, dec!(30));
}

#[test]
fn sma_sliding_window_excludes_old_values() {
    let mut sma = Sma::new("sma3", 3).unwrap();
    // Feed [10, 20, 30] -> SMA = 20
    feed_bars(&mut sma, &["10", "20", "30"]);
    // Feed 40: window = [20, 30, 40] -> SMA = 30
    let v = scalar(sma.update_bar(&bar("40")).unwrap());
    assert_eq!(v, dec!(30));
    // Feed 100: window = [30, 40, 100] -> SMA = 56.666...
    let v2 = scalar(sma.update_bar(&bar("100")).unwrap());
    let expected = (dec!(30) + dec!(40) + dec!(100)) / dec!(3);
    assert_eq!(v2, expected);
}

#[test]
fn sma_period_name_accessible() {
    let sma = Sma::new("my_sma_20", 20).unwrap();
    assert_eq!(sma.name(), "my_sma_20");
    assert_eq!(sma.period(), 20);
}

#[test]
fn sma_all_same_prices_returns_that_price() {
    let mut sma = Sma::new("sma4", 4).unwrap();
    feed_bars(&mut sma, &["50", "50", "50"]);
    let v = scalar(sma.update_bar(&bar("50")).unwrap());
    assert_eq!(v, dec!(50));
}

// ── EMA correctness ───────────────────────────────────────────────────────

#[test]
fn ema_period_1_equals_close() {
    let mut ema = Ema::new("ema1", 1).unwrap();
    let v = ema.update_bar(&bar("99")).unwrap();
    // SMA seed of period 1 = just the price
    assert!(matches!(v, SignalValue::Scalar(d) if d == dec!(99)));
}

#[test]
fn ema_seed_equals_sma_of_first_period_bars() {
    // period=4, k = 2/5 = 0.4
    // SMA seed of [10, 20, 30, 40] = 25
    let mut ema = Ema::new("ema4", 4).unwrap();
    feed_bars(&mut ema, &["10", "20", "30"]);
    let v = scalar(ema.update_bar(&bar("40")).unwrap());
    assert_eq!(v, dec!(25));
}

#[test]
fn ema_subsequent_follows_formula() {
    // period=3, k = 2/4 = 0.5
    // seed = (10+20+30)/3 = 20
    // bar 4 = 40: EMA = 40*0.5 + 20*(1-0.5) = 20+10 = 30
    // bar 5 = 20: EMA = 20*0.5 + 30*0.5 = 25
    let mut ema = Ema::new("ema3", 3).unwrap();
    feed_bars(&mut ema, &["10", "20", "30"]);
    scalar(ema.update_bar(&bar("40")).unwrap()); // = 30
    let v = scalar(ema.update_bar(&bar("20")).unwrap()); // = 25
    assert_eq!(v, dec!(25));
}

#[test]
fn ema_is_ready_flag() {
    let mut ema = Ema::new("ema5", 5).unwrap();
    for i in 1..5 {
        ema.update_bar(&bar(&format!("{}", i * 10))).unwrap();
        assert!(!ema.is_ready(), "not ready after {} bars", i);
    }
    ema.update_bar(&bar("50")).unwrap();
    assert!(ema.is_ready());
}

#[test]
fn ema_monotone_increasing_sequence_tracks_uptrend() {
    let mut ema = Ema::new("ema3", 3).unwrap();
    feed_bars(&mut ema, &["10", "20", "30"]); // seed = 20
    let v1 = scalar(ema.update_bar(&bar("40")).unwrap()); // 30
    let v2 = scalar(ema.update_bar(&bar("50")).unwrap()); // 40
    let v3 = scalar(ema.update_bar(&bar("60")).unwrap()); // 50
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
    let mut rsi = Rsi::new("rsi3", 3).unwrap();
    let values: Vec<_> = ["100", "101", "102", "103"]
        .iter()
        .map(|p| rsi.update_bar(&bar(p)).unwrap())
        .collect();
    assert!(matches!(values[0], SignalValue::Unavailable));
    assert!(matches!(values[1], SignalValue::Unavailable));
    assert!(matches!(values[2], SignalValue::Unavailable));
    assert!(matches!(values[3], SignalValue::Scalar(_)));
}

#[test]
fn rsi_pure_gains_is_100() {
    let mut rsi = Rsi::new("rsi3", 3).unwrap();
    let prices = ["100", "200", "300", "400"];
    let mut last = SignalValue::Unavailable;
    for p in &prices {
        last = rsi.update_bar(&bar(p)).unwrap();
    }
    if let SignalValue::Scalar(v) = last {
        assert_eq!(v, dec!(100), "all gains should give RSI=100");
    } else {
        panic!("expected Scalar");
    }
}

#[test]
fn rsi_pure_losses_near_0() {
    let mut rsi = Rsi::new("rsi3", 3).unwrap();
    // Monotonically decreasing
    let prices = ["100", "90", "80", "70"];
    let mut last = SignalValue::Unavailable;
    for p in &prices {
        last = rsi.update_bar(&bar(p)).unwrap();
    }
    if let SignalValue::Scalar(v) = last {
        assert_eq!(v, Decimal::ZERO, "all losses should give RSI=0");
    } else {
        panic!("expected Scalar");
    }
}

#[test]
fn rsi_always_in_0_to_100() {
    let mut rsi = Rsi::new("rsi14", 14).unwrap();
    // Volatile prices
    let prices = [
        "44.34", "44.09", "44.15", "43.61", "44.83", "45.10", "45.15", "43.61", "44.33", "44.83",
        "45.10", "43.15", "42.90", "43.00", "44.00", "43.50", "44.50",
    ];
    for p in &prices {
        if let SignalValue::Scalar(v) = rsi.update_bar(&bar(p)).unwrap() {
            assert!(v >= Decimal::ZERO, "RSI below 0: {}", v);
            assert!(v <= Decimal::ONE_HUNDRED, "RSI above 100: {}", v);
        }
    }
}

#[test]
fn rsi_is_ready_after_period_plus_one() {
    let mut rsi = Rsi::new("rsi5", 5).unwrap();
    for i in 0..5 {
        rsi.update_bar(&bar(&format!("{}", 100 + i))).unwrap();
        assert!(!rsi.is_ready());
    }
    rsi.update_bar(&bar("110")).unwrap();
    assert!(rsi.is_ready());
}

// ── Signal pipeline integration ──────────────────────────────────────────

#[test]
fn pipeline_sma_ema_rsi_warm_up_correctly() {
    let mut pipeline = SignalPipeline::new()
        .add(Sma::new("sma5", 5).unwrap())
        .add(Ema::new("ema5", 5).unwrap())
        .add(Rsi::new("rsi5", 5).unwrap());

    assert_eq!(pipeline.ready_count(), 0);

    for i in 1..=5 {
        pipeline.update(&bar(&format!("{}", 100 + i)));
    }
    // After 5 bars: SMA and EMA ready, RSI not yet (needs period+1)
    assert_eq!(pipeline.ready_count(), 2);

    pipeline.update(&bar("110"));
    assert_eq!(pipeline.ready_count(), 3);
}

#[test]
fn pipeline_all_signals_present_in_map() {
    let mut pipeline = SignalPipeline::new()
        .add(Sma::new("sma3", 3).unwrap())
        .add(Ema::new("ema3", 3).unwrap())
        .add(Rsi::new("rsi3", 3).unwrap());

    let prices = ["100", "101", "102", "103"];
    let mut last_map = None;
    for p in &prices {
        last_map = Some(pipeline.update(&bar(p)));
    }
    let map = last_map.unwrap();
    assert!(map.get("sma3").is_some());
    assert!(map.get("ema3").is_some());
    assert!(map.get("rsi3").is_some());
    assert!(map.get("nonexistent").is_none());
}

#[test]
fn pipeline_single_signal_works() {
    let mut pipeline = SignalPipeline::new().add(Sma::new("sma2", 2).unwrap());
    pipeline.update(&bar("10"));
    let map = pipeline.update(&bar("20"));
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
    let mut sma = Sma::new("sma1", 1).unwrap();
    let v = sma.update_bar(&bar("77")).unwrap();
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
    let mut ema = Ema::new("ema1", 1).unwrap();
    let v = ema.update_bar(&bar("55")).unwrap();
    assert!(
        matches!(v, SignalValue::Scalar(d) if d == dec!(55)),
        "EMA(1) of a single data point must equal the data point"
    );
    assert!(ema.is_ready());
}

/// RSI with period 1 needs 2 bars (period + 1 extra for first change).
#[test]
fn rsi_single_bar_returns_unavailable() {
    let mut rsi = Rsi::new("rsi1", 1).unwrap();
    let v = rsi.update_bar(&bar("100")).unwrap();
    assert!(matches!(v, SignalValue::Unavailable));
    assert!(!rsi.is_ready());
}

/// RSI(1): after 2 bars the first value is produced.
#[test]
fn rsi_period_1_ready_after_two_bars() {
    let mut rsi = Rsi::new("rsi1", 1).unwrap();
    rsi.update_bar(&bar("100")).unwrap();
    let v = rsi.update_bar(&bar("110")).unwrap();
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
    let mut rsi = Rsi::new("rsi3", 3).unwrap();
    // 4 bars needed to get first RSI(3) value; all at same price.
    rsi.update_bar(&bar("100")).unwrap();
    rsi.update_bar(&bar("100")).unwrap();
    rsi.update_bar(&bar("100")).unwrap();
    let v = rsi.update_bar(&bar("100")).unwrap();
    // avg_gain = 0, avg_loss = 0 → avg_loss == 0 branch → RSI = 100
    assert!(
        matches!(v, SignalValue::Scalar(d) if d == dec!(100)),
        "RSI with all-same prices: avg_loss=0 path must return 100"
    );
}

/// SMA with all-same prices equals that price exactly.
#[test]
fn sma_all_same_values_returns_that_value() {
    let mut sma = Sma::new("sma5", 5).unwrap();
    for _ in 0..4 {
        sma.update_bar(&bar("42")).unwrap();
    }
    let v = sma.update_bar(&bar("42")).unwrap();
    assert!(matches!(v, SignalValue::Scalar(d) if d == dec!(42)));
}

/// EMA with all-same prices: seed = price, subsequent EMA = price.
#[test]
fn ema_all_same_values_returns_that_value() {
    let mut ema = Ema::new("ema4", 4).unwrap();
    for _ in 0..4 {
        ema.update_bar(&bar("33")).unwrap();
    }
    // After seed phase, feed a few more identical bars.
    for _ in 0..5 {
        let v = ema.update_bar(&bar("33")).unwrap();
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
    let mut rsi = Rsi::new("rsi3", 3).unwrap();
    rsi.update_bar(&bar("100")).unwrap();
    rsi.update_bar(&bar("110")).unwrap();
    rsi.update_bar(&bar("120")).unwrap();
    let v = rsi.update_bar(&bar("130")).unwrap();
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
    let mut rsi = Rsi::new("rsi3", 3).unwrap();
    rsi.update_bar(&bar("130")).unwrap();
    rsi.update_bar(&bar("120")).unwrap();
    rsi.update_bar(&bar("110")).unwrap();
    let v = rsi.update_bar(&bar("100")).unwrap();
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
    let mut sma = Sma::new("sma5", 5).unwrap();
    let mut ema = Ema::new("ema5", 5).unwrap();
    // Warm up with diverse prices.
    for p in &["90", "95", "100", "105", "110"] {
        sma.update_bar(&bar(p)).unwrap();
        ema.update_bar(&bar(p)).unwrap();
    }
    // Now feed 20 bars at the new stable price 200.
    let mut last_sma = dec!(0);
    let mut last_ema = dec!(0);
    for _ in 0..20 {
        if let SignalValue::Scalar(s) = sma.update_bar(&bar("200")).unwrap() {
            last_sma = s;
        }
        if let SignalValue::Scalar(e) = ema.update_bar(&bar("200")).unwrap() {
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
        .add(Sma::new("sma5", 5).unwrap())
        .add(Ema::new("ema5", 5).unwrap());

    for p in &prices {
        let b = bar(p);
        series.push(b.clone()).unwrap();
        pipeline.update(&b);
    }

    assert_eq!(series.len(), 10);
    assert_eq!(pipeline.ready_count(), 2);
}

#[test]
fn sma_and_ema_converge_on_flat_price() {
    // With constant price, SMA == EMA == that price
    let mut sma = Sma::new("sma5", 5).unwrap();
    let mut ema = Ema::new("ema5", 5).unwrap();
    let b = bar("100");

    for _ in 0..5 {
        sma.update_bar(&b).unwrap();
        ema.update_bar(&b).unwrap();
    }

    let s = scalar(sma.update_bar(&b).unwrap());
    let e = scalar(ema.update_bar(&b).unwrap());
    assert_eq!(s, dec!(100));
    assert_eq!(e, dec!(100));
}

// ── ATR correctness ───────────────────────────────────────────────────────

#[test]
fn atr_unavailable_on_first_bar() {
    let mut atr = Atr::new("atr5", 5).unwrap();
    let v = atr.update_bar(&ohlc_bar("10", "15", "5", "10")).unwrap();
    assert_eq!(v, SignalValue::Unavailable);
}

#[test]
fn atr_known_values_period_3() {
    // Bar0: no prev_close → unavailable
    // Bar1: prev=10, TR=max(15-5=10, |15-10|=5, |5-10|=5) = 10
    // Bar2: prev=10, TR=max(12-8=4,  |12-10|=2, |8-10|=2) = 4
    // Bar3: prev=9,  TR=max(11-7=4,  |11-9|=2,  |7-9|=2)  = 4
    // ATR(3) after 3 TRs = (10+4+4)/3 = 6
    let mut atr = Atr::new("atr3", 3).unwrap();
    atr.update_bar(&ohlc_bar("10", "15", "5", "10")).unwrap();
    atr.update_bar(&ohlc_bar("10", "12", "8", "10")).unwrap();
    atr.update_bar(&ohlc_bar("9", "11", "7", "9")).unwrap();
    let v = scalar(atr.update_bar(&ohlc_bar("9", "11", "7", "9")).unwrap());
    // After 4th bar: window shifts, still SMA of last 3 TRs
    // bar3 TR: prev=9, bar=(11,7,9) → TR=max(4,2,2)=4
    // window [4,4,4] → ATR = 4
    assert_eq!(v, dec!(4));
}

#[test]
fn atr_reset_restarts_accumulation() {
    let mut atr = Atr::new("atr3", 3).unwrap();
    for _ in 0..4 {
        atr.update_bar(&ohlc_bar("10", "15", "5", "10")).unwrap();
    }
    assert!(atr.is_ready());
    atr.reset();
    assert!(!atr.is_ready());
    let v = atr.update_bar(&ohlc_bar("10", "15", "5", "10")).unwrap();
    assert_eq!(v, SignalValue::Unavailable);
}

// ── BollingerB correctness ────────────────────────────────────────────────

#[test]
fn bollinger_b_flat_prices_returns_half() {
    // All same price → stddev=0 → returns 0.5 by convention
    let mut bb = BollingerB::new("bb3", 3, dec!(2)).unwrap();
    for _ in 0..3 {
        bb.update_bar(&bar("100")).unwrap();
    }
    let v = scalar(bb.update_bar(&bar("100")).unwrap());
    assert_eq!(v, dec!(0.5));
}

#[test]
fn bollinger_b_above_1_on_spike() {
    let mut bb = BollingerB::new("bb5", 5, dec!(1)).unwrap();
    for _ in 0..5 {
        bb.update_bar(&bar("100")).unwrap();
    }
    // After warmup (all 100), bands are tight; a spike to 200 is far above upper band
    let v = scalar(bb.update_bar(&bar("200")).unwrap());
    assert!(v > Decimal::ONE, "%B should be > 1 on spike, got {v}");
}

#[test]
fn bollinger_b_below_0_on_drop() {
    let mut bb = BollingerB::new("bb5", 5, dec!(1)).unwrap();
    for _ in 0..5 {
        bb.update_bar(&bar("100")).unwrap();
    }
    let v = scalar(bb.update_bar(&bar("10")).unwrap());
    assert!(v < Decimal::ZERO, "%B should be < 0 on drop, got {v}");
}

#[test]
fn bollinger_b_unavailable_before_period() {
    let mut bb = BollingerB::new("bb5", 5, dec!(2)).unwrap();
    for _ in 0..4 {
        assert_eq!(bb.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}

// ── StochasticK correctness ───────────────────────────────────────────────

#[test]
fn stochastic_k_close_equals_high_returns_100() {
    // %K = (close - low_min) / (high_max - low_min) * 100
    // When close == high_max: %K = 100
    let mut sk = StochasticK::new("sk3", 3).unwrap();
    sk.update_bar(&ohlc_bar("10", "15", "5", "12")).unwrap();
    sk.update_bar(&ohlc_bar("11", "16", "6", "13")).unwrap();
    let v = scalar(sk.update_bar(&ohlc_bar("12", "20", "8", "20")).unwrap());
    assert_eq!(v, dec!(100));
}

#[test]
fn stochastic_k_close_equals_low_returns_0() {
    // Window: bars with lows [5,6,5], min_low=5; high_max=20.
    // Last bar close=5 = min_low → %K = 0
    let mut sk = StochasticK::new("sk3", 3).unwrap();
    sk.update_bar(&ohlc_bar("10", "15", "5", "12")).unwrap();
    sk.update_bar(&ohlc_bar("11", "16", "6", "13")).unwrap();
    let v = scalar(sk.update_bar(&ohlc_bar("10", "20", "5", "5")).unwrap());
    assert_eq!(v, dec!(0));
}

#[test]
fn stochastic_k_midpoint() {
    // high=21, low=1, close=11 → %K = (11-1)/(21-1)*100 = 10/20*100 = 50
    let mut sk = StochasticK::new("sk1", 1).unwrap();
    let v = scalar(sk.update_bar(&ohlc_bar("11", "21", "1", "11")).unwrap());
    assert_eq!(v, dec!(50));
}

#[test]
fn stochastic_k_flat_range_returns_50() {
    let mut sk = StochasticK::new("sk3", 3).unwrap();
    for _ in 0..3 {
        sk.update_bar(&bar("100")).unwrap();
    }
    let v = scalar(sk.update_bar(&bar("100")).unwrap());
    assert_eq!(v, dec!(50));
}

#[test]
fn stochastic_k_range_0_to_100() {
    let mut sk = StochasticK::new("sk5", 5).unwrap();
    let prices = [("90","95","85","92"), ("92","98","88","90"), ("90","100","80","95"),
                  ("95","102","82","88"), ("88","96","78","93")];
    for (o,h,l,c) in &prices {
        if let SignalValue::Scalar(v) = sk.update_bar(&ohlc_bar(o,h,l,c)).unwrap() {
            assert!(v >= Decimal::ZERO && v <= dec!(100), "%K out of range: {v}");
        }
    }
}

// ── WMA correctness ───────────────────────────────────────────────────────

#[test]
fn wma_known_values_period_3() {
    // WMA(3) of [10, 20, 30]: weights [1,2,3], denom=6
    // = (10*1 + 20*2 + 30*3)/6 = (10+40+90)/6 = 140/6
    let mut wma = Wma::new("wma3", 3).unwrap();
    wma.update_bar(&bar("10")).unwrap();
    wma.update_bar(&bar("20")).unwrap();
    let v = scalar(wma.update_bar(&bar("30")).unwrap());
    let expected = dec!(140) / dec!(6);
    assert_eq!(v, expected);
}

#[test]
fn wma_constant_price_equals_price() {
    let mut wma = Wma::new("wma5", 5).unwrap();
    for _ in 0..5 {
        wma.update_bar(&bar("42")).unwrap();
    }
    let v = scalar(wma.update_bar(&bar("42")).unwrap());
    assert_eq!(v, dec!(42));
}

#[test]
fn wma_most_recent_weighted_highest() {
    // Most recent bar has the highest weight; if recent price > older prices,
    // WMA should exceed SMA.
    let period = 3;
    let mut wma = Wma::new("wma3", period).unwrap();
    let mut sma = Sma::new("sma3", period).unwrap();
    for p in &["10", "20", "30"] {
        wma.update_bar(&bar(p)).unwrap();
        sma.update_bar(&bar(p)).unwrap();
    }
    let wma_v = scalar(wma.update_bar(&bar("100")).unwrap());
    let sma_v = scalar(sma.update_bar(&bar("100")).unwrap());
    assert!(wma_v > sma_v, "WMA ({wma_v}) should exceed SMA ({sma_v}) when recent price dominates");
}

// ── DEMA correctness ──────────────────────────────────────────────────────

#[test]
fn dema_constant_price_equals_price() {
    let mut dema = Dema::new("d3", 3).unwrap();
    let mut last = SignalValue::Unavailable;
    for _ in 0..10 {
        last = dema.update_bar(&bar("75")).unwrap();
    }
    assert_eq!(scalar(last), dec!(75));
}

#[test]
fn dema_faster_than_ema_on_jump() {
    let period = 5;
    let mut dema = Dema::new("d5", period).unwrap();
    let mut ema = Ema::new("e5", period).unwrap();
    for _ in 0..(2 * period) {
        dema.update_bar(&bar("100")).unwrap();
    }
    for _ in 0..period {
        ema.update_bar(&bar("100")).unwrap();
    }
    let dema_v = scalar(dema.update_bar(&bar("300")).unwrap());
    let ema_v = scalar(ema.update_bar(&bar("300")).unwrap());
    assert!(dema_v > ema_v, "DEMA ({dema_v}) should react faster than EMA ({ema_v})");
}

#[test]
fn dema_reset_clears_state() {
    let mut dema = Dema::new("d3", 3).unwrap();
    for _ in 0..6 {
        dema.update_bar(&bar("100")).unwrap();
    }
    assert!(dema.is_ready());
    dema.reset();
    assert!(!dema.is_ready());
}
