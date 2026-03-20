//! TRIX indicator — triple-smoothed EMA rate of change.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// TRIX — 1-period percentage rate of change of a triple-smoothed EMA.
///
/// ```text
/// EMA1 = EMA(close, period)
/// EMA2 = EMA(EMA1, period)
/// EMA3 = EMA(EMA2, period)
/// TRIX = (EMA3 - EMA3_prev) / EMA3_prev × 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `3 × (period - 1) + 2` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Trix;
/// use fin_primitives::signals::Signal;
///
/// let mut trix = Trix::new("trix5", 5).unwrap();
/// ```
pub struct Trix {
    name: String,
    period: usize,
    k: Decimal,         // smoothing factor
    ema1: Option<Decimal>,
    ema2: Option<Decimal>,
    ema3: Option<Decimal>,
    prev_ema3: Option<Decimal>,
    bars_seen: usize,
}

impl Trix {
    /// Constructs a new `Trix` indicator.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period` is zero.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / Decimal::from(period as u64 + 1);
        Ok(Self {
            name: name.into(),
            period,
            k,
            ema1: None,
            ema2: None,
            ema3: None,
            prev_ema3: None,
            bars_seen: 0,
        })
    }

    fn apply_ema(prev: Option<Decimal>, price: Decimal, k: Decimal) -> Decimal {
        match prev {
            None => price,
            Some(e) => price * k + e * (Decimal::ONE - k),
        }
    }
}

impl Signal for Trix {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars_seen += 1;
        let close = bar.close;

        self.ema1 = Some(Self::apply_ema(self.ema1, close, self.k));
        let e1 = self.ema1.unwrap();

        self.ema2 = Some(Self::apply_ema(self.ema2, e1, self.k));
        let e2 = self.ema2.unwrap();

        self.ema3 = Some(Self::apply_ema(self.ema3, e2, self.k));
        let e3 = self.ema3.unwrap();

        // Need prev_ema3 to compute rate of change; that takes 3*(period-1)+2 bars
        let ready_at = 3 * (self.period.saturating_sub(1)) + 2;
        if self.bars_seen < ready_at {
            self.prev_ema3 = Some(e3);
            return Ok(SignalValue::Unavailable);
        }

        let result = match self.prev_ema3 {
            Some(prev) if !prev.is_zero() => {
                let trix = (e3 - prev) / prev * Decimal::ONE_HUNDRED;
                self.prev_ema3 = Some(e3);
                SignalValue::Scalar(trix)
            }
            _ => {
                self.prev_ema3 = Some(e3);
                SignalValue::Unavailable
            }
        };
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        let ready_at = 3 * (self.period.saturating_sub(1)) + 2;
        self.bars_seen >= ready_at && self.prev_ema3.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ema1 = None;
        self.ema2 = None;
        self.ema3 = None;
        self.prev_ema3 = None;
        self.bars_seen = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;

    fn bar(close: &str) -> BarInput {
        BarInput::from_close(close.parse().unwrap())
    }

    #[test]
    fn test_trix_period_zero_error() {
        assert!(Trix::new("trix", 0).is_err());
    }

    #[test]
    fn test_trix_unavailable_before_ready() {
        let mut trix = Trix::new("trix3", 3).unwrap();
        // ready_at = 3*(3-1)+2 = 8
        for _ in 0..7 {
            let r = trix.update(&bar("100")).unwrap();
            assert_eq!(r, SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_trix_ready_after_sufficient_bars() {
        let mut trix = Trix::new("trix3", 3).unwrap();
        for _ in 0..7 {
            trix.update(&bar("100")).unwrap();
        }
        let r = trix.update(&bar("100")).unwrap();
        assert!(matches!(r, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_trix_constant_price_is_zero() {
        let mut trix = Trix::new("trix3", 3).unwrap();
        for _ in 0..8 {
            trix.update(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = trix.update(&bar("100")).unwrap() {
            // At constant price, all EMAs converge to 100, rate of change → 0
            assert!(v.abs() < rust_decimal_macros::dec!(0.0001));
        }
    }

    #[test]
    fn test_trix_reset_clears_state() {
        let mut trix = Trix::new("trix3", 3).unwrap();
        for _ in 0..9 {
            trix.update(&bar("100")).unwrap();
        }
        trix.reset();
        assert!(!trix.is_ready());
        assert_eq!(trix.update(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_trix_period_accessor() {
        let trix = Trix::new("trix5", 5).unwrap();
        assert_eq!(trix.period(), 5);
    }

    #[test]
    fn test_trix_name_accessor() {
        let trix = Trix::new("my_trix", 3).unwrap();
        assert_eq!(trix.name(), "my_trix");
    }
}
