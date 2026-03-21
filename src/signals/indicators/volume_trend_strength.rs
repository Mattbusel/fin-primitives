//! Volume Trend Strength indicator.
//!
//! EMA of signed volume: volume is added positively on up-closes and
//! negatively on down-closes, then smoothed. Measures whether buying or
//! selling volume is dominating over time.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `volume × sign(close − open)`.
///
/// For each bar the raw contribution is:
/// ```text
/// raw = +volume   when close > open  (bullish bar)
///     = -volume   when close < open  (bearish bar)
///     = 0         when close == open (neutral bar)
/// ```
///
/// The EMA of these signed volumes smooths towards positive values when
/// bullish bars consistently carry heavier volume than bearish bars, and
/// towards negative values when the opposite is true.
///
/// Returns a value after the first bar (EMA seeds with the first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeTrendStrength;
/// use fin_primitives::signals::Signal;
///
/// let vts = VolumeTrendStrength::new("vts", 14).unwrap();
/// assert_eq!(vts.period(), 14);
/// ```
pub struct VolumeTrendStrength {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl VolumeTrendStrength {
    /// Constructs a new `VolumeTrendStrength`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { name: name.into(), period, ema: None, k })
    }
}

impl Signal for VolumeTrendStrength {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let raw = if bar.close > bar.open {
            bar.volume
        } else if bar.close < bar.open {
            -bar.volume
        } else {
            Decimal::ZERO
        };

        let ema = match self.ema {
            None => {
                self.ema = Some(raw);
                raw
            }
            Some(prev) => {
                let next = raw * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(next);
                next
            }
        };

        Ok(SignalValue::Scalar(ema))
    }

    fn reset(&mut self) {
        self.ema = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let (high, low) = if c >= o { (c, o) } else { (o, c) };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o,
            high,
            low,
            close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vts_invalid_period() {
        assert!(VolumeTrendStrength::new("vts", 0).is_err());
    }

    #[test]
    fn test_vts_ready_after_first_bar() {
        let mut vts = VolumeTrendStrength::new("vts", 5).unwrap();
        vts.update_bar(&bar("100", "105", "1000")).unwrap();
        assert!(vts.is_ready());
    }

    #[test]
    fn test_vts_bullish_bar_positive() {
        let mut vts = VolumeTrendStrength::new("vts", 5).unwrap();
        let v = vts.update_bar(&bar("100", "110", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(500)));
    }

    #[test]
    fn test_vts_bearish_bar_negative() {
        let mut vts = VolumeTrendStrength::new("vts", 5).unwrap();
        let v = vts.update_bar(&bar("110", "100", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-500)));
    }

    #[test]
    fn test_vts_neutral_bar_zero() {
        let mut vts = VolumeTrendStrength::new("vts", 5).unwrap();
        let v = vts.update_bar(&bar("100", "100", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vts_persistent_bullish_positive() {
        let mut vts = VolumeTrendStrength::new("vts", 3).unwrap();
        for _ in 0..10 {
            vts.update_bar(&bar("100", "105", "1000")).unwrap();
        }
        let v = vts.update_bar(&bar("100", "105", "1000")).unwrap();
        if let SignalValue::Scalar(e) = v {
            assert!(e > dec!(0), "persistent bullish should be positive: {e}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vts_reset() {
        let mut vts = VolumeTrendStrength::new("vts", 5).unwrap();
        vts.update_bar(&bar("100", "105", "1000")).unwrap();
        assert!(vts.is_ready());
        vts.reset();
        assert!(!vts.is_ready());
    }
}
