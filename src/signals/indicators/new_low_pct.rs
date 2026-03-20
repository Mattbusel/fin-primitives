//! New Low Percentage — rolling fraction of bars that achieved a new N-bar low.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// New Low Percentage — fraction of bars in the last `period` bars where `close < min(prior closes)`.
///
/// At each bar, tracks how many closes were a running new low within the window:
/// - **Near 1.0**: price frequently making new lows — strong downtrend.
/// - **Near 0.0**: price rarely hitting new lows — uptrend or consolidation.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NewLowPct;
/// use fin_primitives::signals::Signal;
/// let nlp = NewLowPct::new("nlp_20", 20).unwrap();
/// assert_eq!(nlp.period(), 20);
/// ```
pub struct NewLowPct {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl NewLowPct {
    /// Constructs a new `NewLowPct`.
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

impl Signal for NewLowPct {
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

        let closes: Vec<Decimal> = self.closes.iter().copied().collect();
        let mut running_min = closes[0];
        let mut new_low_count = 0u32;

        for &c in &closes[1..] {
            if c < running_min {
                new_low_count += 1;
                running_min = c;
            }
        }

        let frac = Decimal::from(new_low_count)
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
    fn test_nlp_invalid_period() {
        assert!(NewLowPct::new("nlp", 0).is_err());
        assert!(NewLowPct::new("nlp", 1).is_err());
    }

    #[test]
    fn test_nlp_unavailable_before_period() {
        let mut s = NewLowPct::new("nlp", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("99")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_nlp_perfect_downtrend_gives_one() {
        let mut s = NewLowPct::new("nlp", 4).unwrap();
        s.update_bar(&bar("103")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(1), "perfect downtrend should give 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nlp_uptrend_gives_zero() {
        let mut s = NewLowPct::new("nlp", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100")).unwrap(); }
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_nlp_in_range_zero_to_one() {
        let mut s = NewLowPct::new("nlp", 4).unwrap();
        for p in &["100","95","98","92","97","90"] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "value out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_nlp_reset() {
        let mut s = NewLowPct::new("nlp", 3).unwrap();
        for p in &["103","102","101"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
