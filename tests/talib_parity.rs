//! Parity with TA-Lib (the `ta-lib` crate, a native Rust port of the C reference).
//!
//! Same warm-up bar, same number of values, and agreement to float rounding
//! (fin-primitives computes in `Decimal`, TA-Lib in `f64`).

use fin_primitives::signals::indicators::Rsi;
use fin_primitives::signals::{BarInput, Signal as _, SignalValue};
use rust_decimal::prelude::*;
use ta_lib::Core;

/// Deterministic random walk with prices rounded to cents.
fn closes(n: usize) -> Vec<f64> {
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    let mut px = 100.0f64;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let r = (s % 2001) as f64 / 1000.0 - 1.0;
            px = (px * (1.0 + r * 0.02)).max(1.0);
            (px * 100.0).round() / 100.0
        })
        .collect()
}

#[test]
fn rsi_matches_talib() {
    let core = Core::new();
    let c = closes(5_000);
    for period in [2usize, 14, 30, 100] {
        let mut reference = vec![0.0; c.len()];
        let range = core
            .rsi(0, c.len() - 1, &c, period as i32, &mut reference)
            .expect("TA-Lib RSI");
        let mut rsi = Rsi::new("rsi", period).unwrap();
        let mut compared = 0;
        for (i, &x) in c.iter().enumerate() {
            let bar = BarInput::from_close(Decimal::from_f64(x).unwrap());
            if let SignalValue::Scalar(v) = rsi.update(&bar).unwrap() {
                assert!(
                    i >= range.beg_idx,
                    "period {period}: value at bar {i} before TA-Lib's lookback"
                );
                let want = reference[i - range.beg_idx];
                let got = v.to_f64().unwrap();
                assert!(
                    (got - want).abs() < 1e-9,
                    "period {period}, bar {i}: {got} vs TA-Lib {want}"
                );
                compared += 1;
            }
        }
        assert_eq!(
            compared, range.count,
            "period {period}: value count differs from TA-Lib"
        );
    }
}
