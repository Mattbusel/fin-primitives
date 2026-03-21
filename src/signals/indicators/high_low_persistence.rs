//! High-Low Persistence indicator.
//!
//! Measures whether the current bar's high and low stay within the prior bar's
//! high and low (inside) or extend beyond (outside), over a rolling window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Persistence — rolling fraction of bars where the bar's range is
/// fully contained within the prior bar's range (inside bars).
///
/// For each bar after the first:
/// ```text
/// inside = 1  if high[i] <= high[i-1] AND low[i] >= low[i-1]
///        = 0  otherwise
/// ```
///
/// - **Near 1.0**: the market is consistently forming inside bars — range
///   contraction, low conviction, and potential coiling before a breakout.
/// - **Near 0.0**: the market consistently breaks outside the prior range —
///   expansion and directional follow-through.
///
/// Returns [`SignalValue::Unavailable`] until `period` observations are
/// collected (`period + 1` bars total).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowPersistence;
/// use fin_primitives::signals::Signal;
/// let hlp = HighLowPersistence::new("hlp_20", 20).unwrap();
/// assert_eq!(hlp.period(), 20);
/// ```
pub struct HighLowPersistence {
    name: String,
    period: usize,
    flags: VecDeque<Decimal>,
    sum: Decimal,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
}

impl HighLowPersistence {
    /// Constructs a new `HighLowPersistence`.
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
            flags: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
            prev_high: None,
            prev_low: None,
        })
    }
}

impl Signal for HighLowPersistence {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.flags.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ph = self.prev_high;
        let pl = self.prev_low;
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        let (Some(prev_high), Some(prev_low)) = (ph, pl) else {
            return Ok(SignalValue::Unavailable);
        };

        let flag = if bar.high <= prev_high && bar.low >= prev_low {
            Decimal::ONE
        } else {
            Decimal::ZERO
        };

        self.sum += flag;
        self.flags.push_back(flag);
        if self.flags.len() > self.period {
            let removed = self.flags.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.flags.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.flags.clear();
        self.sum = Decimal::ZERO;
        self.prev_high = None;
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
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
    fn test_hlp_invalid_period() {
        assert!(HighLowPersistence::new("hlp", 0).is_err());
    }

    #[test]
    fn test_hlp_first_bar_unavailable() {
        let mut hlp = HighLowPersistence::new("hlp", 3).unwrap();
        assert_eq!(hlp.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!hlp.is_ready());
    }

    #[test]
    fn test_hlp_all_inside_bars_one() {
        // Every bar fits inside the prior → fraction = 1.0
        let mut hlp = HighLowPersistence::new("hlp", 3).unwrap();
        hlp.update_bar(&bar("120", "80")).unwrap();
        hlp.update_bar(&bar("115", "85")).unwrap();
        hlp.update_bar(&bar("112", "88")).unwrap();
        if let SignalValue::Scalar(v) = hlp.update_bar(&bar("110", "90")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hlp_no_inside_bars_zero() {
        // Every bar expands beyond prior — each has a strictly higher high
        // and strictly lower low than the previous bar.
        let mut hlp = HighLowPersistence::new("hlp", 3).unwrap();
        hlp.update_bar(&bar("100", "90")).unwrap();  // seed
        hlp.update_bar(&bar("105", "85")).unwrap();  // 105>100, 85<90 → not inside
        hlp.update_bar(&bar("110", "80")).unwrap();  // not inside
        hlp.update_bar(&bar("115", "75")).unwrap();  // not inside: window=[0,0,0]
        if let SignalValue::Scalar(v) = hlp.update_bar(&bar("120", "70")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hlp_reset() {
        let mut hlp = HighLowPersistence::new("hlp", 2).unwrap();
        hlp.update_bar(&bar("110", "90")).unwrap();
        hlp.update_bar(&bar("108", "92")).unwrap();
        hlp.update_bar(&bar("106", "94")).unwrap();
        assert!(hlp.is_ready());
        hlp.reset();
        assert!(!hlp.is_ready());
    }
}
