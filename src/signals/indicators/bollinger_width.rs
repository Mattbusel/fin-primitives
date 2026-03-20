//! Bollinger Band Width indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use crate::signals::indicators::BollingerB;
use rust_decimal::Decimal;

/// Bollinger Band Width — measures the width of the Bollinger Bands relative to the middle band.
///
/// `Width = (Upper - Lower) / Middle`
///
/// A widening band indicates increasing volatility; narrowing indicates a squeeze.
/// Returns [`SignalValue::Unavailable`] until the underlying Bollinger indicator is ready.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BollingerWidth;
/// use fin_primitives::signals::Signal;
///
/// let bw = BollingerWidth::new("bw", 20, "2.0".parse().unwrap()).unwrap();
/// assert_eq!(bw.period(), 20);
/// ```
pub struct BollingerWidth {
    name: String,
    inner: BollingerB,
}

impl BollingerWidth {
    /// Constructs a new `BollingerWidth` with the same parameters as `BollingerB`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize, std_dev: Decimal) -> Result<Self, FinError> {
        let inner = BollingerB::new("_bw_inner", period, std_dev)?;
        Ok(Self { name: name.into(), inner })
    }
}

impl Signal for BollingerWidth {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // We need access to the raw band values. Use the inner BollingerB bands() method.
        self.inner.update(bar)?;
        match self.inner.bands() {
            Some((upper, middle, lower)) => {
                if middle.is_zero() {
                    return Ok(SignalValue::Unavailable);
                }
                Ok(SignalValue::Scalar((upper - lower) / middle))
            }
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }

    fn period(&self) -> usize {
        self.inner.period()
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl, high: cl, low: cl, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bw_period_zero_fails() {
        assert!(BollingerWidth::new("bw", 0, dec!(2)).is_err());
    }

    #[test]
    fn test_bw_unavailable_before_period() {
        let mut bw = BollingerWidth::new("bw", 5, dec!(2)).unwrap();
        for _ in 0..4 {
            assert_eq!(bw.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!bw.is_ready());
    }

    #[test]
    fn test_bw_flat_series_zero_width() {
        let mut bw = BollingerWidth::new("bw", 5, dec!(2)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = bw.update_bar(&bar("100")).unwrap();
        }
        // Flat price: std_dev = 0 → width = 0
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bw_reset() {
        let mut bw = BollingerWidth::new("bw", 5, dec!(2)).unwrap();
        for _ in 0..10 { bw.update_bar(&bar("100")).unwrap(); }
        assert!(bw.is_ready());
        bw.reset();
        assert!(!bw.is_ready());
    }
}
