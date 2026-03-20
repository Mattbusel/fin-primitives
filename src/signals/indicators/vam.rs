//! Volatility-Adjusted Momentum (VAM) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Volatility-Adjusted Momentum — momentum normalized by ATR.
///
/// ```text
/// momentum_t = close_t - close_{t-period}
/// ATR_t      = average true range over period
/// VAM        = momentum / ATR   (in ATR units)
/// ```
///
/// Values > 0 indicate upward momentum; < 0 indicate downward momentum.
/// Normalising by ATR makes the signal comparable across instruments with
/// different volatility levels.
///
/// Returns [`SignalValue::Unavailable`] until `2 × period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vam;
/// use fin_primitives::signals::Signal;
///
/// let v = Vam::new("vam", 14).unwrap();
/// assert_eq!(v.period(), 14);
/// ```
pub struct Vam {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    true_ranges: VecDeque<Decimal>,
}

impl Vam {
    /// Creates a new `Vam`.
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
            closes: VecDeque::with_capacity(period + 1),
            prev_close: None,
            true_ranges: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Vam {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.true_ranges.push_back(tr);
        if self.true_ranges.len() > self.period {
            self.true_ranges.pop_front();
        }

        if self.closes.len() < self.period + 1 || self.true_ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let momentum = self.closes.back().copied().unwrap()
            - self.closes.front().copied().unwrap();
        let atr = self.true_ranges.iter().sum::<Decimal>()
            / Decimal::from(self.period as u32);

        if atr.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        // Convert to f64 for division, return as Decimal
        let mom_f = momentum.to_f64().unwrap_or(0.0);
        let atr_f = atr.to_f64().unwrap_or(1.0);
        let vam = mom_f / atr_f;
        Ok(SignalValue::Scalar(Decimal::try_from(vam).unwrap_or(Decimal::ZERO)))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1 && self.true_ranges.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.prev_close = None;
        self.true_ranges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
    fn test_vam_invalid() {
        assert!(Vam::new("v", 0).is_err());
    }

    #[test]
    fn test_vam_unavailable_before_warmup() {
        let mut v = Vam::new("v", 3).unwrap();
        // Needs period+1=4 closes and period=3 TRs
        for _ in 0..3 {
            assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vam_flat_is_zero() {
        let mut v = Vam::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = v.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert!(val.abs() < dec!(0.001), "flat VAM should be ~0: {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vam_uptrend_positive() {
        let mut v = Vam::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..20usize {
            last = v.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert!(val > dec!(0), "uptrend VAM should be positive: {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vam_reset() {
        let mut v = Vam::new("v", 3).unwrap();
        for i in 0..10usize { v.update_bar(&bar(&(100+i).to_string())).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
