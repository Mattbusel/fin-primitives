//! Price Gap Frequency indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Gap Frequency — the fraction of bars over the last `period` bars where
/// the absolute overnight gap exceeds a minimum threshold expressed as a
/// percentage of the prior close.
///
/// ```text
/// gap_pct = |open - prev_close| / prev_close * 100
/// frequency = count(gap_pct > threshold) / period
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` gaps have been observed.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceGapFrequency;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let pgf = PriceGapFrequency::new("pgf", 20, dec!(0.5)).unwrap();
/// assert_eq!(pgf.period(), 20);
/// ```
pub struct PriceGapFrequency {
    name: String,
    period: usize,
    threshold: Decimal,
    prev_close: Option<Decimal>,
    flags: VecDeque<bool>,
}

impl PriceGapFrequency {
    /// Constructs a new `PriceGapFrequency`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `threshold <= 0`.
    pub fn new(name: impl Into<String>, period: usize, threshold: Decimal) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if threshold <= Decimal::ZERO {
            return Err(FinError::InvalidInput("threshold must be > 0".into()));
        }
        Ok(Self {
            name: name.into(),
            period,
            threshold,
            prev_close: None,
            flags: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceGapFrequency {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.flags.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let gap = if pc.is_zero() {
                    Decimal::ZERO
                } else {
                    (bar.open - pc).abs() / pc * Decimal::ONE_HUNDRED
                };
                let exceeded = gap > self.threshold;
                self.flags.push_back(exceeded);
                if self.flags.len() > self.period { self.flags.pop_front(); }
                if self.flags.len() < self.period {
                    SignalValue::Unavailable
                } else {
                    let count = self.flags.iter().filter(|&&f| f).count();
                    SignalValue::Scalar(Decimal::from(count as u32) / Decimal::from(self.period as u32))
                }
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.flags.clear();
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp, low: op, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pgf_invalid_period() {
        assert!(PriceGapFrequency::new("pgf", 0, dec!(0.5)).is_err());
        assert!(PriceGapFrequency::new("pgf", 5, dec!(0)).is_err());
    }

    #[test]
    fn test_pgf_unavailable_before_warm_up() {
        let mut pgf = PriceGapFrequency::new("pgf", 3, dec!(0.5)).unwrap();
        pgf.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(pgf.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pgf_no_gaps() {
        let mut pgf = PriceGapFrequency::new("pgf", 3, dec!(0.5)).unwrap();
        pgf.update_bar(&bar("100", "100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = pgf.update_bar(&bar("100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pgf_all_gaps() {
        // Each bar opens 2% above prev close, threshold=1%
        let mut pgf = PriceGapFrequency::new("pgf", 3, dec!(1)).unwrap();
        pgf.update_bar(&bar("100", "100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = pgf.update_bar(&bar("102", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_pgf_reset() {
        let mut pgf = PriceGapFrequency::new("pgf", 3, dec!(0.5)).unwrap();
        for _ in 0..4 { pgf.update_bar(&bar("100", "100")).unwrap(); }
        assert!(pgf.is_ready());
        pgf.reset();
        assert!(!pgf.is_ready());
    }
}
