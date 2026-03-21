//! Lower Low Rate indicator.
//!
//! Rolling fraction of bars where the low was below the prior bar's low,
//! measuring the frequency of downside probing.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Lower Low Rate — rolling fraction of bars where `low < prev_low`.
///
/// Each bar contributes a 1 (lower low) or 0 (not lower) to a rolling window.
/// The result is the mean of that window over `period` observations.
///
/// - **Near 1.0**: nearly every bar undercuts the prior low — strong downward
///   pressure or a trending bear market.
/// - **Near 0.0**: lows are consistently holding — support or uptrend.
/// - **~0.5**: random / choppy range structure.
///
/// Complement to `PrevHighBreakout` for measuring downside pressure.
///
/// Returns [`SignalValue::Unavailable`] until `period` observations have been
/// collected (requires `period + 1` bars total).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowerLowRate;
/// use fin_primitives::signals::Signal;
/// let llr = LowerLowRate::new("llr_20", 20).unwrap();
/// assert_eq!(llr.period(), 20);
/// ```
pub struct LowerLowRate {
    name: String,
    period: usize,
    window: VecDeque<u8>,
    prev_low: Option<Decimal>,
}

impl LowerLowRate {
    /// Constructs a new `LowerLowRate`.
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
            window: VecDeque::with_capacity(period),
            prev_low: None,
        })
    }
}

impl Signal for LowerLowRate {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pl) = self.prev_low {
            let lower = if bar.low < pl { 1u8 } else { 0u8 };
            self.window.push_back(lower);
            if self.window.len() > self.period {
                self.window.pop_front();
            }
        }
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let count = self.window.iter().map(|&x| u32::from(x)).sum::<u32>();
        #[allow(clippy::cast_possible_truncation)]
        let frac = Decimal::from(count) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(frac))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.prev_low = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(l: &str) -> OhlcvBar {
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let hp = Price::new("999".parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: lp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_llr_invalid_period() {
        assert!(LowerLowRate::new("llr", 0).is_err());
    }

    #[test]
    fn test_llr_unavailable_during_warmup() {
        let mut llr = LowerLowRate::new("llr", 3).unwrap();
        llr.update_bar(&bar("100")).unwrap();
        llr.update_bar(&bar("98")).unwrap();
        llr.update_bar(&bar("96")).unwrap();
        assert!(!llr.is_ready());
    }

    #[test]
    fn test_llr_all_lower_lows_one() {
        let mut llr = LowerLowRate::new("llr", 3).unwrap();
        llr.update_bar(&bar("100")).unwrap();
        llr.update_bar(&bar("98")).unwrap();
        llr.update_bar(&bar("96")).unwrap();
        let v = llr.update_bar(&bar("94")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_llr_no_lower_lows_zero() {
        let mut llr = LowerLowRate::new("llr", 3).unwrap();
        llr.update_bar(&bar("90")).unwrap();
        llr.update_bar(&bar("92")).unwrap();
        llr.update_bar(&bar("94")).unwrap();
        let v = llr.update_bar(&bar("96")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_llr_mixed() {
        // window period=2: bar5(low=92<94 → yes), bar6(low=93>92 → no) → 0.5
        let mut llr = LowerLowRate::new("llr", 2).unwrap();
        llr.update_bar(&bar("100")).unwrap();
        llr.update_bar(&bar("98")).unwrap();
        llr.update_bar(&bar("96")).unwrap(); // yes
        llr.update_bar(&bar("94")).unwrap(); // yes → window=[yes,yes]=1
        let v1 = llr.update_bar(&bar("95")).unwrap(); // no → window=[yes,no]=0.5
        assert_eq!(v1, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_llr_reset() {
        let mut llr = LowerLowRate::new("llr", 2).unwrap();
        llr.update_bar(&bar("100")).unwrap();
        llr.update_bar(&bar("98")).unwrap();
        llr.update_bar(&bar("96")).unwrap();
        assert!(llr.is_ready());
        llr.reset();
        assert!(!llr.is_ready());
    }
}
