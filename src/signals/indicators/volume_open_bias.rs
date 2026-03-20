//! Volume Open Bias indicator -- fraction of rolling volume from gap-up opens.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Open Bias -- the fraction of total rolling volume occurring on bars that
/// open above the previous close (gap-up bars), scaled to 0-100%.
///
/// A high value indicates most volume flows into gap-up sessions (bullish bias).
/// A low value suggests volume predominantly occurs on flat or gap-down opens.
///
/// ```text
/// gap_vol[t] = volume[t] if open[t] > prev_close[t-1], else 0
/// bias[t]    = sum(gap_vol, period) / sum(volume, period) * 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars with a valid prior close
/// have been seen, or if total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeOpenBias;
/// use fin_primitives::signals::Signal;
/// let vob = VolumeOpenBias::new("vob", 10).unwrap();
/// assert_eq!(vob.period(), 10);
/// ```
pub struct VolumeOpenBias {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    gap_window: VecDeque<Decimal>,
    vol_window: VecDeque<Decimal>,
    gap_sum: Decimal,
    vol_sum: Decimal,
}

impl VolumeOpenBias {
    /// Constructs a new `VolumeOpenBias`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            gap_window: VecDeque::with_capacity(period),
            vol_window: VecDeque::with_capacity(period),
            gap_sum: Decimal::ZERO,
            vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeOpenBias {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gap_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let gap_vol = match self.prev_close {
            Some(pc) if bar.open > pc => bar.volume,
            _ => Decimal::ZERO,
        };
        self.prev_close = Some(bar.close);

        self.gap_window.push_back(gap_vol);
        self.vol_window.push_back(bar.volume);
        self.gap_sum += gap_vol;
        self.vol_sum += bar.volume;
        if self.gap_window.len() > self.period {
            if let Some(old_g) = self.gap_window.pop_front() { self.gap_sum -= old_g; }
            if let Some(old_v) = self.vol_window.pop_front() { self.vol_sum -= old_v; }
        }
        if self.gap_window.len() < self.period { return Ok(SignalValue::Unavailable); }
        if self.vol_sum.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.gap_sum / self.vol_sum * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gap_window.clear();
        self.vol_window.clear();
        self.gap_sum = Decimal::ZERO;
        self.vol_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vob_period_0_error() { assert!(VolumeOpenBias::new("vob", 0).is_err()); }

    #[test]
    fn test_vob_unavailable_before_period() {
        let mut vob = VolumeOpenBias::new("vob", 3).unwrap();
        assert_eq!(vob.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vob_all_gap_up_is_100() {
        let mut vob = VolumeOpenBias::new("vob", 3).unwrap();
        // Bar 1: sets prev_close=102. No comparison yet, gap_vol=0.
        vob.update_bar(&bar("100", "102", "1000")).unwrap();
        // Bar 2: open=105 > prev_close=102 -> gap_vol=1000
        vob.update_bar(&bar("105", "107", "1000")).unwrap();
        // Bar 3: open=110 > prev_close=107 -> gap_vol=1000
        vob.update_bar(&bar("110", "112", "1000")).unwrap();
        // Bar 4 (window slides): open=115 > prev_close=112 -> gap_vol=1000
        // window=[1000,1000,1000], vol=[1000,1000,1000] -> 100%
        let v = vob.update_bar(&bar("115", "117", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_vob_no_gap_up_is_0() {
        let mut vob = VolumeOpenBias::new("vob", 3).unwrap();
        // Each bar opens at or below previous close -> no gap-up
        vob.update_bar(&bar("105", "100", "1000")).unwrap(); // prev_close=100
        vob.update_bar(&bar("100", "95", "1000")).unwrap();  // open=100, prev=100 -> no gap
        vob.update_bar(&bar("95",  "90", "1000")).unwrap();  // open=95, prev=95 -> no gap
        let v = vob.update_bar(&bar("90", "85", "1000")).unwrap();  // open=90, prev=90 -> no gap
        // window=[0,0,0], vol=[1000,1000,1000] -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vob_reset() {
        let mut vob = VolumeOpenBias::new("vob", 2).unwrap();
        vob.update_bar(&bar("100", "102", "1000")).unwrap();
        vob.update_bar(&bar("105", "107", "1000")).unwrap();
        vob.update_bar(&bar("110", "112", "1000")).unwrap();
        assert!(vob.is_ready());
        vob.reset();
        assert!(!vob.is_ready());
    }
}
