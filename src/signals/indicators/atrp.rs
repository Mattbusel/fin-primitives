//! ATR Percentage (ATRP) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use crate::signals::indicators::Atr;
use rust_decimal::Decimal;

/// ATR Percentage (ATRP) — normalised ATR expressed as a fraction of the close.
///
/// ```text
/// ATRP = ATR(period) / close × 100
/// ```
///
/// Allows volatility comparison across instruments with different price levels.
/// Returns `SignalValue::Unavailable` until the inner ATR is ready, or when `close == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Atrp;
/// use fin_primitives::signals::Signal;
/// let atrp = Atrp::new("atrp14", 14).unwrap();
/// assert_eq!(atrp.period(), 14);
/// ```
pub struct Atrp {
    name: String,
    atr: Atr,
}

impl Atrp {
    /// Constructs a new `Atrp` indicator.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        let n: String = name.into();
        let atr = Atr::new(format!("{}_atr", n), period)?;
        Ok(Self { name: n, atr })
    }
}

impl Signal for Atrp {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let atr_val = match self.atr.update(bar)? {
            SignalValue::Scalar(v) => v,
            SignalValue::Unavailable => return Ok(SignalValue::Unavailable),
        };
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let pct = atr_val
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool {
        self.atr.is_ready()
    }

    fn period(&self) -> usize {
        self.atr.period()
    }

    fn reset(&mut self) {
        self.atr.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_atrp_period_zero_fails() {
        assert!(Atrp::new("atrp", 0).is_err());
    }

    #[test]
    fn test_atrp_unavailable_before_warmup() {
        let mut atrp = Atrp::new("atrp3", 3).unwrap();
        assert_eq!(atrp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!atrp.is_ready());
    }

    #[test]
    fn test_atrp_ready_after_warmup() {
        let mut atrp = Atrp::new("atrp3", 3).unwrap();
        for p in &["100", "102", "101", "103"] {
            atrp.update_bar(&bar(p)).unwrap();
        }
        assert!(atrp.is_ready());
        // should produce a positive scalar
        let v = atrp.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val > Decimal::ZERO, "ATRP should be positive");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_atrp_constant_prices_low_atrp() {
        let mut atrp = Atrp::new("atrp3", 3).unwrap();
        for _ in 0..10 {
            atrp.update_bar(&bar("100")).unwrap();
        }
        assert!(atrp.is_ready());
        let v = atrp.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(0), "constant prices → zero ATR → zero ATRP");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_atrp_reset_clears_state() {
        let mut atrp = Atrp::new("atrp3", 3).unwrap();
        for p in &["100", "102", "101", "103"] {
            atrp.update_bar(&bar(p)).unwrap();
        }
        assert!(atrp.is_ready());
        atrp.reset();
        assert!(!atrp.is_ready());
    }
}
