//! The `f64` fast path (`signals::float`): every type is checked against its `Decimal`
//! twin bar for bar (same warm-up, values within 1e-9 relative) and, where TA-Lib has
//! the function, against TA-Lib's output for the same 120 bars.
#![cfg(feature = "indicators")]
#![allow(clippy::unwrap_used)]

#[path = "talib_reference/mod.rs"]
mod talib;

use fin_primitives::signals::float::{
    AtrF64, BollingerF64, CciF64, EfficiencyRatioF64, EmaF64, MacdF64, MaxF64, MeanAbsDevF64,
    MfiF64, MinF64, ObvF64, PpoF64, RocF64, RsiF64, SmaF64, StdDevF64, StochasticF64,
    TrueRangeF64,
};
use fin_primitives::signals::indicators::{
    Atr, BollingerB, Cci, EfficiencyRatio, Ema, Macd, Mfi, Obv, Ppo, RollingMAD, Roc, Rsi, Sma,
    StdDev, StochasticK,
};
use fin_primitives::signals::{BarInput, Signal};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use talib::*;

fn dec(x: f64) -> Decimal {
    Decimal::from_str(&x.to_string()).unwrap()
}

fn bars() -> Vec<BarInput> {
    (0..N)
        .map(|i| BarInput::new(dec(CLOSE[i]), dec(HIGH[i]), dec(LOW[i]), dec(OPEN[i]), dec(VOLUME[i])))
        .collect()
}

/// Runs a `Decimal` indicator over the bars; `Unavailable` becomes NaN.
fn run_decimal(mut s: impl Signal) -> Vec<f64> {
    bars()
        .iter()
        .map(|b| s.update(b).unwrap().as_decimal().map_or(f64::NAN, |d| d.to_f64().unwrap()))
        .collect()
}

fn nan(v: Option<f64>) -> f64 {
    v.unwrap_or(f64::NAN)
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

fn closes() -> impl Iterator<Item = f64> {
    CLOSE.iter().copied()
}

#[test]
fn sma_matches_talib_and_decimal() {
    let mut f = Sma::new_f64(10).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(f.update(c))).collect();
    assert_series("SMA vs TA-Lib", &got, &SMA_10);
    assert_series("SMA vs Decimal", &got, &run_decimal(Sma::new("s", 10).unwrap()));
    assert_eq!(f.period(), 10);
}

#[test]
fn ema_matches_talib_and_decimal() {
    let mut f = Ema::new_f64(10).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(f.update(c))).collect();
    assert_series("EMA vs TA-Lib", &got, &EMA_10);
    assert_series("EMA vs Decimal", &got, &run_decimal(Ema::new("e", 10).unwrap()));

    let mut w = Ema::wilder_f64(10).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(w.update(c))).collect();
    assert_series("Wilder EMA vs Decimal", &got, &run_decimal(Ema::wilder("w", 10).unwrap()));
    assert!((w.k() - 0.1).abs() < 1e-15);
}

#[test]
fn rsi_matches_talib_and_decimal() {
    let mut f = Rsi::new_f64(14).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(f.update(c))).collect();
    assert_series("RSI vs TA-Lib", &got, &RSI_14);
    assert_series("RSI vs Decimal", &got, &run_decimal(Rsi::new("r", 14).unwrap()));
    assert_series("Decimal RSI vs TA-Lib", &run_decimal(Rsi::new("r", 14).unwrap()), &RSI_14);
}

#[test]
fn macd_talib_seeding_matches_talib() {
    let mut f = MacdF64::talib(12, 26, 9).unwrap();
    let mut line = Vec::new();
    let mut signal = Vec::new();
    let mut hist = Vec::new();
    for (i, c) in closes().enumerate() {
        let out = f.update(c);
        assert_eq!(f.macd_line().is_some(), i >= 25, "MACD line availability at {i}");
        line.push(out.map_or(f64::NAN, |o| o.macd));
        signal.push(out.map_or(f64::NAN, |o| o.signal));
        hist.push(out.map_or(f64::NAN, |o| o.histogram));
    }
    assert_series("MACD line", &line, &MACD_LINE);
    assert_series("MACD signal", &signal, &MACD_SIGNAL);
    assert_series("MACD histogram", &hist, &MACD_HIST);
}

#[test]
fn macd_new_matches_decimal_macd() {
    let mut f = Macd::new_f64(12, 26, 9).unwrap();
    let got: Vec<f64> = closes().map(|c| f.update(c).map_or(f64::NAN, |o| o.histogram)).collect();
    assert_series("MACD histogram vs Decimal", &got, &run_decimal(Macd::new("m", 12, 26, 9).unwrap()));
    // The two seedings converge but are not the same series.
    let mut t = MacdF64::talib(12, 26, 9).unwrap();
    let talib_first = closes().find_map(|c| t.update(c)).unwrap();
    assert!((talib_first.histogram - got[33]).abs() > 1e-9);
}

#[test]
fn bollinger_matches_talib_and_decimal() {
    let mut f = BollingerB::new_f64(20, 2.0).unwrap();
    let outs: Vec<_> = closes().map(|c| f.update(c)).collect();
    let pick = |g: fn(&fin_primitives::signals::float::BollingerOutputF64) -> f64| -> Vec<f64> {
        outs.iter().map(|o| o.as_ref().map_or(f64::NAN, g)).collect()
    };
    assert_series("BB upper", &pick(|o| o.upper), &BB_UPPER);
    assert_series("BB middle", &pick(|o| o.middle), &BB_MIDDLE);
    assert_series("BB lower", &pick(|o| o.lower), &BB_LOWER);
    assert_series(
        "BB %B vs Decimal",
        &pick(|o| o.percent_b),
        &run_decimal(BollingerB::new("b", 20, Decimal::TWO).unwrap()),
    );
    assert!(BollingerF64::new(20, f64::INFINITY).is_err());
}

#[test]
fn atr_wilder_matches_talib_in_f64_and_decimal() {
    let mut f = Atr::wilder_f64(14).unwrap();
    let got: Vec<f64> = (0..N).map(|i| nan(f.update(HIGH[i], LOW[i], CLOSE[i]))).collect();
    assert_series("Wilder ATR f64 vs TA-Lib", &got, &ATR_14);
    let dec_atr = Atr::wilder("atr", 14).unwrap();
    assert!(dec_atr.is_wilder());
    assert_series("Wilder ATR Decimal vs TA-Lib", &run_decimal(dec_atr), &ATR_14);
}

#[test]
fn atr_simple_matches_decimal_atr() {
    let mut f = Atr::sma_f64(14).unwrap();
    let got: Vec<f64> = (0..N).map(|i| nan(f.update(HIGH[i], LOW[i], CLOSE[i]))).collect();
    let plain = Atr::sma("atr", 14).unwrap();
    assert!(!plain.is_wilder());
    assert_series("SMA ATR vs Decimal", &got, &run_decimal(plain));
}

#[test]
fn atr_new_is_wilder_and_matches_talib() {
    // Since 4.0, Atr::new and Atr::new_f64 are Wilder's ATR: one ATR, the one TA-Lib shows.
    let mut f = Atr::new_f64(14).unwrap();
    let got: Vec<f64> = (0..N).map(|i| nan(f.update(HIGH[i], LOW[i], CLOSE[i]))).collect();
    assert_series("Atr::new_f64 vs TA-Lib", &got, &ATR_14);
    let dec_atr = Atr::new("atr", 14).unwrap();
    assert!(dec_atr.is_wilder());
    assert_series("Atr::new vs TA-Lib", &run_decimal(dec_atr), &ATR_14);
}

#[test]
fn decimal_wilder_atr_reset_and_readiness() {
    let mut atr = Atr::wilder("atr", 14).unwrap();
    let b = bars();
    let first: Vec<_> = b.iter().map(|x| atr.update(x).unwrap()).collect();
    assert!(atr.is_ready());
    atr.reset();
    assert!(!atr.is_ready());
    assert!(atr.is_wilder());
    let second: Vec<_> = b.iter().map(|x| atr.update(x).unwrap()).collect();
    assert_eq!(first, second);
}

#[test]
fn true_range_matches_talib() {
    let mut f = TrueRangeF64::new();
    let got: Vec<f64> = (0..N).map(|i| f.update(HIGH[i], LOW[i], CLOSE[i])).collect();
    assert!((got[0] - (HIGH[0] - LOW[0])).abs() < 1e-12);
    assert_series("TRANGE", &got[1..], &TRANGE[1..]);
    assert_eq!(f.prev_close(), Some(CLOSE[N - 1]));
}

#[test]
fn stddev_matches_talib_and_decimal() {
    let mut f = StdDev::new_f64(20).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(f.update(c))).collect();
    assert_series("STDDEV vs TA-Lib", &got, &STDDEV_20);
    assert_series("STDDEV vs Decimal", &got, &run_decimal(StdDev::new("sd", 20).unwrap()));
    let last_mean = CLOSE[N - 20..].iter().sum::<f64>() / 20.0;
    assert!((f.mean().unwrap() - last_mean).abs() < 1e-9);
}

#[test]
fn rolling_max_min_match_talib() {
    let mut mx = MaxF64::new(14).unwrap();
    let mut mn = MinF64::new(14).unwrap();
    let hi: Vec<f64> = HIGH.iter().map(|&h| nan(mx.update(h))).collect();
    let lo: Vec<f64> = LOW.iter().map(|&l| nan(mn.update(l))).collect();
    assert_series("MAX(high)", &hi, &MAX_HIGH_14);
    assert_series("MIN(low)", &lo, &MIN_LOW_14);
}

#[test]
fn stochastic_matches_talib_and_decimal() {
    let mut f = StochasticK::new_f64(14).unwrap();
    let got: Vec<f64> = (0..N).map(|i| nan(f.update(HIGH[i], LOW[i], CLOSE[i]))).collect();
    assert_series("STOCHF %K vs TA-Lib", &got, &STOCHF_K_14);
    assert_series("%K vs Decimal", &got, &run_decimal(StochasticK::new("k", 14).unwrap()));
    let mut flat = StochasticF64::new(2).unwrap();
    flat.update(5.0, 5.0, 5.0);
    assert_eq!(flat.update(5.0, 5.0, 5.0), Some(50.0));
}

#[test]
fn roc_matches_talib_and_decimal() {
    let mut f = Roc::new_f64(10).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(f.update(c))).collect();
    assert_series("ROC vs TA-Lib", &got, &ROC_10);
    assert_series("ROC vs Decimal", &got, &run_decimal(Roc::new("roc", 10).unwrap()));
    let mut z = RocF64::new(1).unwrap();
    z.update(0.0);
    assert_eq!(z.update(5.0), Some(0.0));
}

#[test]
fn obv_matches_decimal_and_talib_up_to_its_starting_volume() {
    let mut f = Obv::new_f64();
    let got: Vec<f64> = (0..N).map(|i| f.update(CLOSE[i], VOLUME[i])).collect();
    assert_series("OBV vs Decimal", &got, &run_decimal(Obv::new("obv")));
    let shifted: Vec<f64> = got.iter().map(|v| v + VOLUME[0]).collect();
    assert_series("OBV + first volume vs TA-Lib", &shifted, &OBV);
    assert_eq!(f.value(), got[N - 1]);
    let fresh = ObvF64::new();
    assert!(!fresh.is_ready());
}

#[test]
fn mfi_matches_talib_and_decimal() {
    let mut f = Mfi::new_f64(14).unwrap();
    let got: Vec<f64> = (0..N).map(|i| nan(f.update(HIGH[i], LOW[i], CLOSE[i], VOLUME[i]))).collect();
    assert_series("MFI vs TA-Lib", &got, &MFI_14);
    assert_series("MFI vs Decimal", &got, &run_decimal(Mfi::new("mfi", 14).unwrap()));
    assert!(MfiF64::new(3).unwrap().update(1.0, 1.0, 1.0, 1.0).is_none());
}

#[test]
fn cci_matches_talib_and_decimal() {
    let mut f = Cci::new_f64(20).unwrap();
    let got: Vec<f64> = (0..N).map(|i| nan(f.update(HIGH[i], LOW[i], CLOSE[i]))).collect();
    assert_series("CCI vs TA-Lib", &got, &CCI_20);
    assert_series("CCI vs Decimal", &got, &run_decimal(Cci::new("cci", 20).unwrap()));
    let mut flat = CciF64::new(2).unwrap();
    flat.update(1.0, 1.0, 1.0);
    assert_eq!(flat.update(1.0, 1.0, 1.0), Some(0.0));
}

#[test]
fn efficiency_ratio_matches_reference_and_decimal() {
    let mut f = EfficiencyRatio::new_f64(10).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(f.update(c))).collect();
    assert_series("ER vs reference", &got, &ER_10);
    assert_series("ER vs Decimal", &got, &run_decimal(EfficiencyRatio::new("er", 10).unwrap()));
    let mut flat = EfficiencyRatioF64::new(2).unwrap();
    flat.update(3.0);
    flat.update(3.0);
    assert_eq!(flat.update(3.0), Some(0.0));
}

#[test]
fn ppo_talib_seeding_matches_talib_and_new_matches_decimal() {
    let mut t = PpoF64::talib(12, 26).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(t.update(c))).collect();
    assert_series("PPO vs TA-Lib", &got, &PPO_EMA_12_26);
    assert_eq!(PpoF64::talib(26, 12).unwrap().periods(), (12, 26));

    let mut n = Ppo::new_f64(12, 26).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(n.update(c))).collect();
    assert_series("PPO vs Decimal", &got, &run_decimal(Ppo::new("ppo", 12, 26).unwrap()));
}

#[test]
fn mean_abs_dev_matches_reference_and_decimal() {
    let mut f = RollingMAD::new_f64(20).unwrap();
    let got: Vec<f64> = closes().map(|c| nan(f.update(c))).collect();
    assert_series("MAD vs reference", &got, &MAD_20);
    assert_series("MAD vs Decimal", &got, &run_decimal(RollingMAD::new("mad", 20).unwrap()));
}

#[test]
fn every_type_resets_to_a_fresh_state() {
    fn twice<T>(mut make: impl FnMut() -> T, mut run: impl FnMut(&mut T) -> Vec<f64>, mut reset: impl FnMut(&mut T)) {
        let mut a = make();
        let first = run(&mut a);
        reset(&mut a);
        let second = run(&mut a);
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
    }
    twice(|| SmaF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(|| EmaF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(|| RsiF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(
        || MacdF64::talib(3, 6, 4).unwrap(),
        |s| closes().map(|c| s.update(c).map_or(f64::NAN, |o| o.histogram)).collect(),
        |s| s.reset(),
    );
    twice(
        || BollingerF64::new(5, 2.0).unwrap(),
        |s| closes().map(|c| s.update(c).map_or(f64::NAN, |o| o.upper)).collect(),
        |s| s.reset(),
    );
    twice(
        || AtrF64::wilder(5).unwrap(),
        |s| (0..N).map(|i| nan(s.update(HIGH[i], LOW[i], CLOSE[i]))).collect(),
        |s| s.reset(),
    );
    twice(|| StdDevF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(
        || StochasticF64::new(5).unwrap(),
        |s| (0..N).map(|i| nan(s.update(HIGH[i], LOW[i], CLOSE[i]))).collect(),
        |s| s.reset(),
    );
    twice(|| RocF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(ObvF64::new, |s| (0..N).map(|i| s.update(CLOSE[i], VOLUME[i])).collect(), |s| s.reset());
    twice(
        || MfiF64::new(5).unwrap(),
        |s| (0..N).map(|i| nan(s.update(HIGH[i], LOW[i], CLOSE[i], VOLUME[i]))).collect(),
        |s| s.reset(),
    );
    twice(
        || CciF64::new(5).unwrap(),
        |s| (0..N).map(|i| nan(s.update(HIGH[i], LOW[i], CLOSE[i]))).collect(),
        |s| s.reset(),
    );
    twice(|| EfficiencyRatioF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(|| PpoF64::talib(3, 6).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(|| MeanAbsDevF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(|| MaxF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(|| MinF64::new(5).unwrap(), |s| closes().map(|c| nan(s.update(c))).collect(), |s| s.reset());
    twice(TrueRangeF64::new, |s| (0..N).map(|i| s.update(HIGH[i], LOW[i], CLOSE[i])).collect(), |s| s.reset());
}

#[test]
fn readiness_flags_track_warm_up() {
    let mut rsi = RsiF64::new(3).unwrap();
    for (i, c) in [1.0, 2.0, 3.0, 2.5].into_iter().enumerate() {
        let v = rsi.update(c);
        assert_eq!(v.is_some(), rsi.is_ready(), "bar {i}");
    }
    assert!(rsi.is_ready());
    let mut macd = MacdF64::new(2, 3, 2).unwrap();
    assert!(!macd.is_ready());
    for c in [1.0, 2.0, 3.0, 4.0] {
        macd.update(c);
    }
    assert!(macd.is_ready());
    assert_eq!(macd.periods(), (2, 3, 2));
}
