//! [`yata`](https://docs.rs/yata) interop (`yata` feature).
//!
//! [`OhlcvBar`] and [`BarInput`] implement `yata::core::OHLCV`, so yata methods and
//! indicators can read them directly (for example `Source::Close.get(&bar)` or
//! `candle.tp()` style helpers). [`bar_input`] converts any yata `OHLCV` value,
//! such as `yata::core::Candle`, into a [`BarInput`] for fin-primitives indicators.
//!
//! ```
//! # #[cfg(feature = "yata")] {
//! use fin_primitives::signals::BarInput;
//! use rust_decimal_macros::dec;
//! use yata::core::OHLCV;
//!
//! let bar = BarInput { open: dec!(10), high: dec!(12), low: dec!(9), close: dec!(11), volume: dec!(500) };
//! assert_eq!(bar.tp(), (12.0 + 9.0 + 11.0) / 3.0); // yata's typical price
//! # }
//! ```

use super::{decimal_to_f64, f64_to_decimal};
use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::BarInput;
use yata::core::{ValueType, OHLCV};

// `ValueType` is f64 by default and f32 with yata's `value_type_f32` feature; the
// casts below are correct for both.
#[allow(clippy::cast_possible_truncation, clippy::unnecessary_cast)]
fn v(d: rust_decimal::Decimal) -> ValueType {
    decimal_to_f64(d) as ValueType
}

impl OHLCV for OhlcvBar {
    fn open(&self) -> ValueType {
        v(self.open.value())
    }
    fn high(&self) -> ValueType {
        v(self.high.value())
    }
    fn low(&self) -> ValueType {
        v(self.low.value())
    }
    fn close(&self) -> ValueType {
        v(self.close.value())
    }
    fn volume(&self) -> ValueType {
        v(self.volume.value())
    }
}

impl OHLCV for BarInput {
    fn open(&self) -> ValueType {
        v(self.open)
    }
    fn high(&self) -> ValueType {
        v(self.high)
    }
    fn low(&self) -> ValueType {
        v(self.low)
    }
    fn close(&self) -> ValueType {
        v(self.close)
    }
    fn volume(&self) -> ValueType {
        v(self.volume)
    }
}

/// Build a [`BarInput`] from any yata `OHLCV` value (for example `yata::core::Candle`).
///
/// # Errors
/// [`FinError::InvalidInput`] if any field is NaN or infinite.
#[allow(clippy::unnecessary_cast)]
pub fn bar_input<T: OHLCV + ?Sized>(c: &T) -> Result<BarInput, FinError> {
    Ok(BarInput {
        open: f64_to_decimal(c.open() as f64)?,
        high: f64_to_decimal(c.high() as f64)?,
        low: f64_to_decimal(c.low() as f64)?,
        close: f64_to_decimal(c.close() as f64)?,
        volume: f64_to_decimal(c.volume() as f64)?,
    })
}

impl TryFrom<&yata::core::Candle> for BarInput {
    type Error = FinError;

    fn try_from(c: &yata::core::Candle) -> Result<Self, Self::Error> {
        bar_input(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::indicators::Ema;
    use crate::signals::{Signal, SignalValue};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use yata::core::{Candle, Method};
    use yata::methods::EMA;

    #[test]
    fn yata_candle_converts_exactly() {
        let c = Candle { open: 1.5, high: 2.25, low: 1.0, close: 2.0, volume: 10.0 };
        let b = BarInput::try_from(&c).unwrap();
        assert_eq!((b.open, b.high, b.low, b.close, b.volume), (dec!(1.5), dec!(2.25), dec!(1), dec!(2), dec!(10)));
        let bad = Candle { close: f64::NAN, ..c };
        assert!(BarInput::try_from(&bad).is_err());
    }

    #[test]
    fn yata_reads_fin_bars_and_ema_agrees_after_warmup() {
        // yata seeds its EMA with the first value, fin-primitives with an SMA of the first
        // `period` values; after the seed washes out (about 10 periods) they agree.
        let closes: Vec<Decimal> = (0..300).map(|i| Decimal::from(100 + (i * 37 % 23)) / dec!(4)).collect();
        let bars: Vec<BarInput> = closes.iter().map(|&c| BarInput::from_close(c)).collect();
        let mut y = EMA::new(10, &OHLCV::close(&bars[0])).unwrap();
        let mut f = Ema::new("ema", 10).unwrap();
        for (i, b) in bars.iter().enumerate() {
            let yv = y.next(&OHLCV::close(b));
            if let SignalValue::Scalar(fv) = f.update(b).unwrap() {
                if i > 150 {
                    assert!((decimal_to_f64(fv) - yv).abs() < 1e-9, "bar {i}: fin {fv} yata {yv}");
                }
            }
        }
    }
}
