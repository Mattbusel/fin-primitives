//! Trend Intensity Index (TII).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Intensity Index — measures how consistently price closes above or below its midpoint SMA.
///
/// ```text
/// SMA[i]     = SMA(period, close)[i]
/// midpoint   = SMA of first ⌊period/2⌋ bars of the current window
/// SD_pos     = # of bars in period where close > midpoint_SMA
/// TII        = (SD_pos / period) × 100      range [0, 100]
/// ```
///
/// A simpler, widely-used formulation counts, within the last `period` bars, how many closes
/// sit above the `period`-bar SMA midpoint:
///
/// ```text
/// pos_count  = |{ close[i] > SMA(period)[i] }| for i in last `period` bars
/// TII        = (pos_count / period) × 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Tii;
/// use fin_primitives::signals::Signal;
///
/// let tii = Tii::new("tii14", 14).unwrap();
/// assert_eq!(tii.period(), 14);
/// ```
pub struct Tii {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl Tii {
    /// Creates a new `Tii` with the given lookback period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Tii {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma: Decimal = self.closes.iter().sum::<Decimal>()
            / Decimal::from(self.period as u32);

        let pos_count = self.closes.iter().filter(|&&c| c > sma).count();
        let tii = Decimal::from(pos_count as u32 * 100)
            / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(tii))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_tii_invalid_period() {
        assert!(Tii::new("t", 0).is_err());
    }

    #[test]
    fn test_tii_unavailable_before_period() {
        let mut tii = Tii::new("t", 5).unwrap();
        for _ in 0..4 {
            assert_eq!(tii.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!tii.is_ready());
    }

    #[test]
    fn test_tii_flat_price_is_50() {
        // All closes equal the SMA → no close is strictly above → TII = 0
        // (equal is not "above", so flat price → 0)
        let mut tii = Tii::new("t", 4).unwrap();
        for _ in 0..4 { tii.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = tii.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        }
    }

    #[test]
    fn test_tii_strong_uptrend_approaches_100() {
        // Period=4; feed 4 bars at 100 then 4 bars at 200 → last window all 200, SMA=200, TII=0.
        // Feed a mix: 3 low then 1 high: window [100,100,100,200], SMA=125, 1 above → TII=25.
        // For TII=75: need 3/4 above SMA. Feed many lows then 3 highs.
        let mut tii = Tii::new("t", 4).unwrap();
        // Start: 4 bars at 100
        for _ in 0..4 { tii.update_bar(&bar("100")).unwrap(); }
        // Now push 3 bars well above 100 → window = [100, 200, 200, 200], SMA=175, 3 above → TII=75
        tii.update_bar(&bar("200")).unwrap();
        tii.update_bar(&bar("200")).unwrap();
        if let SignalValue::Scalar(v) = tii.update_bar(&bar("200")).unwrap() {
            assert!(v >= dec!(50), "expected TII >= 50 with 3/4 above SMA, got {v}");
        }
    }

    #[test]
    fn test_tii_in_range_0_100() {
        let mut tii = Tii::new("t", 5).unwrap();
        let prices = ["100", "102", "99", "103", "101", "98", "104", "100", "102", "101"];
        for c in &prices {
            if let SignalValue::Scalar(v) = tii.update_bar(&bar(c)).unwrap() {
                assert!(v >= dec!(0), "TII below 0: {v}");
                assert!(v <= dec!(100), "TII above 100: {v}");
            }
        }
    }

    #[test]
    fn test_tii_reset() {
        let mut tii = Tii::new("t", 4).unwrap();
        for _ in 0..10 { tii.update_bar(&bar("100")).unwrap(); }
        assert!(tii.is_ready());
        tii.reset();
        assert!(!tii.is_ready());
    }
}
