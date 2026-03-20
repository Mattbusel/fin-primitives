//! New High Percentage — rolling fraction of bars that achieved a new N-bar high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// New High Percentage — fraction of bars in the last `period` bars where `close > max(prior N closes)`.
///
/// At each bar, checks how many of the last `period` closes exceeded the rolling maximum
/// of all prior closes within the window at the time of that bar:
/// - **Near 1.0**: price frequently breaking to new highs — strong uptrend.
/// - **Near 0.0**: price rarely reaching new highs — downtrend or consolidation.
///
/// Uses a simplified computation: counts bars where close is the running max at the time.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NewHighPct;
/// use fin_primitives::signals::Signal;
/// let nhp = NewHighPct::new("nhp_20", 20).unwrap();
/// assert_eq!(nhp.period(), 20);
/// ```
pub struct NewHighPct {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl NewHighPct {
    /// Constructs a new `NewHighPct`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for NewHighPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Count bars that were a new running high at their time
        let closes: Vec<Decimal> = self.closes.iter().copied().collect();
        let mut running_max = closes[0];
        let mut new_high_count = 0u32;

        for &c in &closes[1..] {
            if c > running_max {
                new_high_count += 1;
                running_max = c;
            }
        }

        let frac = Decimal::from(new_high_count)
            .checked_div(Decimal::from((self.period - 1) as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(frac))
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
    fn test_nhp_invalid_period() {
        assert!(NewHighPct::new("nhp", 0).is_err());
        assert!(NewHighPct::new("nhp", 1).is_err());
    }

    #[test]
    fn test_nhp_unavailable_before_period() {
        let mut s = NewHighPct::new("nhp", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_nhp_perfect_uptrend_gives_one() {
        // Every bar is a new high → frac = (period-1)/(period-1) = 1.0
        let mut s = NewHighPct::new("nhp", 4).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("103")).unwrap() {
            assert_eq!(v, dec!(1), "perfect uptrend should give 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nhp_flat_prices_give_zero() {
        let mut s = NewHighPct::new("nhp", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100")).unwrap(); }
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_nhp_in_range_zero_to_one() {
        let mut s = NewHighPct::new("nhp", 4).unwrap();
        let prices = ["100","105","100","102","108","101","103"];
        for p in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "value out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_nhp_reset() {
        let mut s = NewHighPct::new("nhp", 3).unwrap();
        for p in &["100","101","102"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
