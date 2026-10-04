//! [`ta`](https://docs.rs/ta) interop (`ta` feature).
//!
//! [`OhlcvBar`] and [`BarInput`] implement ta's `Open`, `High`, `Low`, `Close` and
//! `Volume` traits, so any ta indicator accepts them where it accepts a
//! `ta::DataItem`:
//!
//! ```
//! # #[cfg(feature = "ta")] {
//! use fin_primitives::signals::BarInput;
//! use rust_decimal_macros::dec;
//! use ta::indicators::RelativeStrengthIndex;
//! use ta::Next;
//!
//! let mut rsi = RelativeStrengthIndex::new(14).unwrap();
//! let bar = BarInput::from_close(dec!(64250.50));
//! let value: f64 = rsi.next(&bar); // ta reads the close through `ta::Close`
//! assert!(value.is_finite());
//! # }
//! ```
//!
//! Note that ta's `RelativeStrengthIndex` smooths with an EMA of factor `2 / (n + 1)`,
//! while [`Rsi`](crate::signals::indicators::Rsi) uses Wilder's `1 / n` like TA-Lib, so
//! the two give different numbers for the same bars by design.

use super::{decimal_to_f64, f64_to_decimal};
use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::BarInput;

macro_rules! impl_ta_traits {
    ($ty:ty, $open:expr, $high:expr, $low:expr, $close:expr, $volume:expr) => {
        impl ta::Open for $ty {
            fn open(&self) -> f64 {
                decimal_to_f64($open(self))
            }
        }
        impl ta::High for $ty {
            fn high(&self) -> f64 {
                decimal_to_f64($high(self))
            }
        }
        impl ta::Low for $ty {
            fn low(&self) -> f64 {
                decimal_to_f64($low(self))
            }
        }
        impl ta::Close for $ty {
            fn close(&self) -> f64 {
                decimal_to_f64($close(self))
            }
        }
        impl ta::Volume for $ty {
            fn volume(&self) -> f64 {
                decimal_to_f64($volume(self))
            }
        }
    };
}

impl_ta_traits!(
    OhlcvBar,
    |b: &OhlcvBar| b.open.value(),
    |b: &OhlcvBar| b.high.value(),
    |b: &OhlcvBar| b.low.value(),
    |b: &OhlcvBar| b.close.value(),
    |b: &OhlcvBar| b.volume.value()
);
impl_ta_traits!(
    BarInput,
    |b: &BarInput| b.open,
    |b: &BarInput| b.high,
    |b: &BarInput| b.low,
    |b: &BarInput| b.close,
    |b: &BarInput| b.volume
);

/// Build a [`BarInput`] from anything ta can read (a `ta::DataItem`, or your own
/// type that implements ta's five traits), so fin-primitives indicators can run on
/// data you already hold in ta's format.
///
/// # Errors
/// [`FinError::InvalidInput`] if any field is NaN or infinite.
pub fn bar_input<T>(item: &T) -> Result<BarInput, FinError>
where
    T: ta::Open + ta::High + ta::Low + ta::Close + ta::Volume,
{
    Ok(BarInput {
        open: f64_to_decimal(item.open())?,
        high: f64_to_decimal(item.high())?,
        low: f64_to_decimal(item.low())?,
        close: f64_to_decimal(item.close())?,
        volume: f64_to_decimal(item.volume())?,
    })
}

impl TryFrom<&ta::DataItem> for BarInput {
    type Error = FinError;

    fn try_from(item: &ta::DataItem) -> Result<Self, Self::Error> {
        bar_input(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::indicators::Sma;
    use crate::signals::{Signal, SignalValue};
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ta::indicators::{AverageTrueRange, SimpleMovingAverage};
    use ta::{DataItem, Next};

    fn bars() -> Vec<OhlcvBar> {
        let closes = [100, 102, 101, 105, 107, 104, 108, 111, 109, 112, 115, 113];
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let c = Decimal::from(c);
                OhlcvBar {
                    symbol: Symbol::new("X").unwrap(),
                    open: Price::new(c - dec!(0.5)).unwrap(),
                    high: Price::new(c + dec!(1.25)).unwrap(),
                    low: Price::new(c - dec!(1.75)).unwrap(),
                    close: Price::new(c).unwrap(),
                    volume: Quantity::new(dec!(1000) + Decimal::from(i)).unwrap(),
                    ts_open: NanoTimestamp::new(i as i64),
                    ts_close: NanoTimestamp::new(i as i64 + 1),
                    tick_count: 1,
                }
            })
            .collect()
    }

    #[test]
    fn ta_indicators_run_on_fin_bars_like_on_data_items() {
        let mut on_bars = AverageTrueRange::new(5).unwrap();
        let mut on_items = AverageTrueRange::new(5).unwrap();
        for b in bars() {
            let item = DataItem::builder()
                .open(ta::Open::open(&b))
                .high(ta::High::high(&b))
                .low(ta::Low::low(&b))
                .close(ta::Close::close(&b))
                .volume(ta::Volume::volume(&b))
                .build()
                .unwrap();
            assert_eq!(on_bars.next(&b), on_items.next(&item));
        }
    }

    #[test]
    fn fin_sma_matches_ta_sma_on_the_same_bars() {
        // Cross-check against an independent implementation.
        let mut ta_sma = SimpleMovingAverage::new(4).unwrap();
        let mut fin_sma = Sma::new("sma", 4).unwrap();
        for (i, b) in bars().iter().enumerate() {
            let t = ta_sma.next(b);
            let f = fin_sma.update_bar(b).unwrap();
            if i >= 3 {
                let SignalValue::Scalar(f) = f else { panic!("not ready at {i}") };
                assert!((decimal_to_f64(f) - t).abs() < 1e-9, "bar {i}: fin {f} ta {t}");
            }
        }
    }

    #[test]
    fn data_item_converts_to_bar_input_exactly() {
        let item = DataItem::builder().open(1.1).high(2.2).low(0.9).close(2.0).volume(30.5).build().unwrap();
        let b = BarInput::try_from(&item).unwrap();
        assert_eq!((b.open, b.high, b.low, b.close, b.volume), (dec!(1.1), dec!(2.2), dec!(0.9), dec!(2), dec!(30.5)));
    }
}
