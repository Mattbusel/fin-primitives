//! Swing Point Detector indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Swing Point Detector.
///
/// Identifies swing highs and swing lows by looking for bars where the high/low
/// is the extreme over a `lookback` window on each side. Uses a centered approach:
/// a swing high at bar T requires T's high to be the highest over the surrounding
/// `lookback` bars on each side.
///
/// Since centered detection requires future bars, this indicator uses a lagged
/// approach: when we have accumulated `lookback` bars after a candidate bar, we
/// can determine if it is a swing point.
///
/// Output:
/// - `+1.0`: swing high detected at the center of the current window.
/// - `−1.0`: swing low detected.
/// - `+2.0`: both swing high and swing low (rare, occurs on a spike/pin bar).
/// - `0.0`: not a swing point.
///
/// Returns `SignalValue::Unavailable` until `2 * lookback + 1` bars are seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SwingPointDetector;
/// use fin_primitives::signals::Signal;
/// let spd = SwingPointDetector::new("spd_5", 5).unwrap();
/// assert_eq!(spd.period(), 11); // 2*5+1
/// ```
pub struct SwingPointDetector {
    name: String,
    lookback: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl SwingPointDetector {
    /// Constructs a new `SwingPointDetector`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `lookback == 0`.
    pub fn new(name: impl Into<String>, lookback: usize) -> Result<Self, FinError> {
        if lookback == 0 {
            return Err(FinError::InvalidPeriod(lookback));
        }
        let window = 2 * lookback + 1;
        Ok(Self {
            name: name.into(),
            lookback,
            highs: VecDeque::with_capacity(window),
            lows: VecDeque::with_capacity(window),
        })
    }

    fn window(&self) -> usize {
        2 * self.lookback + 1
    }
}

impl Signal for SwingPointDetector {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);

        let window = self.window();
        if self.highs.len() > window {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < window {
            return Ok(SignalValue::Unavailable);
        }

        // Center bar is at index `lookback`
        let center_high = self.highs[self.lookback];
        let center_low = self.lows[self.lookback];

        let is_swing_high = self.highs.iter().enumerate().all(|(i, &h)| {
            i == self.lookback || h < center_high
        });
        let is_swing_low = self.lows.iter().enumerate().all(|(i, &l)| {
            i == self.lookback || l > center_low
        });

        let value = match (is_swing_high, is_swing_low) {
            (true, true) => Decimal::TWO,
            (true, false) => Decimal::ONE,
            (false, true) => Decimal::NEGATIVE_ONE,
            (false, false) => Decimal::ZERO,
        };
        Ok(SignalValue::Scalar(value))
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.window()
    }

    fn period(&self) -> usize {
        self.window()
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: hi,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_lookback_zero_fails() {
        assert!(matches!(SwingPointDetector::new("spd", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_period_is_two_lookback_plus_one() {
        let spd = SwingPointDetector::new("spd", 3).unwrap();
        assert_eq!(spd.period(), 7);
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut spd = SwingPointDetector::new("spd", 2).unwrap();
        assert_eq!(spd.update_bar(&bar("15", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_swing_high_detected() {
        let mut spd = SwingPointDetector::new("spd", 2).unwrap();
        // Feed: low, low, HIGH, low, low → swing high at center
        spd.update_bar(&bar("12", "10")).unwrap();
        spd.update_bar(&bar("13", "10")).unwrap();
        spd.update_bar(&bar("20", "10")).unwrap(); // center candidate
        spd.update_bar(&bar("14", "10")).unwrap();
        let v = spd.update_bar(&bar("13", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_swing_low_detected() {
        let mut spd = SwingPointDetector::new("spd", 2).unwrap();
        // Feed: high, high, LOW, high, high
        spd.update_bar(&bar("15", "12")).unwrap();
        spd.update_bar(&bar("15", "11")).unwrap();
        spd.update_bar(&bar("15", "5")).unwrap(); // center candidate
        spd.update_bar(&bar("15", "11")).unwrap();
        let v = spd.update_bar(&bar("15", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_reset() {
        let mut spd = SwingPointDetector::new("spd", 2).unwrap();
        for _ in 0..5 {
            spd.update_bar(&bar("12", "10")).unwrap();
        }
        assert!(spd.is_ready());
        spd.reset();
        assert!(!spd.is_ready());
    }
}
