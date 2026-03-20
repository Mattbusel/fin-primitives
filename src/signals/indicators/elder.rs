//! Elder Ray indicator (Bull Power / Bear Power).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Elder Ray: measures bull and bear power relative to an EMA.
///
/// ```text
/// Bull Power = high - EMA(period)
/// Bear Power = low  - EMA(period)
/// ```
///
/// This implementation outputs `bull_power + bear_power` as a single scalar,
/// which represents net directional pressure. Positive values indicate bulls
/// dominate; negative values indicate bears dominate.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ElderRay;
/// use fin_primitives::signals::Signal;
/// let er = ElderRay::new("elder_13", 13).unwrap();
/// assert_eq!(er.period(), 13);
/// ```
pub struct ElderRay {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    count: usize,
    multiplier: Decimal,
    seed_sum: Decimal,
}

impl ElderRay {
    /// Constructs a new `ElderRay` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let multiplier = Decimal::from(2u32)
            / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            ema: None,
            count: 0,
            multiplier,
            seed_sum: Decimal::ZERO,
        })
    }
}

impl Signal for ElderRay {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.count += 1;
        if self.count <= self.period {
            self.seed_sum += bar.close;
            if self.count == self.period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = self.seed_sum / Decimal::from(self.period as u32);
                self.ema = Some(seed);
                let bull = bar.high - seed;
                let bear = bar.low - seed;
                return Ok(SignalValue::Scalar(bull + bear));
            }
            return Ok(SignalValue::Unavailable);
        }
        let prev_ema = self.ema.unwrap_or(bar.close);
        let new_ema = bar.close * self.multiplier + prev_ema * (Decimal::ONE - self.multiplier);
        self.ema = Some(new_ema);
        let bull = bar.high - new_ema;
        let bear = bar.low - new_ema;
        Ok(SignalValue::Scalar(bull + bear))
    }

    fn is_ready(&self) -> bool {
        self.count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ema = None;
        self.count = 0;
        self.seed_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(high: Decimal, low: Decimal, close: Decimal) -> OhlcvBar {
        let p_high = Price::new(high).unwrap();
        let p_low = Price::new(low).unwrap();
        let p_close = Price::new(close).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p_close,
            high: p_high,
            low: p_low,
            close: p_close,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_elder_period_0_fails() {
        assert!(ElderRay::new("e", 0).is_err());
    }

    #[test]
    fn test_elder_unavailable_before_period() {
        let mut e = ElderRay::new("e3", 3).unwrap();
        assert_eq!(e.update_bar(&bar(dec!(11), dec!(9), dec!(10))).unwrap(), SignalValue::Unavailable);
        assert_eq!(e.update_bar(&bar(dec!(11), dec!(9), dec!(10))).unwrap(), SignalValue::Unavailable);
        assert!(!e.is_ready());
    }

    #[test]
    fn test_elder_flat_series_zero_net() {
        // Flat prices: high=close+1, low=close-1, EMA == close → bull=+1, bear=-1, net=0
        let mut e = ElderRay::new("e3", 3).unwrap();
        for _ in 0..3 {
            e.update_bar(&bar(dec!(11), dec!(9), dec!(10))).unwrap();
        }
        let result = e.update_bar(&bar(dec!(11), dec!(9), dec!(10))).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_elder_reset() {
        let mut e = ElderRay::new("e3", 3).unwrap();
        for _ in 0..3 {
            e.update_bar(&bar(dec!(11), dec!(9), dec!(10))).unwrap();
        }
        assert!(e.is_ready());
        e.reset();
        assert!(!e.is_ready());
    }
}
