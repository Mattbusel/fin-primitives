//! Williams VIX Fix indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Williams VIX Fix (WVF) — a synthetic volatility/fear indicator.
///
/// ```text
/// WVF = (highest_close(period) - current_low) / highest_close(period) * 100
/// ```
///
/// Readings above the threshold (often 1.5-2x StdDev or a fixed level like 15)
/// suggest capitulation lows and potential reversals.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VixFix;
/// use fin_primitives::signals::Signal;
///
/// let vf = VixFix::new("vf22", 22).unwrap();
/// assert_eq!(vf.period(), 22);
/// ```
pub struct VixFix {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl VixFix {
    /// Constructs a new `VixFix`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VixFix {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let highest_close = self
            .closes
            .iter()
            .copied()
            .reduce(Decimal::max)
            .unwrap_or(bar.close);

        if highest_close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let wvf = (highest_close - bar.low)
            .checked_div(highest_close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(wvf))
    }

    fn reset(&mut self) {
        self.closes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vix_fix_invalid_period() {
        assert!(VixFix::new("vf", 0).is_err());
    }

    #[test]
    fn test_vix_fix_unavailable_before_period() {
        let mut vf = VixFix::new("vf", 3).unwrap();
        assert_eq!(vf.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!vf.is_ready());
    }

    #[test]
    fn test_vix_fix_zero_when_low_equals_highest_close() {
        // If low == highest_close, WVF = 0
        let mut vf = VixFix::new("vf", 2).unwrap();
        vf.update_bar(&bar("100", "90", "100")).unwrap();
        // highest_close = 100; this bar low = 100 => WVF = (100-100)/100*100 = 0
        let v = vf.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vix_fix_positive_fear() {
        // highest_close = 110, low = 80 => WVF = (110-80)/110*100 ~ 27.27
        let mut vf = VixFix::new("vf", 2).unwrap();
        vf.update_bar(&bar("115", "100", "110")).unwrap();
        let v = vf.update_bar(&bar("105", "80", "95")).unwrap();
        if let SignalValue::Scalar(wvf) = v {
            assert!(wvf > dec!(0), "expected positive WVF, got {wvf}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vix_fix_reset() {
        let mut vf = VixFix::new("vf", 2).unwrap();
        vf.update_bar(&bar("110", "90", "100")).unwrap();
        vf.update_bar(&bar("105", "95", "102")).unwrap();
        assert!(vf.is_ready());
        vf.reset();
        assert!(!vf.is_ready());
    }
}
