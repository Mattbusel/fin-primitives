//! Volatility Compression indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Compression — consecutive count of bars where the bar's range is below
/// the rolling average range over the last `period` bars.
///
/// Outputs:
/// - **+N**: N consecutive bars where range < rolling avg (compression building).
/// - **0**: current bar has range >= rolling avg (compression broken).
///
/// This is useful for detecting squeeze setups: prolonged compression often precedes
/// a volatility expansion breakout.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityCompression;
/// use fin_primitives::signals::Signal;
///
/// let vc = VolatilityCompression::new("vc", 14).unwrap();
/// assert_eq!(vc.period(), 14);
/// ```
pub struct VolatilityCompression {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
    sum: Decimal,
    streak: u32,
}

impl VolatilityCompression {
    /// Constructs a new `VolatilityCompression`.
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
            ranges: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
            streak: 0,
        })
    }

    /// Returns the current compression streak count.
    pub fn streak(&self) -> u32 {
        self.streak
    }
}

impl Signal for VolatilityCompression {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();

        self.sum += range;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            let removed = self.ranges.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if range < avg {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.ranges.clear();
        self.sum = Decimal::ZERO;
        self.streak = 0;
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
    fn test_vc_invalid_period() {
        assert!(VolatilityCompression::new("vc", 0).is_err());
    }

    #[test]
    fn test_vc_unavailable_during_warmup() {
        let mut vc = VolatilityCompression::new("vc", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(vc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vc.is_ready());
    }

    #[test]
    fn test_vc_uniform_ranges_zero() {
        // Equal ranges → avg = range → 0 bars below avg → streak stays 0
        let mut vc = VolatilityCompression::new("vc", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = vc.update_bar(&bar("110", "90")).unwrap(); // range=20 every bar
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vc_compression_builds() {
        // 3 wide bars, then narrow bars to build compression
        let mut vc = VolatilityCompression::new("vc", 3).unwrap();
        vc.update_bar(&bar("120", "80")).unwrap(); // range=40
        vc.update_bar(&bar("120", "80")).unwrap(); // range=40
        vc.update_bar(&bar("120", "80")).unwrap(); // range=40, now avg=40
        // narrow bars: range=5 < avg=40 → compression
        let r1 = vc.update_bar(&bar("105", "100")).unwrap(); // streak=1
        let r2 = vc.update_bar(&bar("105", "100")).unwrap(); // streak=2
        assert_eq!(r1, SignalValue::Scalar(dec!(1)));
        assert_eq!(r2, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_vc_reset() {
        let mut vc = VolatilityCompression::new("vc", 3).unwrap();
        for _ in 0..3 { vc.update_bar(&bar("110", "90")).unwrap(); }
        assert!(vc.is_ready());
        vc.reset();
        assert!(!vc.is_ready());
        assert_eq!(vc.streak(), 0);
    }
}
