//! High-Low Percentage Range — current bar's range as a percentage of close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Percentage Range — rolling SMA of `(high - low) / close * 100`.
///
/// Normalizes the bar range by the closing price, giving a dimensionless volatility measure:
/// - **High values**: large intrabar swings relative to price — high volatility.
/// - **Low values**: tight ranges relative to price — compressed, low-risk environment.
///
/// Returns the `period`-bar SMA of each bar's HL% range.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowPctRange;
/// use fin_primitives::signals::Signal;
/// let hlp = HighLowPctRange::new("hlp_14", 14).unwrap();
/// assert_eq!(hlp.period(), 14);
/// ```
pub struct HighLowPctRange {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighLowPctRange {
    /// Constructs a new `HighLowPctRange`.
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
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for HighLowPctRange {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let hl_pct = if bar.close.is_zero() {
            Decimal::ZERO
        } else {
            (bar.high - bar.low)
                .checked_div(bar.close)
                .ok_or(FinError::ArithmeticOverflow)?
                * Decimal::ONE_HUNDRED
        };

        self.sum += hl_pct;
        self.window.push_back(hl_pct);

        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(avg.max(Decimal::ZERO)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
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
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hlp_invalid_period() {
        assert!(HighLowPctRange::new("hlp", 0).is_err());
    }

    #[test]
    fn test_hlp_unavailable_before_period() {
        let mut s = HighLowPctRange::new("hlp", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_hlp_known_value() {
        // H=110, L=90, C=100 → HL%=20/100*100=20%
        let mut s = HighLowPctRange::new("hlp", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","100")).unwrap() {
            assert!((v - dec!(20)).abs() < dec!(0.001), "HL%=20 for 10% range bar: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hlp_non_negative() {
        let mut s = HighLowPctRange::new("hlp", 3).unwrap();
        for (h,l,c) in &[("110","90","100"),("115","85","98"),("108","92","105"),("112","88","100")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(h,l,c)).unwrap() {
                assert!(v >= dec!(0), "HL% range must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_hlp_reset() {
        let mut s = HighLowPctRange::new("hlp", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
