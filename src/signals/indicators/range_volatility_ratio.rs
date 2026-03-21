//! Range Volatility Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Volatility Ratio — the current bar's range (`high - low`) divided by the
/// rolling average range over the last `period` bars.
///
/// ```text
/// avg_range  = mean(high[i] - low[i], i in [t-period+1, t-1])
/// output     = range[t] / avg_range
/// ```
///
/// - **> 1.0**: current bar's range is wider than average (volatility expansion).
/// - **< 1.0**: current bar's range is narrower than average (compression).
/// - **≈ 1.0**: range is close to normal.
///
/// Useful for detecting volatility expansion/contraction on a bar-by-bar basis
/// relative to the recent average range. Requires only `period + 1` bars (the
/// average is computed over the *previous* `period` bars, then the current bar
/// is compared against it).
///
/// Returns [`SignalValue::Unavailable`] until `period` prior bars have been seen,
/// or when the average range is zero (all prior bars are doji/flat).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeVolatilityRatio;
/// use fin_primitives::signals::Signal;
/// let rvr = RangeVolatilityRatio::new("rvr_14", 14).unwrap();
/// assert_eq!(rvr.period(), 14);
/// ```
pub struct RangeVolatilityRatio {
    name: String,
    period: usize,
    prior_ranges: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeVolatilityRatio {
    /// Constructs a new `RangeVolatilityRatio`.
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
            prior_ranges: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeVolatilityRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.prior_ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let current_range = bar.range();

        // Compute ratio against the average of PRIOR period bars
        let result = if self.prior_ranges.len() >= self.period {
            let avg = self.sum
                .checked_div(Decimal::from(self.period as u32))
                .ok_or(FinError::ArithmeticOverflow)?;
            if avg.is_zero() {
                SignalValue::Unavailable
            } else {
                let ratio = current_range
                    .checked_div(avg)
                    .ok_or(FinError::ArithmeticOverflow)?;
                SignalValue::Scalar(ratio)
            }
        } else {
            SignalValue::Unavailable
        };

        // Update rolling window with current bar's range
        self.sum += current_range;
        self.prior_ranges.push_back(current_range);
        if self.prior_ranges.len() > self.period {
            let removed = self.prior_ranges.pop_front().unwrap();
            self.sum -= removed;
        }

        Ok(result)
    }

    fn reset(&mut self) {
        self.prior_ranges.clear();
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rvr_invalid_period() {
        assert!(RangeVolatilityRatio::new("rvr", 0).is_err());
    }

    #[test]
    fn test_rvr_unavailable_during_warmup() {
        // period=3: need 4 bars total for first Scalar (window fills on bar 3, ratio on bar 4)
        let mut rvr = RangeVolatilityRatio::new("rvr", 3).unwrap();
        assert_eq!(rvr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!rvr.is_ready());
        assert_eq!(rvr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!rvr.is_ready());
        // After 3rd bar, window is full → is_ready() = true, but update still returned Unavailable
        assert_eq!(rvr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(rvr.is_ready()); // ready for the NEXT bar to produce a Scalar
    }

    #[test]
    fn test_rvr_uniform_ranges_one() {
        // All bars same range → ratio = 1.0
        let mut rvr = RangeVolatilityRatio::new("rvr", 3).unwrap();
        for _ in 0..4 {
            rvr.update_bar(&bar("110", "90")).unwrap(); // range=20
        }
        if let SignalValue::Scalar(v) = rvr.update_bar(&bar("110", "90")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rvr_wide_bar_above_one() {
        // Three narrow bars (range=5) then a wide bar (range=50) → ratio > 1
        let mut rvr = RangeVolatilityRatio::new("rvr", 3).unwrap();
        rvr.update_bar(&bar("105", "100")).unwrap();
        rvr.update_bar(&bar("105", "100")).unwrap();
        rvr.update_bar(&bar("105", "100")).unwrap();
        // avg range = 5, current range = 50 → ratio = 10
        if let SignalValue::Scalar(v) = rvr.update_bar(&bar("150", "100")).unwrap() {
            assert!(v > dec!(1), "wide bar → ratio > 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rvr_narrow_bar_below_one() {
        // Three wide bars (range=50) then a narrow bar (range=1) → ratio < 1
        let mut rvr = RangeVolatilityRatio::new("rvr", 3).unwrap();
        rvr.update_bar(&bar("150", "100")).unwrap();
        rvr.update_bar(&bar("150", "100")).unwrap();
        rvr.update_bar(&bar("150", "100")).unwrap();
        if let SignalValue::Scalar(v) = rvr.update_bar(&bar("101", "100")).unwrap() {
            assert!(v < dec!(1), "narrow bar → ratio < 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rvr_reset() {
        let mut rvr = RangeVolatilityRatio::new("rvr", 3).unwrap();
        for _ in 0..4 { rvr.update_bar(&bar("110", "90")).unwrap(); }
        assert!(rvr.is_ready());
        rvr.reset();
        assert!(!rvr.is_ready());
    }
}
