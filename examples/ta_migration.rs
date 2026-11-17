//! Moving from the `ta` crate (0.5, last release 2021) to `fin_primitives::ta_compat`.
//!
//! ```text
//! cargo run --example ta_migration --features ta-compat
//! ```
//!
//! The migration is the dependency line plus one `use` line per file:
//!
//! ```toml
//! # ta = "0.5"
//! fin-primitives = { version = "3.4", default-features = false, features = ["ta-compat"] }
//! ```
//!
//! ```rust,ignore
//! // use ta::indicators::{ExponentialMovingAverage, RelativeStrengthIndex};
//! // use ta::{DataItem, Next};
//! use fin_primitives::ta_compat as ta;
//! ```
//!
//! Everything below is code written for `ta` and unchanged after that line, plus the
//! indicators `ta` users asked for and `ta` never shipped: ADX and DX (+DI, -DI), DEMA, WMA
//! and the Hull moving average. All of them give TA-Lib's numbers (`ta`'s RSI and ATR do
//! not), and `NaN` during warm-up.

#![allow(clippy::expect_used)]

use fin_primitives::ta_compat as ta;
use ta::indicators::{
    AverageDirectionalIndex, AverageTrueRange, DirectionalIndex, DoubleExponentialMovingAverage,
    ExponentialMovingAverage, HullMovingAverage, RelativeStrengthIndex,
};
use ta::{Close as _, DataItem, Next, Reset};

fn bars() -> Vec<DataItem> {
    // A small deterministic series: an uptrend, a pullback, another leg up.
    let closes = [
        100.0, 101.2, 102.1, 101.8, 103.0, 104.2, 103.9, 105.1, 106.0, 105.4, 104.1, 103.0, 102.2,
        102.9, 104.0, 105.3, 106.8, 107.5, 108.9, 109.4, 110.8, 111.2, 112.6, 113.1, 114.0,
        113.5, 114.9, 116.2, 117.0, 118.3,
    ];
    let mut prev: f64 = 99.5;
    closes
        .iter()
        .map(|&c| {
            let open = prev;
            let bar = DataItem::builder()
                .open(open)
                .high(open.max(c) + 0.6)
                .low(open.min(c) - 0.5)
                .close(c)
                .volume(1_000.0)
                .build()
                .expect("valid bar");
            prev = c;
            bar
        })
        .collect()
}

fn main() {
    let bars = bars();

    // Indicators `ta` 0.5 has, with TA-Lib's values.
    let mut ema = ExponentialMovingAverage::new(10).expect("period > 0");
    let mut rsi = RelativeStrengthIndex::new(14).expect("period > 0");
    let mut atr = AverageTrueRange::new(14).expect("period > 0");

    // Indicators `ta` 0.5 does not have.
    let mut adx = AverageDirectionalIndex::new(14).expect("period > 0");
    let mut dmi = DirectionalIndex::new(14).expect("period > 0");
    let mut dema = DoubleExponentialMovingAverage::new(10).expect("period > 0");
    let mut hma = HullMovingAverage::new(9).expect("period >= 2");

    println!("bar   close    EMA10    RSI14    ATR14   DEMA10    HMA9    +DI    -DI    ADX14");
    for (i, bar) in bars.iter().enumerate() {
        let d = dmi.next(bar);
        println!(
            "{i:>3} {:>8.2} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>7.3} {:>6.2} {:>6.2} {:>8.3}",
            bar.close(),
            ema.next(bar),
            rsi.next(bar),
            atr.next(bar),
            dema.next(bar),
            hma.next(bar),
            d.plus_di,
            d.minus_di,
            adx.next(bar),
        );
    }

    // A strategy rule written for `ta` keeps working: comparisons with NaN are false, so it
    // does not fire during warm-up.
    adx.reset();
    let trending = bars.iter().map(|b| adx.next(b)).filter(|&v| v > 25.0).count();
    println!("bars with ADX(14) > 25: {trending}");
}
