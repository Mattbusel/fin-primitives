//! `ta_compat`: the `ta` crate's API shape, checked against TA-Lib's output for the
//! same 120 bars (tests/talib_reference), plus the trait and constructor surface that
//! code written for `ta` relies on.
#![cfg(feature = "ta-compat")]
#![allow(clippy::unwrap_used)]

#[path = "talib_reference/mod.rs"]
mod talib;

use fin_primitives::error::FinError;
use fin_primitives::ta_compat::indicators::*;
use fin_primitives::ta_compat::{
    Close, DataItem, DataItemBuilder, High, Low, Next, Open, Period, Reset, TaError, Volume,
};
use talib::*;

fn items() -> Vec<DataItem> {
    (0..N)
        .map(|i| {
            DataItem::builder()
                .open(OPEN[i])
                .high(HIGH[i])
                .low(LOW[i])
                .close(CLOSE[i])
                .volume(VOLUME[i])
                .build()
                .unwrap()
        })
        .collect()
}

#[track_caller]
fn assert_series(what: &str, actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len(), "{what}: length");
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        if e.is_nan() {
            assert!(a.is_nan(), "{what}[{i}]: expected warm-up NaN, got {a}");
        } else {
            assert!(
                (a - e).abs() <= 1e-9 * e.abs().max(1.0),
                "{what}[{i}]: got {a}, expected {e}"
            );
        }
    }
}

/// Runs a close-based indicator through both `Next<f64>` and `Next<&DataItem>` and
/// checks the two agree.
fn on_closes<I>(make: impl Fn() -> I) -> Vec<f64>
where
    I: Next<f64, Output = f64> + for<'a> Next<&'a DataItem, Output = f64>,
{
    let mut by_value = make();
    let mut by_bar = make();
    let a: Vec<f64> = CLOSE.iter().map(|&c| by_value.next(c)).collect();
    let b: Vec<f64> = items().iter().map(|bar| by_bar.next(bar)).collect();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    a
}

fn on_bars<I>(mut ind: I) -> Vec<f64>
where
    I: for<'a> Next<&'a DataItem, Output = f64>,
{
    items().iter().map(|bar| ind.next(bar)).collect()
}

// ── Values against TA-Lib ────────────────────────────────────────────────────

#[test]
fn sma_ema_rsi_match_talib() {
    assert_series("SMA(10)", &on_closes(|| SimpleMovingAverage::new(10).unwrap()), &SMA_10);
    assert_series("EMA(10)", &on_closes(|| ExponentialMovingAverage::new(10).unwrap()), &EMA_10);
    assert_series("RSI(14)", &on_closes(|| RelativeStrengthIndex::new(14).unwrap()), &RSI_14);
}

#[test]
fn stddev_mad_roc_er_match_reference() {
    assert_series("SD(20)", &on_closes(|| StandardDeviation::new(20).unwrap()), &STDDEV_20);
    assert_series("MAD(20)", &on_closes(|| MeanAbsoluteDeviation::new(20).unwrap()), &MAD_20);
    assert_series("ROC(10)", &on_closes(|| RateOfChange::new(10).unwrap()), &ROC_10);
    assert_series("ER(10)", &on_closes(|| EfficiencyRatio::new(10).unwrap()), &ER_10);
}

#[test]
fn maximum_minimum_match_talib() {
    assert_series("MAX(high, 14)", &on_bars(Maximum::new(14).unwrap()), &MAX_HIGH_14);
    assert_series("MIN(low, 14)", &on_bars(Minimum::new(14).unwrap()), &MIN_LOW_14);
    let mut mx = Maximum::new(14).unwrap();
    let mut mn = Minimum::new(14).unwrap();
    let mxs: Vec<f64> = CLOSE.iter().map(|&c| mx.next(c)).collect();
    let mns: Vec<f64> = CLOSE.iter().map(|&c| mn.next(c)).collect();
    assert_series("MAX(close, 14)", &mxs, &MAX_CLOSE_14);
    assert_series("MIN(close, 14)", &mns, &MIN_CLOSE_14);
}

#[test]
fn atr_and_true_range_match_talib() {
    assert_series("ATR(14)", &on_bars(AverageTrueRange::new(14).unwrap()), &ATR_14);
    let tr = on_bars(TrueRange::new());
    assert_eq!(tr[0], HIGH[0] - LOW[0]);
    assert_series("TRANGE", &tr[1..], &TRANGE[1..]);
    // f64 input: absolute change, 0 on the first value (as in `ta`).
    let mut t = TrueRange::new();
    assert_eq!(t.next(10.0), 0.0);
    assert_eq!(t.next(12.5), 2.5);
    assert_eq!(t.next(11.0), 1.5);
}

#[test]
fn stochastics_match_talib() {
    assert_series("STOCHF %K(14)", &on_bars(FastStochastic::new(14).unwrap()), &STOCHF_K_14);
    assert_series("STOCH slow %K(14, EMA 3)", &on_bars(SlowStochastic::new(14, 3).unwrap()), &STOCH_SLOWK_EMA_14_3);
}

#[test]
fn obv_mfi_cci_match_talib() {
    assert_series("OBV", &on_bars(OnBalanceVolume::new()), &OBV);
    assert_series("MFI(14)", &on_bars(MoneyFlowIndex::new(14).unwrap()), &MFI_14);
    assert_series("CCI(20)", &on_bars(CommodityChannelIndex::new(20).unwrap()), &CCI_20);
}

#[test]
fn macd_matches_talib() {
    let mut macd = MovingAverageConvergenceDivergence::new(12, 26, 9).unwrap();
    let out: Vec<MovingAverageConvergenceDivergenceOutput> =
        items().iter().map(|b| macd.next(b)).collect();
    let line: Vec<f64> = out.iter().map(|o| o.macd).collect();
    // TA-Lib reports the line only once the signal is ready; ours starts at bar 26.
    assert!(line[..25].iter().all(|v| v.is_nan()));
    assert!(line[25..].iter().all(|v| !v.is_nan()));
    assert_series("MACD line", &line[33..], &MACD_LINE[33..]);
    let signal: Vec<f64> = out.iter().map(|o| o.signal).collect();
    let hist: Vec<f64> = out.iter().map(|o| o.histogram).collect();
    assert_series("MACD signal", &signal, &MACD_SIGNAL);
    assert_series("MACD histogram", &hist, &MACD_HIST);
    assert!(macd.is_ready());
}

#[test]
fn ppo_matches_talib() {
    let mut ppo = PercentagePriceOscillator::new(12, 26, 9).unwrap();
    let out: Vec<PercentagePriceOscillatorOutput> = CLOSE.iter().map(|&c| ppo.next(c)).collect();
    let line: Vec<f64> = out.iter().map(|o| o.ppo).collect();
    assert_series("PPO(12, 26)", &line, &PPO_EMA_12_26);
    // Signal: SMA-seeded EMA(9) of TA-Lib's PPO line.
    let k = 2.0 / 10.0;
    let mut expected = vec![f64::NAN; N];
    let first = 25;
    let mut sig = PPO_EMA_12_26[first..first + 9].iter().sum::<f64>() / 9.0;
    expected[first + 8] = sig;
    for i in first + 9..N {
        sig += (PPO_EMA_12_26[i] - sig) * k;
        expected[i] = sig;
    }
    let signal: Vec<f64> = out.iter().map(|o| o.signal).collect();
    assert_series("PPO signal", &signal, &expected);
    for o in &out[33..] {
        assert!((o.ppo - o.signal - o.histogram).abs() < 1e-12);
    }
}

#[test]
fn bollinger_matches_talib() {
    let mut bb = BollingerBands::new(20, 2.0).unwrap();
    let out: Vec<BollingerBandsOutput> = CLOSE.iter().map(|&c| bb.next(c)).collect();
    assert_series("BB upper", &out.iter().map(|o| o.upper).collect::<Vec<_>>(), &BB_UPPER);
    assert_series("BB average", &out.iter().map(|o| o.average).collect::<Vec<_>>(), &BB_MIDDLE);
    assert_series("BB lower", &out.iter().map(|o| o.lower).collect::<Vec<_>>(), &BB_LOWER);
    assert_eq!(bb.multiplier(), 2.0);
}

#[test]
fn keltner_is_talib_ema_of_typical_price_plus_wilder_atr() {
    let mut kc = KeltnerChannel::new(20, 2.0).unwrap();
    let out: Vec<KeltnerChannelOutput> = items().iter().map(|b| kc.next(b)).collect();
    let upper: Vec<f64> = (0..N).map(|i| KC_MIDDLE_20[i] + 2.0 * ATR_20[i]).collect();
    let lower: Vec<f64> = (0..N).map(|i| KC_MIDDLE_20[i] - 2.0 * ATR_20[i]).collect();
    assert_series("KC average", &out.iter().map(|o| o.average).collect::<Vec<_>>(), &KC_MIDDLE_20);
    assert_series("KC upper", &out.iter().map(|o| o.upper).collect::<Vec<_>>(), &upper);
    assert_series("KC lower", &out.iter().map(|o| o.lower).collect::<Vec<_>>(), &lower);
    assert!(kc.is_ready());
}

#[test]
fn chandelier_exit_uses_wilder_atr() {
    let mut ce = ChandelierExit::new(22, 3.0).unwrap();
    let out: Vec<ChandelierExitOutput> = items().iter().map(|b| ce.next(b)).collect();
    let long: Vec<f64> = (0..N).map(|i| MAX_HIGH_22[i] - 3.0 * ATR_22[i]).collect();
    let short: Vec<f64> = (0..N).map(|i| MIN_LOW_22[i] + 3.0 * ATR_22[i]).collect();
    assert_series("CE long", &out.iter().map(|o| o.long).collect::<Vec<_>>(), &long);
    assert_series("CE short", &out.iter().map(|o| o.short).collect::<Vec<_>>(), &short);
    let (l, s): (f64, f64) = out[N - 1].clone().into();
    assert_eq!((l, s), (out[N - 1].long, out[N - 1].short));
}

// ── API surface ──────────────────────────────────────────────────────────────

#[test]
fn constructors_reject_bad_parameters_with_invalid_parameter() {
    assert_eq!(SimpleMovingAverage::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(ExponentialMovingAverage::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(StandardDeviation::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(MeanAbsoluteDeviation::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(RelativeStrengthIndex::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(Maximum::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(Minimum::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(FastStochastic::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(SlowStochastic::new(14, 0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(AverageTrueRange::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(MovingAverageConvergenceDivergence::new(12, 26, 0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(PercentagePriceOscillator::new(0, 26, 9).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(CommodityChannelIndex::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(EfficiencyRatio::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(BollingerBands::new(0, 2.0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(BollingerBands::new(20, f64::NAN).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(KeltnerChannel::new(0, 2.0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(ChandelierExit::new(0, 3.0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(RateOfChange::new(0).unwrap_err(), TaError::InvalidParameter);
    assert_eq!(MoneyFlowIndex::new(0).unwrap_err(), TaError::InvalidParameter);
}

#[test]
fn defaults_and_display_match_ta() {
    assert_eq!(SimpleMovingAverage::default().to_string(), "SMA(9)");
    assert_eq!(ExponentialMovingAverage::default().to_string(), "EMA(9)");
    assert_eq!(StandardDeviation::default().to_string(), "SD(9)");
    assert_eq!(MeanAbsoluteDeviation::default().to_string(), "MAD(9)");
    assert_eq!(RelativeStrengthIndex::default().to_string(), "RSI(14)");
    assert_eq!(Maximum::default().to_string(), "MAX(14)");
    assert_eq!(Minimum::default().to_string(), "MIN(14)");
    assert_eq!(FastStochastic::default().to_string(), "FAST_STOCH(14)");
    assert_eq!(SlowStochastic::default().to_string(), "SLOW_STOCH(14, 3)");
    assert_eq!(TrueRange::default().to_string(), "TRUE_RANGE()");
    assert_eq!(AverageTrueRange::default().to_string(), "ATR(14)");
    assert_eq!(MovingAverageConvergenceDivergence::default().to_string(), "MACD(12, 26, 9)");
    assert_eq!(PercentagePriceOscillator::default().to_string(), "PPO(12, 26, 9)");
    assert_eq!(CommodityChannelIndex::default().to_string(), "CCI(20)");
    assert_eq!(EfficiencyRatio::default().to_string(), "ER(14)");
    assert_eq!(BollingerBands::default().to_string(), "BB(9, 2)");
    assert_eq!(KeltnerChannel::default().to_string(), "KC(10, 2)");
    assert_eq!(ChandelierExit::default().to_string(), "CE(22, 3)");
    assert_eq!(RateOfChange::default().to_string(), "ROC(9)");
    assert_eq!(MoneyFlowIndex::default().to_string(), "MFI(14)");
    assert_eq!(OnBalanceVolume::default().to_string(), "OBV");
}

#[test]
fn period_trait_reports_constructor_periods() {
    assert_eq!(SimpleMovingAverage::new(7).unwrap().period(), 7);
    assert_eq!(ExponentialMovingAverage::new(7).unwrap().period(), 7);
    assert_eq!(StandardDeviation::new(7).unwrap().period(), 7);
    assert_eq!(MeanAbsoluteDeviation::new(7).unwrap().period(), 7);
    assert_eq!(RelativeStrengthIndex::new(7).unwrap().period(), 7);
    assert_eq!(Maximum::new(7).unwrap().period(), 7);
    assert_eq!(Minimum::new(7).unwrap().period(), 7);
    assert_eq!(FastStochastic::new(7).unwrap().period(), 7);
    assert_eq!(AverageTrueRange::new(7).unwrap().period(), 7);
    assert_eq!(CommodityChannelIndex::new(7).unwrap().period(), 7);
    assert_eq!(EfficiencyRatio::new(7).unwrap().period(), 7);
    assert_eq!(BollingerBands::new(7, 1.5).unwrap().period(), 7);
    assert_eq!(KeltnerChannel::new(7, 1.5).unwrap().period(), 7);
    assert_eq!(ChandelierExit::new(7, 1.5).unwrap().period(), 7);
    assert_eq!(RateOfChange::new(7).unwrap().period(), 7);
    assert_eq!(MoneyFlowIndex::new(7).unwrap().period(), 7);
}

/// Code written against `ta` is often generic over `Next + Reset`.
fn run_twice<I: Next<f64, Output = f64> + Reset>(ind: &mut I, xs: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let a = xs.iter().map(|&x| ind.next(x)).collect();
    ind.reset();
    let b = xs.iter().map(|&x| ind.next(x)).collect();
    (a, b)
}

#[test]
fn reset_restores_the_initial_state() {
    fn check<I: Next<f64, Output = f64> + Reset>(mut ind: I) {
        let (a, b) = run_twice(&mut ind, &CLOSE);
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }
    check(SimpleMovingAverage::new(5).unwrap());
    check(ExponentialMovingAverage::new(5).unwrap());
    check(StandardDeviation::new(5).unwrap());
    check(MeanAbsoluteDeviation::new(5).unwrap());
    check(RelativeStrengthIndex::new(5).unwrap());
    check(Maximum::new(5).unwrap());
    check(Minimum::new(5).unwrap());
    check(FastStochastic::new(5).unwrap());
    check(SlowStochastic::new(5, 3).unwrap());
    check(TrueRange::new());
    check(AverageTrueRange::new(5).unwrap());
    check(EfficiencyRatio::new(5).unwrap());
    check(RateOfChange::new(5).unwrap());

    let bars = items();
    let mut obv = OnBalanceVolume::new();
    let a: Vec<f64> = bars.iter().map(|b| obv.next(b)).collect();
    obv.reset();
    let b: Vec<f64> = bars.iter().map(|b| obv.next(b)).collect();
    assert_eq!(a, b);

    let mut mfi = MoneyFlowIndex::new(5).unwrap();
    let a: Vec<f64> = bars.iter().map(|b| mfi.next(b)).collect();
    mfi.reset();
    let b: Vec<f64> = bars.iter().map(|b| mfi.next(b)).collect();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));

    let mut kc = KeltnerChannel::new(5, 2.0).unwrap();
    let a: Vec<_> = bars.iter().map(|b| kc.next(b)).collect();
    kc.reset();
    let b: Vec<_> = bars.iter().map(|b| kc.next(b)).collect();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));

    let mut macd = MovingAverageConvergenceDivergence::new(3, 6, 4).unwrap();
    let a: Vec<_> = CLOSE.iter().map(|&c| macd.next(c)).collect();
    macd.reset();
    let b: Vec<_> = CLOSE.iter().map(|&c| macd.next(c)).collect();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

#[test]
fn warm_up_is_nan_and_is_ready_flips_with_the_first_value() {
    let mut sma = SimpleMovingAverage::new(3).unwrap();
    assert!(sma.next(1.0).is_nan());
    assert!(!sma.is_ready());
    assert!(sma.next(2.0).is_nan());
    assert_eq!(sma.next(3.0), 2.0);
    assert!(sma.is_ready());

    let mut bb = BollingerBands::new(3, 2.0).unwrap();
    let o = bb.next(1.0);
    assert!(o.average.is_nan() && o.upper.is_nan() && o.lower.is_nan());
}

#[test]
fn macd_output_converts_to_tuple_and_inverted_periods_swap() {
    let mut a = MovingAverageConvergenceDivergence::new(26, 12, 9).unwrap();
    let mut b = MovingAverageConvergenceDivergence::new(12, 26, 9).unwrap();
    assert_eq!(a.to_string(), "MACD(12, 26, 9)");
    for &c in &CLOSE {
        let (m1, s1, h1): (f64, f64, f64) = a.next(c).into();
        let (m2, s2, h2): (f64, f64, f64) = b.next(c).into();
        assert_eq!(format!("{:?}", (m1, s1, h1)), format!("{:?}", (m2, s2, h2)));
    }
    let mut p = PercentagePriceOscillator::new(12, 26, 9).unwrap();
    let mut last = None;
    for &c in &CLOSE {
        last = Some(p.next(c));
    }
    let (ppo, sig, hist): (f64, f64, f64) = last.unwrap().into();
    assert!((ppo - sig - hist).abs() < 1e-12);
    assert!(p.is_ready());
}

#[test]
fn flat_input_edge_values() {
    let mut fs = FastStochastic::new(3).unwrap();
    let mut er = EfficiencyRatio::new(2).unwrap();
    let mut roc = RateOfChange::new(1).unwrap();
    let mut rsi = RelativeStrengthIndex::new(2).unwrap();
    for _ in 0..4 {
        fs.next(5.0);
        er.next(5.0);
        rsi.next(5.0);
    }
    assert_eq!(fs.next(5.0), 50.0);
    assert_eq!(er.next(5.0), 0.0);
    assert_eq!(rsi.next(5.0), 100.0);
    roc.next(0.0);
    assert_eq!(roc.next(3.0), 0.0);
}

#[test]
fn data_item_builder_validates_like_ta() {
    let ok = DataItemBuilder::new().open(2.0).high(3.0).low(1.0).close(2.5).volume(10.0).build().unwrap();
    assert_eq!((ok.open(), ok.high(), ok.low(), ok.close(), ok.volume()), (2.0, 3.0, 1.0, 2.5, 10.0));
    let missing = DataItem::builder().open(2.0).high(3.0).low(1.0).close(2.5).build();
    assert_eq!(missing.unwrap_err(), TaError::DataItemIncomplete);
    let cases = [
        (2.0, 3.0, 2.5, 2.4, 1.0),  // close below low
        (3.5, 3.0, 1.0, 2.0, 1.0),  // open above high
        (2.0, 3.0, 1.0, 2.0, -1.0), // negative volume
        (-1.0, 3.0, -2.0, 2.0, 1.0), // negative low
        (2.0, 3.0, 1.0, f64::NAN, 1.0),
    ];
    for (o, h, l, c, v) in cases {
        let r = DataItem::builder().open(o).high(h).low(l).close(c).volume(v).build();
        assert_eq!(r.unwrap_err(), TaError::DataItemInvalid, "{o} {h} {l} {c} {v}");
    }
}

#[test]
fn errors_display_and_convert() {
    assert_eq!(TaError::InvalidParameter.to_string(), "invalid parameter");
    assert_eq!(TaError::DataItemIncomplete.to_string(), "data item is incomplete");
    assert_eq!(TaError::DataItemInvalid.to_string(), "data item is invalid");
    let e: FinError = TaError::InvalidParameter.into();
    assert!(matches!(e, FinError::InvalidInput(ref m) if m == "invalid parameter"));
    let boxed: Box<dyn std::error::Error> = Box::new(TaError::DataItemInvalid);
    assert_eq!(boxed.to_string(), "data item is invalid");
}

#[test]
fn fin_primitives_bars_are_inputs() {
    use fin_primitives::signals::BarInput;
    use rust_decimal_macros::dec;
    let bar = BarInput::new(dec!(10.5), dec!(11), dec!(9.5), dec!(10), dec!(1000));
    assert_eq!((bar.open(), bar.high(), bar.low(), bar.close(), bar.volume()), (10.0, 11.0, 9.5, 10.5, 1000.0));
    let mut atr = AverageTrueRange::new(1).unwrap();
    assert!(atr.next(&bar).is_nan());
    assert_eq!(atr.next(&bar), 1.5);

    use fin_primitives::ohlcv::OhlcvBar;
    use fin_primitives::types::{NanoTimestamp, Price, Quantity, Symbol};
    let ob = OhlcvBar {
        symbol: Symbol::new("X").unwrap(),
        open: Price::new(dec!(10)).unwrap(),
        high: Price::new(dec!(11)).unwrap(),
        low: Price::new(dec!(9.5)).unwrap(),
        close: Price::new(dec!(10.5)).unwrap(),
        volume: Quantity::new(dec!(1000)).unwrap(),
        ts_open: NanoTimestamp::new(0),
        ts_close: NanoTimestamp::new(1),
        tick_count: 1,
    };
    assert_eq!((ob.open(), ob.high(), ob.low(), ob.close(), ob.volume()), (10.0, 11.0, 9.5, 10.5, 1000.0));
    let mut obv = OnBalanceVolume::new();
    assert_eq!(obv.next(&ob), 1000.0);
}

#[test]
fn indicators_are_clone_and_debug() {
    let mut a = RelativeStrengthIndex::new(3).unwrap();
    for &c in &CLOSE[..10] {
        a.next(c);
    }
    let mut b = a.clone();
    assert_eq!(a.next(CLOSE[10]), b.next(CLOSE[10]));
    assert!(format!("{a:?}").contains("RelativeStrengthIndex"));
}

// ── Cross-check against the real `ta` crate where the docs say values agree ──

#[cfg(feature = "ta")]
mod against_ta {
    use super::*;

    fn ta_items() -> Vec<ta::DataItem> {
        (0..N)
            .map(|i| {
                ta::DataItem::builder()
                    .open(OPEN[i])
                    .high(HIGH[i])
                    .low(LOW[i])
                    .close(CLOSE[i])
                    .volume(VOLUME[i])
                    .build()
                    .unwrap()
            })
            .collect()
    }

    /// Ours (on `ta::DataItem` bars) against `ta`'s, once both are warmed up.
    fn agree_after(
        what: &str,
        warm_up: usize,
        mut ours: impl FnMut(&ta::DataItem) -> f64,
        mut theirs: impl FnMut(&ta::DataItem) -> f64,
    ) {
        for (i, bar) in ta_items().iter().enumerate() {
            let (a, b) = (ours(bar), theirs(bar));
            if i >= warm_up {
                assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{what}[{i}]: {a} vs ta {b}");
            }
        }
    }

    #[test]
    fn unchanged_indicators_agree_with_ta_after_warm_up() {
        use ta::Next as _;
        let mut o = SimpleMovingAverage::new(10).unwrap();
        let mut t = ta::indicators::SimpleMovingAverage::new(10).unwrap();
        agree_after("SMA", 9, |b| o.next(b), |b| t.next(b));
        let mut o = StandardDeviation::new(20).unwrap();
        let mut t = ta::indicators::StandardDeviation::new(20).unwrap();
        agree_after("SD", 19, |b| o.next(b), |b| t.next(b));
        let mut o = MeanAbsoluteDeviation::new(20).unwrap();
        let mut t = ta::indicators::MeanAbsoluteDeviation::new(20).unwrap();
        agree_after("MAD", 19, |b| o.next(b), |b| t.next(b));
        let mut o = Maximum::new(14).unwrap();
        let mut t = ta::indicators::Maximum::new(14).unwrap();
        agree_after("MAX", 13, |b| o.next(b), |b| t.next(b));
        let mut o = Minimum::new(14).unwrap();
        let mut t = ta::indicators::Minimum::new(14).unwrap();
        agree_after("MIN", 13, |b| o.next(b), |b| t.next(b));
        let mut o = FastStochastic::new(14).unwrap();
        let mut t = ta::indicators::FastStochastic::new(14).unwrap();
        agree_after("FAST_STOCH", 13, |b| o.next(b), |b| t.next(b));
        let mut o = TrueRange::new();
        let mut t = ta::indicators::TrueRange::new();
        agree_after("TRUE_RANGE", 0, |b| o.next(b), |b| t.next(b));
        let mut o = OnBalanceVolume::new();
        let mut t = ta::indicators::OnBalanceVolume::new();
        agree_after("OBV", 0, |b| o.next(b), |b| t.next(b));
        let mut o = RateOfChange::new(10).unwrap();
        let mut t = ta::indicators::RateOfChange::new(10).unwrap();
        agree_after("ROC", 10, |b| o.next(b), |b| t.next(b));
        let mut o = EfficiencyRatio::new(10).unwrap();
        let mut t = ta::indicators::EfficiencyRatio::new(10).unwrap();
        agree_after("ER", 10, |b| o.next(b), |b| t.next(b));
        let mut o = BollingerBands::new(20, 2.0).unwrap();
        let mut t = ta::indicators::BollingerBands::new(20, 2.0).unwrap();
        agree_after("BB upper", 19, |b| o.next(b).upper, |b| t.next(b).upper);
    }

    #[test]
    fn rsi_and_atr_differ_from_ta_as_documented() {
        use ta::Next as _;
        let mut o = RelativeStrengthIndex::new(14).unwrap();
        let mut t = ta::indicators::RelativeStrengthIndex::new(14).unwrap();
        let bars = ta_items();
        let ours: Vec<f64> = bars.iter().map(|b| o.next(b)).collect();
        let theirs: Vec<f64> = bars.iter().map(|b| t.next(b)).collect();
        assert!((ours[N - 1] - theirs[N - 1]).abs() > 1e-6);
        assert!((ours[N - 1] - RSI_14[N - 1]).abs() < 1e-9);
    }
}
