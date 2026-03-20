//! Chaikin Volatility indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Chaikin Volatility (CV) — measures the rate of change of the EMA of the H-L range.
///
/// ```text
/// HL_range = high - low
/// EMA_now   = EMA(HL_range, period)
/// EMA_prev  = EMA(HL_range, period) from `period` bars ago
/// CV = (EMA_now - EMA_prev) / EMA_prev * 100
/// ```
///
/// Positive values indicate expanding volatility; negative values indicate contracting.
/// Returns [`SignalValue::Unavailable`] until `2 * period` bars have been seen
/// (one full period to seed the EMA, then another to have a reference EMA from `period` bars ago).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChaikinVolatility;
/// use fin_primitives::signals::Signal;
///
/// let cv = ChaikinVolatility::new("cv10", 10).unwrap();
/// assert_eq!(cv.period(), 10);
/// ```
pub struct ChaikinVolatility {
    name: String,
    period: usize,
    alpha: Decimal,
    current_ema: Option<Decimal>,
    /// Rolling window of past EMA values so we can look back `period` bars.
    ema_history: VecDeque<Decimal>,
}

impl ChaikinVolatility {
    /// Constructs a new `ChaikinVolatility`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let alpha = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            alpha,
            current_ema: None,
            ema_history: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for ChaikinVolatility {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ema_history.len() > self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let hl = bar.high - bar.low;
        let ema = match self.current_ema {
            None => hl,
            Some(prev) => prev + self.alpha * (hl - prev),
        };
        self.current_ema = Some(ema);

        self.ema_history.push_back(ema);
        if self.ema_history.len() > self.period + 1 {
            self.ema_history.pop_front();
        }

        if self.ema_history.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        // ema_prev is from `period` bars ago (front of the window)
        let ema_prev = *self.ema_history.front().unwrap();
        if ema_prev.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let cv = (ema - ema_prev)
            .checked_div(ema_prev)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(cv))
    }

    fn reset(&mut self) {
        self.current_ema = None;
        self.ema_history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mid = Price::new(((hp.value() + lp.value()) / Decimal::TWO).max(Decimal::ONE)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mid, high: hp, low: lp, close: mid,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cv_invalid_period() {
        assert!(ChaikinVolatility::new("cv", 0).is_err());
    }

    #[test]
    fn test_cv_unavailable_before_ready() {
        let mut cv = ChaikinVolatility::new("cv", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(cv.update_bar(&bar("105", "95")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!cv.is_ready());
    }

    #[test]
    fn test_cv_constant_range_zero_change() {
        let mut cv = ChaikinVolatility::new("cv", 2).unwrap();
        // Feed 3 bars with identical range of 10
        for _ in 0..3 {
            cv.update_bar(&bar("105", "95")).unwrap();
        }
        assert!(cv.is_ready());
        if let SignalValue::Scalar(v) = cv.update_bar(&bar("105", "95")).unwrap() {
            // Constant range: EMA doesn't change => CV = 0
            assert!(v.abs() < dec!(0.001), "expected near-zero CV for constant range, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cv_expanding_range_positive() {
        let mut cv = ChaikinVolatility::new("cv", 2).unwrap();
        // Seed with tight range
        for _ in 0..3 {
            cv.update_bar(&bar("101", "99")).unwrap();
        }
        // Wide range bar should push CV positive
        let v = cv.update_bar(&bar("120", "80")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val > dec!(0), "expected positive CV for expanding range, got {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cv_reset() {
        let mut cv = ChaikinVolatility::new("cv", 2).unwrap();
        for _ in 0..4 {
            cv.update_bar(&bar("105", "95")).unwrap();
        }
        assert!(cv.is_ready());
        cv.reset();
        assert!(!cv.is_ready());
    }
}
