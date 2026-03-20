//! Price Acceleration indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Acceleration — the change in Rate of Change between successive bars.
///
/// ```text
/// ROC(n) = close[now] - close[now - n]
/// acceleration = ROC(n)[now] - ROC(n)[prev]
///              = (close[now] - close[now-n]) - (close[prev] - close[prev-n])
/// ```
///
/// Positive acceleration means momentum is increasing; negative means it is fading.
/// Requires `period + 2` bars before producing the first value.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceAcceleration;
/// use fin_primitives::signals::Signal;
///
/// let pa = PriceAcceleration::new("accel", 5).unwrap();
/// assert_eq!(pa.period(), 5);
/// ```
pub struct PriceAcceleration {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
    prev_roc: Option<Decimal>,
}

impl PriceAcceleration {
    /// Creates a new `PriceAcceleration`.
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
            history: VecDeque::with_capacity(period + 2),
            prev_roc: None,
        })
    }
}

impl Signal for PriceAcceleration {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);

        // Need period+1 bars to compute ROC, +1 more to compute its change
        if self.history.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // ROC = close[now] - close[now - period]
        let oldest = *self.history.front().unwrap();
        let roc = bar.close - oldest;

        // Trim to keep exactly period+1 entries (oldest needed for next ROC)
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }

        let result = match self.prev_roc {
            None => {
                self.prev_roc = Some(roc);
                SignalValue::Unavailable
            }
            Some(prev) => {
                let accel = roc - prev;
                self.prev_roc = Some(roc);
                SignalValue::Scalar(accel)
            }
        };

        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period + 1 && self.prev_roc.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
        self.prev_roc = None;
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
    fn test_price_acceleration_invalid_period() {
        assert!(PriceAcceleration::new("a", 0).is_err());
    }

    #[test]
    fn test_price_acceleration_unavailable_early() {
        let mut pa = PriceAcceleration::new("a", 2).unwrap();
        // Need period+2 = 4 bars before first scalar
        for _ in 0..3 {
            assert_eq!(pa.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_price_acceleration_constant_trend_zero() {
        // Constant price change of +1 per bar => ROC stays constant => acceleration = 0
        let mut pa = PriceAcceleration::new("a", 2).unwrap();
        let prices = ["100", "101", "102", "103", "104", "105"];
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = pa.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0), "constant trend => zero acceleration");
        } else {
            panic!("expected Scalar, got {last:?}");
        }
    }

    #[test]
    fn test_price_acceleration_accelerating_positive() {
        // Accelerating upward: gaps grow => positive acceleration
        let mut pa = PriceAcceleration::new("a", 1).unwrap();
        // ROC(1): bar2-bar1=1, bar3-bar2=2, bar4-bar3=3 => accel grows
        pa.update_bar(&bar("100")).unwrap(); // seed
        pa.update_bar(&bar("101")).unwrap(); // ROC=1, prev=None
        pa.update_bar(&bar("103")).unwrap(); // ROC=2, accel=2-1=1
        if let SignalValue::Scalar(v) = pa.update_bar(&bar("106")).unwrap() {
            // ROC=3, accel=3-2=1
            assert!(v > dec!(0), "accelerating should be positive: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_price_acceleration_reset() {
        let mut pa = PriceAcceleration::new("a", 2).unwrap();
        for p in &["100", "101", "102", "103", "104"] { pa.update_bar(&bar(p)).unwrap(); }
        assert!(pa.is_ready());
        pa.reset();
        assert!(!pa.is_ready());
        assert_eq!(pa.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
