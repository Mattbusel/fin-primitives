//! Price Velocity indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Velocity — rate of change expressed as price units per bar.
///
/// ```text
/// PriceVelocity = (close[now] - close[now - period]) / period
/// ```
///
/// Unlike ROC (which is percentage-based), Price Velocity is in raw price units.
/// Useful when absolute price movement per bar matters (e.g., for stop-loss sizing).
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceVelocity;
/// use fin_primitives::signals::Signal;
///
/// let pv = PriceVelocity::new("pv10", 10).unwrap();
/// assert_eq!(pv.period(), 10);
/// ```
pub struct PriceVelocity {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl PriceVelocity {
    /// Constructs a new `PriceVelocity`.
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
            history: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for PriceVelocity {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period + 1
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }
        if self.history.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let old = *self.history.front().unwrap();
        #[allow(clippy::cast_possible_truncation)]
        let velocity = (bar.close - old)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(velocity))
    }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_pv_invalid_period() {
        assert!(PriceVelocity::new("pv", 0).is_err());
    }

    #[test]
    fn test_pv_unavailable_before_period() {
        let mut pv = PriceVelocity::new("pv", 3).unwrap();
        assert_eq!(pv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(pv.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!pv.is_ready());
    }

    #[test]
    fn test_pv_constant_price_zero_velocity() {
        let mut pv = PriceVelocity::new("pv", 3).unwrap();
        for _ in 0..4 {
            pv.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(pv.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pv_linear_rise_velocity_one() {
        // [100, 101, 102, 103]: pv(3) = (103-100)/3 = 1
        let mut pv = PriceVelocity::new("pv", 3).unwrap();
        pv.update_bar(&bar("100")).unwrap();
        pv.update_bar(&bar("101")).unwrap();
        pv.update_bar(&bar("102")).unwrap();
        let v = pv.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_pv_negative_velocity() {
        // [103, 102, 101, 100]: pv(3) = (100-103)/3 = -1
        let mut pv = PriceVelocity::new("pv", 3).unwrap();
        pv.update_bar(&bar("103")).unwrap();
        pv.update_bar(&bar("102")).unwrap();
        pv.update_bar(&bar("101")).unwrap();
        let v = pv.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_pv_reset() {
        let mut pv = PriceVelocity::new("pv", 2).unwrap();
        pv.update_bar(&bar("100")).unwrap();
        pv.update_bar(&bar("101")).unwrap();
        pv.update_bar(&bar("102")).unwrap();
        assert!(pv.is_ready());
        pv.reset();
        assert!(!pv.is_ready());
    }
}
