//! OBV Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// OBV Momentum — rate of change of On-Balance Volume over `period` bars.
///
/// ```text
/// OBV_t         = OBV_{t-1} + volume  if close > prev_close
///                 OBV_{t-1} - volume  if close < prev_close
///                 OBV_{t-1}            otherwise
///
/// obv_momentum  = (OBV_t - OBV_{t-period}) / |OBV_{t-period}| × 100
///               = 0 if OBV_{t-period} == 0
/// ```
///
/// Positive momentum indicates accumulation acceleration; negative indicates
/// distribution acceleration.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ObvMomentum;
/// use fin_primitives::signals::Signal;
///
/// let om = ObvMomentum::new("obvm", 10).unwrap();
/// assert_eq!(om.period(), 10);
/// ```
pub struct ObvMomentum {
    name: String,
    period: usize,
    obv: Decimal,
    prev_close: Option<Decimal>,
    obv_history: VecDeque<Decimal>,
}

impl ObvMomentum {
    /// Creates a new `ObvMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            obv: Decimal::ZERO,
            prev_close: None,
            obv_history: VecDeque::with_capacity(period + 1),
        })
    }

    /// Returns the current OBV value.
    pub fn obv(&self) -> Decimal { self.obv }
}

impl Signal for ObvMomentum {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let vol = bar.volume;

        match self.prev_close {
            None => {
                self.prev_close = Some(close);
                self.obv_history.push_back(self.obv);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => {
                if close > pc { self.obv += vol; }
                else if close < pc { self.obv -= vol; }
                self.prev_close = Some(close);
            }
        }

        self.obv_history.push_back(self.obv);
        if self.obv_history.len() > self.period + 1 {
            self.obv_history.pop_front();
        }

        if self.obv_history.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let obv_old = self.obv_history.front().copied().unwrap();
        let momentum = if obv_old.is_zero() {
            Decimal::ZERO
        } else {
            (self.obv - obv_old) / obv_old.abs() * Decimal::from(100u32)
        };

        Ok(SignalValue::Scalar(momentum))
    }

    fn is_ready(&self) -> bool {
        self.obv_history.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.obv = Decimal::ZERO;
        self.prev_close = None;
        self.obv_history.clear();
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
    fn test_obvm_invalid() {
        assert!(ObvMomentum::new("o", 0).is_err());
    }

    #[test]
    fn test_obvm_unavailable_before_warmup() {
        let mut om = ObvMomentum::new("o", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(om.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_obvm_produces_scalar() {
        let mut om = ObvMomentum::new("o", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..10usize {
            last = om.update_bar(&bar(&(100 + i).to_string(), "1000")).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_obvm_flat_price_zero() {
        // Flat price → OBV never changes → momentum = 0
        let mut om = ObvMomentum::new("o", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = om.update_bar(&bar("100", "1000")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0), "flat OBV → momentum = 0");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_obvm_reset() {
        let mut om = ObvMomentum::new("o", 3).unwrap();
        for i in 0..10usize { om.update_bar(&bar(&(100+i).to_string(), "1000")).unwrap(); }
        assert!(om.is_ready());
        om.reset();
        assert!(!om.is_ready());
        assert_eq!(om.obv(), dec!(0));
    }
}
