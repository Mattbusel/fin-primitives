//! [`wickra-core`](https://docs.rs/wickra-core) interop (`wickra` feature, needs Rust 1.86+).
//!
//! Convert fin-primitives bars to wickra `Candle`s to run any of wickra's
//! indicators on them, and convert wickra candles back to [`BarInput`] for
//! fin-primitives indicators:
//!
//! ```
//! # #[cfg(feature = "wickra")] {
//! use fin_primitives::signals::BarInput;
//! use rust_decimal_macros::dec;
//! use wickra_core::{Atr, Candle, Indicator};
//!
//! let mut atr = Atr::new(3).unwrap();
//! let bar = BarInput { open: dec!(10), high: dec!(12), low: dec!(9), close: dec!(11), volume: dec!(500) };
//! let candle = Candle::try_from(&bar).unwrap();
//! let _ = atr.update(candle);
//! # }
//! ```
//!
//! wickra's `Candle` carries a caller-defined `i64` timestamp; conversions from
//! [`OhlcvBar`] put `ts_open` in nanoseconds there, and [`BarInput`] has no
//! timestamp so it converts with `0`.

use super::{decimal_to_f64, f64_to_decimal};
use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::BarInput;
use wickra_core::Candle;

fn candle(open: f64, high: f64, low: f64, close: f64, volume: f64, ts: i64) -> Result<Candle, FinError> {
    Candle::new(open, high, low, close, volume, ts).map_err(|e| FinError::InvalidInput(e.to_string()))
}

impl TryFrom<&OhlcvBar> for Candle {
    type Error = FinError;

    /// # Errors
    /// [`FinError::InvalidInput`] if wickra rejects the candle (it re-checks the OHLC
    /// relationships after the conversion to `f64`).
    fn try_from(b: &OhlcvBar) -> Result<Self, Self::Error> {
        candle(
            decimal_to_f64(b.open.value()),
            decimal_to_f64(b.high.value()),
            decimal_to_f64(b.low.value()),
            decimal_to_f64(b.close.value()),
            decimal_to_f64(b.volume.value()),
            b.ts_open.nanos(),
        )
    }
}

impl TryFrom<&BarInput> for Candle {
    type Error = FinError;

    fn try_from(b: &BarInput) -> Result<Self, Self::Error> {
        candle(
            decimal_to_f64(b.open),
            decimal_to_f64(b.high),
            decimal_to_f64(b.low),
            decimal_to_f64(b.close),
            decimal_to_f64(b.volume),
            0,
        )
    }
}

impl TryFrom<&Candle> for BarInput {
    type Error = FinError;

    /// # Errors
    /// [`FinError::InvalidInput`] if a field is NaN or infinite.
    fn try_from(c: &Candle) -> Result<Self, Self::Error> {
        Ok(BarInput {
            open: f64_to_decimal(c.open)?,
            high: f64_to_decimal(c.high)?,
            low: f64_to_decimal(c.low)?,
            close: f64_to_decimal(c.close)?,
            volume: f64_to_decimal(c.volume)?,
        })
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
    use wickra_core::Indicator;

    #[test]
    fn bar_to_candle_and_back_is_exact_for_cent_prices() {
        let bar = OhlcvBar {
            symbol: Symbol::new("ETH-USD").unwrap(),
            open: Price::new(dec!(3186.17)).unwrap(),
            high: Price::new(dec!(3190.94)).unwrap(),
            low: Price::new(dec!(3182.16)).unwrap(),
            close: Price::new(dec!(3190.94)).unwrap(),
            volume: Quantity::new(dec!(12.5)).unwrap(),
            ts_open: NanoTimestamp::new(1_767_625_200_000_000_000),
            ts_close: NanoTimestamp::new(1_767_625_259_000_000_000),
            tick_count: 12,
        };
        let c = Candle::try_from(&bar).unwrap();
        assert_eq!(c.timestamp, 1_767_625_200_000_000_000);
        let back = BarInput::try_from(&c).unwrap();
        assert_eq!(back, BarInput::from(&bar));
    }

    #[test]
    fn wickra_sma_matches_fin_sma() {
        let mut w = wickra_core::Sma::new(5).unwrap();
        let mut f = Sma::new("sma", 5).unwrap();
        for i in 0..50 {
            let close = Decimal::from(1000 + (i * 7919) % 97) / dec!(10);
            let wv = w.update(decimal_to_f64(close));
            match (f.update(&BarInput::from_close(close)).unwrap(), wv) {
                (SignalValue::Scalar(fv), Some(wv)) => assert!((decimal_to_f64(fv) - wv).abs() < 1e-9),
                (SignalValue::Unavailable, None) => {}
                (a, b) => panic!("warm-up differs at {i}: fin {a:?} wickra {b:?}"),
            }
        }
    }

    #[test]
    fn invalid_candle_is_an_error_not_a_panic() {
        let bad = BarInput { open: dec!(10), high: dec!(9), low: dec!(8), close: dec!(9), volume: dec!(1) };
        assert!(Candle::try_from(&bad).is_err()); // high below open
    }
}
