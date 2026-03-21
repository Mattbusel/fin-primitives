//! Body Acceleration indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Acceleration — the average change in body size per bar over a rolling window.
///
/// ```text
/// body[i]     = |close[i] - open[i]|
/// accel[t]    = (body[t] - body[t-period]) / period
/// ```
///
/// - **Positive**: body size is expanding on average — conviction increasing.
/// - **Negative**: body size is contracting on average — momentum losing steam.
/// - **Near zero**: body size is stable.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyAcceleration;
/// use fin_primitives::signals::Signal;
/// let ba = BodyAcceleration::new("ba_10", 10).unwrap();
/// assert_eq!(ba.period(), 10);
/// ```
pub struct BodyAcceleration {
    name: String,
    period: usize,
    bodies: VecDeque<Decimal>,
}

impl BodyAcceleration {
    /// Constructs a new `BodyAcceleration`.
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
            bodies: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for BodyAcceleration {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bodies.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.body_size();
        self.bodies.push_back(body);
        if self.bodies.len() > self.period + 1 {
            self.bodies.pop_front();
        }
        if self.bodies.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let oldest = *self.bodies.front().unwrap();
        let newest = *self.bodies.back().unwrap();
        let accel = (newest - oldest)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(accel))
    }

    fn reset(&mut self) {
        self.bodies.clear();
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
        let hi = op.value().max(cp.value());
        let lo = op.value().min(cp.value());
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op,
            high: Price::new(hi).unwrap(),
            low: Price::new(lo).unwrap(),
            close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ba_invalid_period() {
        assert!(BodyAcceleration::new("ba", 0).is_err());
    }

    #[test]
    fn test_ba_unavailable_during_warmup() {
        let mut ba = BodyAcceleration::new("ba", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(ba.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ba.is_ready());
    }

    #[test]
    fn test_ba_constant_body_zero() {
        // Same body every bar → accel = 0
        let mut ba = BodyAcceleration::new("ba", 3).unwrap();
        for _ in 0..5 {
            ba.update_bar(&bar("100", "105")).unwrap(); // body=5
        }
        if let SignalValue::Scalar(v) = ba.update_bar(&bar("100", "105")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ba_expanding_body_positive() {
        // Bodies growing: 1, 2, 3, 4 → accel = (4-1)/3 > 0
        let mut ba = BodyAcceleration::new("ba", 3).unwrap();
        ba.update_bar(&bar("100", "101")).unwrap(); // body=1
        ba.update_bar(&bar("100", "102")).unwrap(); // body=2
        ba.update_bar(&bar("100", "103")).unwrap(); // body=3
        if let SignalValue::Scalar(v) = ba.update_bar(&bar("100", "104")).unwrap() {
            assert!(v > dec!(0), "expanding bodies → positive acceleration: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ba_contracting_body_negative() {
        // Bodies shrinking: 5, 4, 3, 2 → accel = (2-5)/3 < 0
        let mut ba = BodyAcceleration::new("ba", 3).unwrap();
        ba.update_bar(&bar("100", "105")).unwrap(); // body=5
        ba.update_bar(&bar("100", "104")).unwrap(); // body=4
        ba.update_bar(&bar("100", "103")).unwrap(); // body=3
        if let SignalValue::Scalar(v) = ba.update_bar(&bar("100", "102")).unwrap() {
            assert!(v < dec!(0), "contracting bodies → negative acceleration: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ba_reset() {
        let mut ba = BodyAcceleration::new("ba", 2).unwrap();
        for _ in 0..3 { ba.update_bar(&bar("100", "102")).unwrap(); }
        assert!(ba.is_ready());
        ba.reset();
        assert!(!ba.is_ready());
    }
}
