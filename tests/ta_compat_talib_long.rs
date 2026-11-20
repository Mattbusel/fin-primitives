//! `ta_compat` ADX, DX, +DI, -DI, DEMA, WMA and Hull against TA-Lib (the `ta-lib` crate, a
//! native Rust port of the C reference) on 20,000 bars, several periods each, including
//! runs of flat bars where the true range is zero.
#![cfg(feature = "ta-compat")]
#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]

use fin_primitives::ta_compat::indicators::{
    AverageDirectionalIndex, DirectionalIndex, DoubleExponentialMovingAverage, HullMovingAverage,
    WeightedMovingAverage,
};
use fin_primitives::ta_compat::{DataItem, Next};
use ta_lib::Core;

const N: usize = 20_000;

/// Deterministic OHLC random walk in cents, with a flat stretch every 2,000 bars.
fn bars() -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s % 10_000) as f64 / 10_000.0
    };
    let (mut o, mut h, mut l, mut c) = (vec![], vec![], vec![], vec![]);
    let mut px = 100.0f64;
    for i in 0..N {
        if i % 2_000 < 30 && i >= 2_000 {
            let p = (px * 100.0).round() / 100.0;
            o.push(p);
            h.push(p);
            l.push(p);
            c.push(p);
            continue;
        }
        let open = ((px * (1.0 + (next() - 0.5) * 0.01)) * 100.0).round() / 100.0;
        let close = ((open * (1.0 + (next() - 0.5) * 0.04)) * 100.0).round() / 100.0;
        let high = ((open.max(close) * (1.0 + next() * 0.01)) * 100.0).round() / 100.0;
        let low = ((open.min(close) * (1.0 - next() * 0.01)) * 100.0).round() / 100.0;
        o.push(open);
        h.push(high);
        l.push(low);
        c.push(close);
        px = close.max(1.0);
    }
    (o, h, l, c)
}

fn items(o: &[f64], h: &[f64], l: &[f64], c: &[f64]) -> Vec<DataItem> {
    (0..o.len())
        .map(|i| {
            DataItem::builder()
                .open(o[i])
                .high(h[i])
                .low(l[i])
                .close(c[i])
                .volume(1.0)
                .build()
                .unwrap()
        })
        .collect()
}

/// Places a TA-Lib output (`count` values from `beg`) into a full-length series with NaN
/// warm-up.
fn full(out: &[f64], beg: usize, count: usize, n: usize) -> Vec<f64> {
    let mut v = vec![f64::NAN; n];
    v[beg..beg + count].copy_from_slice(&out[..count]);
    v
}

#[track_caller]
fn assert_close(what: &str, got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len());
    let mut compared = 0;
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        if w.is_nan() {
            assert!(g.is_nan(), "{what}[{i}]: expected warm-up NaN, got {g}");
        } else {
            assert!(
                (g - w).abs() <= 1e-9 * w.abs().max(1.0),
                "{what}[{i}]: got {g}, TA-Lib {w}"
            );
            compared += 1;
        }
    }
    assert!(compared > N / 2, "{what}: only {compared} values compared");
}

#[test]
fn adx_dx_di_match_talib_on_20k_bars() {
    let core = Core::new();
    let (o, h, l, c) = bars();
    let bars = items(&o, &h, &l, &c);
    for period in [2usize, 5, 14, 30] {
        let p = period as i32;
        let mut out = vec![0.0; N];
        let r = core.adx(0, N - 1, &h, &l, &c, p, &mut out).unwrap();
        let want_adx = full(&out, r.beg_idx, r.count, N);
        let r = core.dx(0, N - 1, &h, &l, &c, p, &mut out).unwrap();
        let want_dx = full(&out, r.beg_idx, r.count, N);
        let r = core.plus_di(0, N - 1, &h, &l, &c, p, &mut out).unwrap();
        let want_pdi = full(&out, r.beg_idx, r.count, N);
        let r = core.minus_di(0, N - 1, &h, &l, &c, p, &mut out).unwrap();
        let want_mdi = full(&out, r.beg_idx, r.count, N);

        let mut adx = AverageDirectionalIndex::new(period).unwrap();
        let got: Vec<f64> = bars.iter().map(|b| adx.next(b)).collect();
        assert_close(&format!("ADX({period})"), &got, &want_adx);

        let mut dx = DirectionalIndex::new(period).unwrap();
        let outs: Vec<_> = bars.iter().map(|b| dx.next(b)).collect();
        assert_close(
            &format!("DX({period})"),
            &outs.iter().map(|x| x.dx).collect::<Vec<_>>(),
            &want_dx,
        );
        assert_close(
            &format!("PLUS_DI({period})"),
            &outs.iter().map(|x| x.plus_di).collect::<Vec<_>>(),
            &want_pdi,
        );
        assert_close(
            &format!("MINUS_DI({period})"),
            &outs.iter().map(|x| x.minus_di).collect::<Vec<_>>(),
            &want_mdi,
        );
    }
}

fn talib_wma(core: &Core, x: &[f64], period: usize) -> Vec<f64> {
    // Start at the first non-NaN input, as the Python wrapper does.
    let first = x.iter().position(|v| !v.is_nan()).unwrap();
    let tail = &x[first..];
    let mut out = vec![0.0; tail.len()];
    let r = core
        .wma(0, tail.len() - 1, tail, period as i32, &mut out)
        .unwrap();
    full(&out, first + r.beg_idx, r.count, x.len())
}

#[test]
fn dema_wma_hull_match_talib_on_20k_bars() {
    let core = Core::new();
    let (_, _, _, c) = bars();
    for period in [2usize, 3, 9, 16, 50] {
        let mut out = vec![0.0; N];
        let r = core.dema(0, N - 1, &c, period as i32, &mut out).unwrap();
        let want = full(&out, r.beg_idx, r.count, N);
        let mut dema = DoubleExponentialMovingAverage::new(period).unwrap();
        let got: Vec<f64> = c.iter().map(|&x| dema.next(x)).collect();
        assert_close(&format!("DEMA({period})"), &got, &want);

        let want = talib_wma(&core, &c, period);
        let mut wma = WeightedMovingAverage::new(period).unwrap();
        let got: Vec<f64> = c.iter().map(|&x| wma.next(x)).collect();
        assert_close(&format!("WMA({period})"), &got, &want);

        // TA-Lib's WMA needs a period of at least 2, so Hull from period 4 on.
        if period < 4 {
            continue;
        }
        let half = talib_wma(&core, &c, period / 2);
        let fullp = talib_wma(&core, &c, period);
        let diff: Vec<f64> = half.iter().zip(&fullp).map(|(a, b)| 2.0 * a - b).collect();
        let root = (period as f64).sqrt().floor() as usize;
        let want = talib_wma(&core, &diff, root);
        let mut hma = HullMovingAverage::new(period).unwrap();
        let got: Vec<f64> = c.iter().map(|&x| hma.next(x)).collect();
        assert_close(&format!("HMA({period})"), &got, &want);
    }
}
