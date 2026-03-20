//! McGinley Dynamic indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// McGinley Dynamic — a self-adjusting moving average that adapts to market speed.
///
/// ```text
/// MD[i] = MD[i-1] + (close - MD[i-1]) / (N * (close / MD[i-1])^4)
/// ```
///
/// The first bar seeds `MD` with the closing price. Returns
/// [`SignalValue::Unavailable`] only on the very first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::McGinley;
/// use fin_primitives::signals::Signal;
///
/// let mg = McGinley::new("mg14", 14).unwrap();
/// assert_eq!(mg.period(), 14);
/// ```
pub struct McGinley {
    name: String,
    period: usize,
    value: Option<Decimal>,
    ready: bool,
}

impl McGinley {
    /// Constructs a new `McGinley` dynamic indicator.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            value: None,
            ready: false,
        })
    }
}

impl Signal for McGinley {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let close = bar.close;

        let Some(prev) = self.value else {
            self.value = Some(close);
            return Ok(SignalValue::Unavailable);
        };

        if prev.is_zero() {
            self.value = Some(close);
            return Ok(SignalValue::Scalar(close));
        }

        let close_f = close.to_f64().unwrap_or(0.0);
        let prev_f  = prev.to_f64().unwrap_or(1.0);
        let n       = self.period as f64;
        let ratio   = close_f / prev_f;
        let denom   = n * ratio.powi(4);

        let new_val = if denom == 0.0 {
            prev_f
        } else {
            prev_f + (close_f - prev_f) / denom
        };

        let result = Decimal::try_from(new_val).unwrap_or(prev);
        self.value = Some(result);
        self.ready = true;
        Ok(SignalValue::Scalar(result))
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.value = None;
        self.ready = false;
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
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_mcginley_invalid_period() {
        assert!(McGinley::new("mg", 0).is_err());
    }

    #[test]
    fn test_mcginley_first_bar_unavailable() {
        let mut mg = McGinley::new("mg", 14).unwrap();
        assert_eq!(mg.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!mg.is_ready());
    }

    #[test]
    fn test_mcginley_second_bar_produces_scalar() {
        let mut mg = McGinley::new("mg", 14).unwrap();
        mg.update_bar(&bar("100")).unwrap();
        let v = mg.update_bar(&bar("102")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(mg.is_ready());
    }

    #[test]
    fn test_mcginley_constant_price_converges() {
        // When close == MD, the ratio = 1, denom = N, so MD moves toward close slowly.
        let mut mg = McGinley::new("mg", 5).unwrap();
        for _ in 0..50 {
            mg.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = mg.update_bar(&bar("100")).unwrap() {
            let diff = (v - dec!(100)).abs();
            assert!(diff < dec!(0.01), "Expected ~100, got {v}");
        }
    }

    #[test]
    fn test_mcginley_reset() {
        let mut mg = McGinley::new("mg", 5).unwrap();
        mg.update_bar(&bar("100")).unwrap();
        mg.update_bar(&bar("101")).unwrap();
        assert!(mg.is_ready());
        mg.reset();
        assert!(!mg.is_ready());
    }
}
