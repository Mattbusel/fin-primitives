//! High-Low Squeeze indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Squeeze — ratio of the current bar's range to the maximum range seen
/// over the last `period` bars, expressed as a percentage.
///
/// ```text
/// range[i]   = high[i] - low[i]
/// max_range  = max(range[t-period+1 .. t])
/// squeeze    = range[t] / max_range × 100
/// ```
///
/// - **100%**: current bar has the widest range in the window (expansion, no squeeze).
/// - **Low value (e.g. < 20%)**: current bar is highly compressed relative to recent swings.
/// - **Rising toward 100%**: breakout from compression.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or
/// when the max range in the window is zero (all bars are doji).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowSqueeze;
/// use fin_primitives::signals::Signal;
/// let hls = HighLowSqueeze::new("hls_20", 20).unwrap();
/// assert_eq!(hls.period(), 20);
/// ```
pub struct HighLowSqueeze {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl HighLowSqueeze {
    /// Constructs a new `HighLowSqueeze`.
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
        })
    }
}

impl Signal for HighLowSqueeze {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let r = bar.range();
        self.ranges.push_back(r);
        if self.ranges.len() > self.period {
            self.ranges.pop_front();
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let max_range = self.ranges.iter().copied().fold(Decimal::ZERO, Decimal::max);
        if max_range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let squeeze = r
            .checked_div(max_range)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(squeeze))
    }

    fn reset(&mut self) {
        self.ranges.clear();
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
    fn test_hls_invalid_period() {
        assert!(HighLowSqueeze::new("hls", 0).is_err());
    }

    #[test]
    fn test_hls_unavailable_during_warmup() {
        let mut hls = HighLowSqueeze::new("hls", 3).unwrap();
        assert_eq!(hls.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(hls.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!hls.is_ready());
    }

    #[test]
    fn test_hls_uniform_ranges_100() {
        // All same range → current = max → squeeze = 100%
        let mut hls = HighLowSqueeze::new("hls", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..4 {
            last = hls.update_bar(&bar("110", "90")).unwrap(); // range=20
        }
        assert_eq!(last, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_hls_compressed_bar_low() {
        // Wide bars (range=100) then a narrow bar (range=2) → squeeze low
        let mut hls = HighLowSqueeze::new("hls", 3).unwrap();
        hls.update_bar(&bar("200", "100")).unwrap();
        hls.update_bar(&bar("200", "100")).unwrap();
        hls.update_bar(&bar("200", "100")).unwrap();
        if let SignalValue::Scalar(v) = hls.update_bar(&bar("102", "100")).unwrap() {
            assert!(v < dec!(10), "narrow bar vs wide window → low squeeze: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hls_widest_bar_is_100() {
        // Widest bar in window → 100%
        let mut hls = HighLowSqueeze::new("hls", 3).unwrap();
        hls.update_bar(&bar("105", "100")).unwrap();
        hls.update_bar(&bar("110", "100")).unwrap();
        hls.update_bar(&bar("120", "100")).unwrap();
        if let SignalValue::Scalar(v) = hls.update_bar(&bar("130", "100")).unwrap() {
            assert_eq!(v, dec!(100), "widest range bar → squeeze = 100%");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hls_reset() {
        let mut hls = HighLowSqueeze::new("hls", 3).unwrap();
        for _ in 0..3 { hls.update_bar(&bar("110", "90")).unwrap(); }
        assert!(hls.is_ready());
        hls.reset();
        assert!(!hls.is_ready());
    }
}
