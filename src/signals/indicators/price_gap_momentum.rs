//! Price Gap Momentum indicator.
//!
//! Tracks the EMA of the gap between each bar's open and the prior bar's close,
//! measuring the smoothed magnitude and direction of overnight / session gaps.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `open[t] − close[t-1]`.
///
/// Positive values indicate persistent upward gaps (bullish overnight sentiment);
/// negative values indicate persistent downward gaps (bearish overnight sentiment).
/// Near zero indicates gaps are random or balanced.
///
/// Returns a value starting from the second bar. `is_ready()` is `true` after 2 bars.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceGapMomentum;
/// use fin_primitives::signals::Signal;
///
/// let pgm = PriceGapMomentum::new("pgm", 10).unwrap();
/// assert_eq!(pgm.period(), 10);
/// assert!(!pgm.is_ready());
/// ```
pub struct PriceGapMomentum {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    ema: Option<Decimal>,
    k: Decimal,
    seen_bars: usize,
}

impl PriceGapMomentum {
    /// Constructs a new `PriceGapMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            ema: None,
            k,
            seen_bars: 0,
        })
    }
}

impl crate::signals::Signal for PriceGapMomentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.seen_bars >= 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.seen_bars += 1;

        let result = if let Some(pc) = self.prev_close {
            let gap = bar.open - pc;
            let ema = match self.ema {
                None => {
                    self.ema = Some(gap);
                    gap
                }
                Some(prev) => {
                    let next = gap * self.k + prev * (Decimal::ONE - self.k);
                    self.ema = Some(next);
                    next
                }
            };
            SignalValue::Scalar(ema)
        } else {
            SignalValue::Unavailable
        };

        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.ema = None;
        self.seen_bars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let (high, low) = if c >= o { (c, o) } else { (o, c) };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high, low, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pgm_invalid_period() {
        assert!(PriceGapMomentum::new("pgm", 0).is_err());
    }

    #[test]
    fn test_pgm_unavailable_first_bar() {
        let mut pgm = PriceGapMomentum::new("pgm", 5).unwrap();
        assert_eq!(pgm.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!pgm.is_ready());
    }

    #[test]
    fn test_pgm_ready_after_second_bar() {
        let mut pgm = PriceGapMomentum::new("pgm", 5).unwrap();
        pgm.update_bar(&bar("100", "105")).unwrap();
        pgm.update_bar(&bar("108", "112")).unwrap();
        assert!(pgm.is_ready());
    }

    #[test]
    fn test_pgm_gap_up_seeds_positive() {
        let mut pgm = PriceGapMomentum::new("pgm", 5).unwrap();
        pgm.update_bar(&bar("100", "105")).unwrap(); // close=105
        // open=110: gap = 110-105 = 5 → EMA seeds at 5
        let v = pgm.update_bar(&bar("110", "115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_pgm_gap_down_seeds_negative() {
        let mut pgm = PriceGapMomentum::new("pgm", 5).unwrap();
        pgm.update_bar(&bar("105", "100")).unwrap(); // close=100
        // open=95: gap = 95-100 = -5
        let v = pgm.update_bar(&bar("95", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-5)));
    }

    #[test]
    fn test_pgm_reset() {
        let mut pgm = PriceGapMomentum::new("pgm", 5).unwrap();
        pgm.update_bar(&bar("100", "105")).unwrap();
        pgm.update_bar(&bar("108", "112")).unwrap();
        assert!(pgm.is_ready());
        pgm.reset();
        assert!(!pgm.is_ready());
    }
}
