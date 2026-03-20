//! Volume Flow Ratio — rolling (up-volume minus down-volume) / total volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Flow Ratio — `(up_vol - down_vol) / total_vol` over the last `period` bars.
///
/// Classifies each bar's volume as bullish (close > open) or bearish (close < open)
/// and measures the net directional volume bias:
/// - **+1.0**: all volume in bullish bars — strong buying pressure.
/// - **0.0**: balanced buying and selling.
/// - **-1.0**: all volume in bearish bars — strong selling pressure.
///
/// Bars where `close == open` (doji) contribute to total but not to either side.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated,
/// or when total volume is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeFlowRatio;
/// use fin_primitives::signals::Signal;
/// let vfr = VolumeFlowRatio::new("vfr_14", 14).unwrap();
/// assert_eq!(vfr.period(), 14);
/// ```
pub struct VolumeFlowRatio {
    name: String,
    period: usize,
    // Store (direction: i8, volume) per bar: +1 bull, -1 bear, 0 doji
    window: VecDeque<(i8, Decimal)>,
    up_vol: Decimal,
    down_vol: Decimal,
    total_vol: Decimal,
}

impl VolumeFlowRatio {
    /// Constructs a new `VolumeFlowRatio`.
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
            up_vol: Decimal::ZERO,
            down_vol: Decimal::ZERO,
            total_vol: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeFlowRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let dir: i8 = if bar.close > bar.open {
            1
        } else if bar.close < bar.open {
            -1
        } else {
            0
        };

        // Add new bar
        match dir {
            1  => self.up_vol += bar.volume,
            -1 => self.down_vol += bar.volume,
            _  => {}
        }
        self.total_vol += bar.volume;
        self.window.push_back((dir, bar.volume));

        // Remove oldest bar
        if self.window.len() > self.period {
            let (old_dir, old_vol) = self.window.pop_front().unwrap();
            match old_dir {
                1  => self.up_vol -= old_vol,
                -1 => self.down_vol -= old_vol,
                _  => {}
            }
            self.total_vol -= old_vol;
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.total_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let net = self.up_vol - self.down_vol;
        let ratio = net
            .checked_div(self.total_vol)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio.max(Decimal::NEGATIVE_ONE).min(Decimal::ONE)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.up_vol = Decimal::ZERO;
        self.down_vol = Decimal::ZERO;
        self.total_vol = Decimal::ZERO;
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
        let hp = if cp > op { cp } else { op };
        let lp = if cp < op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vfr_invalid_period() {
        assert!(VolumeFlowRatio::new("vfr", 0).is_err());
    }

    #[test]
    fn test_vfr_unavailable_before_period() {
        let mut s = VolumeFlowRatio::new("vfr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100","105","1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("100","105","1000")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_vfr_all_bull_gives_one() {
        let mut s = VolumeFlowRatio::new("vfr", 2).unwrap();
        s.update_bar(&bar("100","105","1000")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","105","1000")).unwrap() {
            assert_eq!(v, dec!(1), "all bull volume should give +1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vfr_all_bear_gives_negative_one() {
        let mut s = VolumeFlowRatio::new("vfr", 2).unwrap();
        s.update_bar(&bar("105","100","1000")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105","100","1000")).unwrap() {
            assert_eq!(v, dec!(-1), "all bear volume should give -1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vfr_equal_bull_bear_gives_zero() {
        let mut s = VolumeFlowRatio::new("vfr", 2).unwrap();
        s.update_bar(&bar("100","105","1000")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105","100","1000")).unwrap() {
            assert_eq!(v, dec!(0), "equal bull/bear volume should give 0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vfr_in_range() {
        let mut s = VolumeFlowRatio::new("vfr", 3).unwrap();
        let bars = [("100","105","1000"),("105","100","800"),("100","103","1200"),("103","99","500")];
        for (o,c,v) in &bars {
            if let SignalValue::Scalar(val) = s.update_bar(&bar(o,c,v)).unwrap() {
                assert!(val >= dec!(-1) && val <= dec!(1), "ratio out of [-1,1]: {val}");
            }
        }
    }

    #[test]
    fn test_vfr_reset() {
        let mut s = VolumeFlowRatio::new("vfr", 2).unwrap();
        s.update_bar(&bar("100","105","1000")).unwrap();
        s.update_bar(&bar("100","105","1000")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
