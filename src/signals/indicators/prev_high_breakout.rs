//! Previous High Breakout indicator.
//!
//! Rolling fraction of bars where the high exceeded the prior bar's high,
//! measuring the frequency of upside breakout attempts.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Previous High Breakout — rolling fraction of bars where `high > prev_high`.
///
/// Each bar contributes a 1 (breakout) or 0 (no breakout) to a rolling window.
/// The result is the mean of that window, representing the frequency of upside
/// breakouts over the past `period` bars.
///
/// - **Near 1.0**: nearly every bar is making a new intrabar high — strong
///   upward momentum or a trending market.
/// - **Near 0.0**: highs are consistently failing to exceed prior highs —
///   distribution or downtrend.
/// - **~0.5**: random / choppy market structure.
///
/// Returns [`SignalValue::Unavailable`] until `period` breakout observations
/// have been collected (requires `period + 1` bars total).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PrevHighBreakout;
/// use fin_primitives::signals::Signal;
/// let phb = PrevHighBreakout::new("phb_20", 20).unwrap();
/// assert_eq!(phb.period(), 20);
/// ```
pub struct PrevHighBreakout {
    name: String,
    period: usize,
    window: VecDeque<u8>,
    prev_high: Option<Decimal>,
}

impl PrevHighBreakout {
    /// Constructs a new `PrevHighBreakout`.
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
            prev_high: None,
        })
    }
}

impl Signal for PrevHighBreakout {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let broke_out = if bar.high > ph { 1u8 } else { 0u8 };
            self.window.push_back(broke_out);
            if self.window.len() > self.period {
                self.window.pop_front();
            }
        }
        self.prev_high = Some(bar.high);

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
        self.prev_high = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new("1".parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_phb_invalid_period() {
        assert!(PrevHighBreakout::new("phb", 0).is_err());
    }

    #[test]
    fn test_phb_unavailable_during_warmup() {
        let mut phb = PrevHighBreakout::new("phb", 3).unwrap();
        phb.update_bar(&bar("100")).unwrap();
        phb.update_bar(&bar("102")).unwrap();
        phb.update_bar(&bar("104")).unwrap();
        assert!(!phb.is_ready());
    }

    #[test]
    fn test_phb_all_breakouts_one() {
        // Every bar exceeds prior high → fraction = 1
        let mut phb = PrevHighBreakout::new("phb", 3).unwrap();
        phb.update_bar(&bar("100")).unwrap();
        phb.update_bar(&bar("102")).unwrap();
        phb.update_bar(&bar("104")).unwrap();
        let v = phb.update_bar(&bar("106")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_phb_no_breakouts_zero() {
        // Every bar lower than prior → fraction = 0
        let mut phb = PrevHighBreakout::new("phb", 3).unwrap();
        phb.update_bar(&bar("110")).unwrap();
        phb.update_bar(&bar("108")).unwrap();
        phb.update_bar(&bar("106")).unwrap();
        let v = phb.update_bar(&bar("104")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_phb_mixed_fraction() {
        // 2 of 4 bars break out: 100→102(yes), 102→101(no), 101→103(yes), 103→102(no)
        // window(period=2): last 2 observations after 5th bar: [yes(103>101), no(102<103)]
        let mut phb = PrevHighBreakout::new("phb", 2).unwrap();
        phb.update_bar(&bar("100")).unwrap();
        phb.update_bar(&bar("102")).unwrap(); // yes
        phb.update_bar(&bar("101")).unwrap(); // no
        phb.update_bar(&bar("103")).unwrap(); // yes
        let v = phb.update_bar(&bar("102")).unwrap(); // no → window=[yes,no]
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_phb_reset() {
        let mut phb = PrevHighBreakout::new("phb", 2).unwrap();
        phb.update_bar(&bar("100")).unwrap();
        phb.update_bar(&bar("102")).unwrap();
        phb.update_bar(&bar("104")).unwrap();
        assert!(phb.is_ready());
        phb.reset();
        assert!(!phb.is_ready());
    }
}
