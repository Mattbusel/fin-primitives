//! Open-Close Balance indicator.
//!
//! Rolling directional body score: net body direction divided by total body
//! magnitude. Ranges from -1 (all bearish) to +1 (all bullish).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open-Close Balance — rolling `sum(close - open) / sum(|close - open|)`.
///
/// ```text
/// body[i]      = close[i] - open[i]   (signed)
/// abs_body[i]  = |body[i]|
///
/// OCB[t] = sum(body[t-period+1..t]) / sum(abs_body[t-period+1..t])
/// ```
///
/// - **+1.0**: all bars in the window closed above their open — pure bullish
///   body dominance.
/// - **-1.0**: all bars closed below their open — pure bearish dominance.
/// - **0.0**: net body movement is zero (balanced bulls and bears, or all doji).
///
/// Returns [`SignalValue::Unavailable`] when total absolute body is zero (all
/// doji bars), or until `period` bars are collected.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenCloseBalance;
/// use fin_primitives::signals::Signal;
/// let ocb = OpenCloseBalance::new("ocb_20", 20).unwrap();
/// assert_eq!(ocb.period(), 20);
/// ```
pub struct OpenCloseBalance {
    name: String,
    period: usize,
    bodies: VecDeque<Decimal>,
    abs_bodies: VecDeque<Decimal>,
    sum_body: Decimal,
    sum_abs: Decimal,
}

impl OpenCloseBalance {
    /// Constructs a new `OpenCloseBalance`.
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
            bodies: VecDeque::with_capacity(period),
            abs_bodies: VecDeque::with_capacity(period),
            sum_body: Decimal::ZERO,
            sum_abs: Decimal::ZERO,
        })
    }
}

impl Signal for OpenCloseBalance {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bodies.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.close - bar.open;
        let abs_body = body.abs();

        self.sum_body += body;
        self.sum_abs += abs_body;
        self.bodies.push_back(body);
        self.abs_bodies.push_back(abs_body);

        if self.bodies.len() > self.period {
            let old_body = self.bodies.pop_front().unwrap();
            let old_abs = self.abs_bodies.pop_front().unwrap();
            self.sum_body -= old_body;
            self.sum_abs -= old_abs;
        }

        if self.bodies.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.sum_abs.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let balance = self.sum_body
            .checked_div(self.sum_abs)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(balance))
    }

    fn reset(&mut self) {
        self.bodies.clear();
        self.abs_bodies.clear();
        self.sum_body = Decimal::ZERO;
        self.sum_abs = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp > op { cp } else { op };
        let low = if cp < op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ocb_invalid_period() {
        assert!(OpenCloseBalance::new("ocb", 0).is_err());
    }

    #[test]
    fn test_ocb_unavailable_during_warmup() {
        let mut ocb = OpenCloseBalance::new("ocb", 3).unwrap();
        assert_eq!(ocb.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ocb.update_bar(&bar("105", "108")).unwrap(), SignalValue::Unavailable);
        assert!(!ocb.is_ready());
    }

    #[test]
    fn test_ocb_all_bullish_one() {
        // All bars: close > open → sum(body) = sum(abs_body) → OCB = 1
        let mut ocb = OpenCloseBalance::new("ocb", 3).unwrap();
        ocb.update_bar(&bar("100", "105")).unwrap(); // body=5
        ocb.update_bar(&bar("105", "108")).unwrap(); // body=3
        if let SignalValue::Scalar(v) = ocb.update_bar(&bar("108", "112")).unwrap() {
            // body=4; sum=12, sum_abs=12 → 1
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocb_all_bearish_minus_one() {
        let mut ocb = OpenCloseBalance::new("ocb", 3).unwrap();
        ocb.update_bar(&bar("105", "100")).unwrap();
        ocb.update_bar(&bar("100", "97")).unwrap();
        if let SignalValue::Scalar(v) = ocb.update_bar(&bar("97", "93")).unwrap() {
            assert_eq!(v, dec!(-1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocb_balanced_zero() {
        // One bullish (+5) and one bearish (-5) body → sum = 0
        let mut ocb = OpenCloseBalance::new("ocb", 2).unwrap();
        ocb.update_bar(&bar("100", "105")).unwrap(); // +5
        if let SignalValue::Scalar(v) = ocb.update_bar(&bar("105", "100")).unwrap() {
            // -5; sum=0, sum_abs=10 → 0
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocb_all_doji_unavailable() {
        let mut ocb = OpenCloseBalance::new("ocb", 2).unwrap();
        ocb.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(ocb.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ocb_reset() {
        let mut ocb = OpenCloseBalance::new("ocb", 2).unwrap();
        ocb.update_bar(&bar("100", "105")).unwrap();
        ocb.update_bar(&bar("105", "110")).unwrap();
        assert!(ocb.is_ready());
        ocb.reset();
        assert!(!ocb.is_ready());
    }
}
