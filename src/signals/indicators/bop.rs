//! Balance of Power (BOP) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Balance of Power (BOP) indicator.
///
/// Measures buying vs selling pressure by comparing close-to-open movement
/// relative to the bar's range, then smoothed with an EMA:
///
/// ```text
/// raw_bop = (close - open) / (high - low)
/// BOP     = EMA(raw_bop, period)
/// ```
///
/// - Values near +1 indicate strong buying pressure.
/// - Values near -1 indicate strong selling pressure.
/// - When `high == low` (doji), the raw BOP for that bar is treated as 0.
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Bop;
/// use fin_primitives::signals::Signal;
/// let bop = Bop::new("bop14", 14).unwrap();
/// assert_eq!(bop.period(), 14);
/// ```
pub struct Bop {
    name: String,
    period: usize,
    multiplier: Decimal,
    ema: Option<Decimal>,
    bar_count: usize,
    /// Warm-up SMA accumulator before EMA kicks in.
    warmup: VecDeque<Decimal>,
}

impl Bop {
    /// Constructs a new `Bop` indicator.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let multiplier = Decimal::TWO
            .checked_div(Decimal::from(period as u32) + Decimal::ONE)
            .unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            ema: None,
            bar_count: 0,
            warmup: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Bop {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let raw = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.bar_count += 1;
        self.warmup.push_back(raw);
        if self.warmup.len() > self.period {
            self.warmup.pop_front();
        }

        if self.bar_count < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Seed EMA with SMA on first ready bar.
        let new_ema = match self.ema {
            None => {
                #[allow(clippy::cast_possible_truncation)]
                let sum: Decimal = self.warmup.iter().copied().sum();
                sum.checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?
            }
            Some(prev) => {
                let diff = raw - prev;
                prev + self
                    .multiplier
                    .checked_mul(diff)
                    .ok_or(FinError::ArithmeticOverflow)?
            }
        };
        self.ema = Some(new_ema);
        Ok(SignalValue::Scalar(new_ema))
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ema = None;
        self.bar_count = 0;
        self.warmup.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn ohlc(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bop_period_zero_fails() {
        assert!(Bop::new("bop", 0).is_err());
    }

    #[test]
    fn test_bop_unavailable_before_warmup() {
        let mut bop = Bop::new("bop3", 3).unwrap();
        assert_eq!(bop.update_bar(&ohlc("10", "15", "9", "12")).unwrap(), SignalValue::Unavailable);
        assert_eq!(bop.update_bar(&ohlc("12", "16", "11", "14")).unwrap(), SignalValue::Unavailable);
        assert!(!bop.is_ready());
    }

    #[test]
    fn test_bop_bullish_bar_positive() {
        // Close >> open → positive BOP
        let mut bop = Bop::new("bop1", 1).unwrap();
        let v = bop.update_bar(&ohlc("10", "20", "8", "18")).unwrap();
        // raw = (18-10)/(20-8) = 8/12 ≈ 0.667
        if let SignalValue::Scalar(val) = v {
            assert!(val > Decimal::ZERO);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bop_bearish_bar_negative() {
        // Close << open → negative BOP
        let mut bop = Bop::new("bop1", 1).unwrap();
        let v = bop.update_bar(&ohlc("18", "20", "8", "10")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val < Decimal::ZERO);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bop_doji_bar_zero() {
        // high == low → raw = 0
        let mut bop = Bop::new("bop1", 1).unwrap();
        let bar = OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(dec!(100)).unwrap(),
            high: Price::new(dec!(100)).unwrap(),
            low: Price::new(dec!(100)).unwrap(),
            close: Price::new(dec!(100)).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        };
        let v = bop.update_bar(&bar).unwrap();
        assert_eq!(v, SignalValue::Scalar(Decimal::ZERO));
    }

    #[test]
    fn test_bop_reset_clears_state() {
        let mut bop = Bop::new("bop1", 1).unwrap();
        bop.update_bar(&ohlc("10", "20", "8", "18")).unwrap();
        assert!(bop.is_ready());
        bop.reset();
        assert!(!bop.is_ready());
        assert_eq!(bop.update_bar(&ohlc("10", "20", "8", "18")).unwrap(), SignalValue::Unavailable);
    }
}
