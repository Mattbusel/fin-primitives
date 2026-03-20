//! Centre of Gravity (COG) oscillator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Centre of Gravity Oscillator (Ehlers).
///
/// `COG = -Σ(close[i] × (i+1)) / Σ(close[i])` for `i = 0..period-1`
/// where `i=0` is the most recent bar.
///
/// Oscillates around zero; crossovers signal potential turning points.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Cog;
/// use fin_primitives::signals::Signal;
///
/// let c = Cog::new("cog10", 10).unwrap();
/// assert_eq!(c.period(), 10);
/// assert!(!c.is_ready());
/// ```
pub struct Cog {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl Cog {
    /// Constructs a new `Cog`.
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

impl Signal for Cog {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_front(bar.close); // most recent at front (index 0)
        if self.closes.len() > self.period {
            self.closes.pop_back();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mut num = Decimal::ZERO;
        let mut den = Decimal::ZERO;
        for (i, &c) in self.closes.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let weight = Decimal::from((i + 1) as u32);
            num += c * weight;
            den += c;
        }
        if den.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(-(num / den)))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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
    fn test_cog_invalid_period() {
        assert!(Cog::new("c", 0).is_err());
    }

    #[test]
    fn test_cog_unavailable_before_period() {
        let mut c = Cog::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!c.is_ready());
    }

    #[test]
    fn test_cog_flat_market() {
        // Flat market: all closes equal → COG = -Σ(p*(i+1))/Σ(p) = -(1+2+3)/3 * p/p = -2
        let mut c = Cog::new("c", 3).unwrap();
        c.update_bar(&bar("100")).unwrap();
        c.update_bar(&bar("100")).unwrap();
        let v = c.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-2)));
    }

    #[test]
    fn test_cog_reset() {
        let mut c = Cog::new("c", 3).unwrap();
        for _ in 0..3 { c.update_bar(&bar("100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }

    #[test]
    fn test_cog_period_and_name() {
        let c = Cog::new("my_cog", 10).unwrap();
        assert_eq!(c.period(), 10);
        assert_eq!(c.name(), "my_cog");
    }
}
