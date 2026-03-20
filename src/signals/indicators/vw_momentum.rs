//! Volume-Weighted Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted Momentum — measures the average price change per unit of volume
/// over a rolling window, giving more weight to high-volume moves.
///
/// ```text
/// vwm = Σ[(close[i] - close[i-1]) × volume[i]] / Σ[volume[i]]  over period bars
/// ```
///
/// Positive values indicate net buying pressure; negative values indicate selling pressure.
/// Zero volume bars contribute zero to both numerator and denominator.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// or total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VwMomentum;
/// use fin_primitives::signals::Signal;
///
/// let v = VwMomentum::new("vwm", 10).unwrap();
/// assert_eq!(v.period(), 10);
/// ```
pub struct VwMomentum {
    name: String,
    period: usize,
    // Each entry: (price_change × volume, volume)
    history: VecDeque<(Decimal, Decimal)>,
    prev_close: Option<Decimal>,
}

impl VwMomentum {
    /// Creates a new `VwMomentum`.
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
            history: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for VwMomentum {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let vol = bar.volume;

        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(close);

        let weighted_change = (close - prev) * vol;
        self.history.push_back((weighted_change, vol));
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_vol: Decimal = self.history.iter().map(|(_, v)| v).sum();
        if total_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let total_weighted: Decimal = self.history.iter().map(|(wc, _)| wc).sum();
        let vwm = total_weighted
            .checked_div(total_vol)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(vwm))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwm_invalid_period() {
        assert!(VwMomentum::new("v", 0).is_err());
    }

    #[test]
    fn test_vwm_unavailable_early() {
        let mut v = VwMomentum::new("v", 3).unwrap();
        assert_eq!(v.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(v.update_bar(&bar("101", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vwm_uptrend_positive() {
        let mut v = VwMomentum::new("v", 3).unwrap();
        v.update_bar(&bar("100", "1000")).unwrap();
        v.update_bar(&bar("101", "1000")).unwrap();
        v.update_bar(&bar("102", "1000")).unwrap();
        if let SignalValue::Scalar(val) = v.update_bar(&bar("103", "1000")).unwrap() {
            assert!(val > dec!(0), "uptrend should give positive VWM: {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vwm_downtrend_negative() {
        let mut v = VwMomentum::new("v", 3).unwrap();
        v.update_bar(&bar("103", "1000")).unwrap();
        v.update_bar(&bar("102", "1000")).unwrap();
        v.update_bar(&bar("101", "1000")).unwrap();
        if let SignalValue::Scalar(val) = v.update_bar(&bar("100", "1000")).unwrap() {
            assert!(val < dec!(0), "downtrend should give negative VWM: {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vwm_reset() {
        let mut v = VwMomentum::new("v", 3).unwrap();
        for p in &["100", "101", "102", "103"] { v.update_bar(&bar(p, "100")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
        assert_eq!(v.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }
}
