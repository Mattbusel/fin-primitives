//! Psychological Line indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Psychological Line — percentage of up-bars (close > prior close) over `period` bars.
///
/// ```text
/// PL = count(close[i] > close[i-1], i in last period) / period * 100
/// ```
///
/// Range: 0 to 100. Readings above 70 suggest overbought; below 30 suggest oversold.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PsychologicalLine;
/// use fin_primitives::signals::Signal;
///
/// let pl = PsychologicalLine::new("pl12", 12).unwrap();
/// assert_eq!(pl.period(), 12);
/// ```
pub struct PsychologicalLine {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    up_flags: VecDeque<bool>,
}

impl PsychologicalLine {
    /// Constructs a new `PsychologicalLine`.
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
            prev_close: None,
            up_flags: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PsychologicalLine {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.up_flags.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let Some(prev) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        let is_up = bar.close > prev;
        self.prev_close = Some(bar.close);
        self.up_flags.push_back(is_up);
        if self.up_flags.len() > self.period {
            self.up_flags.pop_front();
        }

        if self.up_flags.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let count = self.up_flags.iter().filter(|&&b| b).count();
        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(count as u32 * 100) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.up_flags.clear();
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
    fn test_pl_invalid_period() {
        assert!(PsychologicalLine::new("pl", 0).is_err());
    }

    #[test]
    fn test_pl_first_bar_unavailable() {
        let mut pl = PsychologicalLine::new("pl", 3).unwrap();
        assert_eq!(pl.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pl_all_up() {
        let mut pl = PsychologicalLine::new("pl", 3).unwrap();
        pl.update_bar(&bar("100")).unwrap();
        pl.update_bar(&bar("101")).unwrap();
        pl.update_bar(&bar("102")).unwrap();
        let v = pl.update_bar(&bar("103")).unwrap();
        // All 3 bars in window are up
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pl_all_down() {
        let mut pl = PsychologicalLine::new("pl", 3).unwrap();
        pl.update_bar(&bar("103")).unwrap();
        pl.update_bar(&bar("102")).unwrap();
        pl.update_bar(&bar("101")).unwrap();
        let v = pl.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pl_mixed() {
        let mut pl = PsychologicalLine::new("pl", 4).unwrap();
        pl.update_bar(&bar("100")).unwrap();
        pl.update_bar(&bar("101")).unwrap(); // up
        pl.update_bar(&bar("100")).unwrap(); // down
        pl.update_bar(&bar("102")).unwrap(); // up
        let v = pl.update_bar(&bar("101")).unwrap(); // down; window: [up, down, up, down] = 2/4
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_pl_reset() {
        let mut pl = PsychologicalLine::new("pl", 2).unwrap();
        pl.update_bar(&bar("100")).unwrap();
        pl.update_bar(&bar("101")).unwrap();
        pl.update_bar(&bar("102")).unwrap();
        assert!(pl.is_ready());
        pl.reset();
        assert!(!pl.is_ready());
    }
}
