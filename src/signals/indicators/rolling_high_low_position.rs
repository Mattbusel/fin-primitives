//! Rolling High/Low Position — close position within the N-period high/low channel.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling High/Low Position — where the close falls within the N-period channel, in [0, 1].
///
/// Defined as `(close - period_low) / (period_high - period_low)`:
/// - **1.0**: close equals the N-period high (top of the channel).
/// - **0.0**: close equals the N-period low (bottom of the channel).
/// - **0.5**: close is at the midpoint of the channel.
///
/// Similar to [`crate::signals::indicators::StochasticK`] but uses a rolling high/low
/// window without a separate smoothing period.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when the
/// channel range is zero (all closes the same).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingHighLowPosition;
/// use fin_primitives::signals::Signal;
/// let rhlp = RollingHighLowPosition::new("rhlp_14", 14).unwrap();
/// assert_eq!(rhlp.period(), 14);
/// ```
pub struct RollingHighLowPosition {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl RollingHighLowPosition {
    /// Constructs a new `RollingHighLowPosition`.
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
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingHighLowPosition {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let range = period_high - period_low;

        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let position = (bar.close - period_low)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(position.clamp(Decimal::ZERO, Decimal::ONE)))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rhlp_invalid_period() {
        assert!(RollingHighLowPosition::new("rhlp", 0).is_err());
    }

    #[test]
    fn test_rhlp_unavailable_before_period() {
        let mut rhlp = RollingHighLowPosition::new("rhlp", 3).unwrap();
        assert_eq!(rhlp.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rhlp.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!rhlp.is_ready());
    }

    #[test]
    fn test_rhlp_close_at_high_gives_one() {
        let mut rhlp = RollingHighLowPosition::new("rhlp", 3).unwrap();
        for _ in 0..3 {
            rhlp.update_bar(&bar("110", "90", "100")).unwrap();
        }
        // close=110 = period_high → position=1
        let v = rhlp.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rhlp_close_at_low_gives_zero() {
        let mut rhlp = RollingHighLowPosition::new("rhlp", 3).unwrap();
        for _ in 0..3 {
            rhlp.update_bar(&bar("110", "90", "100")).unwrap();
        }
        // close=90 = period_low → position=0
        let v = rhlp.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rhlp_output_in_unit_interval() {
        let mut rhlp = RollingHighLowPosition::new("rhlp", 5).unwrap();
        let bars = [
            bar("105", "95", "100"),
            bar("107", "93", "104"),
            bar("103", "97", "98"),
            bar("108", "92", "103"),
            bar("106", "94", "101"),
            bar("104", "96", "100"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = rhlp.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "position must be >= 0: {v}");
                assert!(v <= dec!(1), "position must be <= 1: {v}");
            }
        }
    }

    #[test]
    fn test_rhlp_flat_channel_unavailable() {
        let mut rhlp = RollingHighLowPosition::new("rhlp", 3).unwrap();
        for _ in 0..3 {
            rhlp.update_bar(&bar("100", "100", "100")).unwrap();
        }
        let v = rhlp.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rhlp_reset() {
        let mut rhlp = RollingHighLowPosition::new("rhlp", 2).unwrap();
        rhlp.update_bar(&bar("110", "90", "100")).unwrap();
        rhlp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(rhlp.is_ready());
        rhlp.reset();
        assert!(!rhlp.is_ready());
    }

    #[test]
    fn test_rhlp_period_and_name() {
        let rhlp = RollingHighLowPosition::new("my_rhlp", 14).unwrap();
        assert_eq!(rhlp.period(), 14);
        assert_eq!(rhlp.name(), "my_rhlp");
    }
}
