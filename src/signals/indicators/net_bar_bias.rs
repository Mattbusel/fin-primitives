//! Net Bar Bias — rolling fraction of net up-bars minus down-bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Net Bar Bias — `(up_bars - down_bars) / period` over the last `period` bars.
///
/// Measures the directional tilt of recent bar closes:
/// - **+1.0**: every bar is bullish (`close > open`).
/// - **-1.0**: every bar is bearish.
/// - **0.0**: equal number of up and down bars (or all doji).
///
/// Doji bars (`close == open`) count as neither up nor down.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NetBarBias;
/// use fin_primitives::signals::Signal;
/// let nbb = NetBarBias::new("nbb_10", 10).unwrap();
/// assert_eq!(nbb.period(), 10);
/// ```
pub struct NetBarBias {
    name: String,
    period: usize,
    window: VecDeque<i8>, // +1, -1, or 0 per bar
    net: i32,
}

impl NetBarBias {
    /// Constructs a new `NetBarBias`.
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
            net: 0,
        })
    }
}

impl Signal for NetBarBias {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let flag: i8 = if bar.is_bullish() {
            1
        } else if bar.is_bearish() {
            -1
        } else {
            0
        };

        self.net += i32::from(flag);
        self.window.push_back(flag);

        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.net -= i32::from(removed);
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let bias = Decimal::from(self.net)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(bias))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.net = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp.max(op), low: cp.min(op), close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_nbb_invalid_period() {
        assert!(NetBarBias::new("nbb", 0).is_err());
    }

    #[test]
    fn test_nbb_unavailable_before_period() {
        let mut s = NetBarBias::new("nbb", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100","105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("105","110")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nbb_all_bullish_gives_one() {
        let mut s = NetBarBias::new("nbb", 3).unwrap();
        s.update_bar(&bar("100","105")).unwrap();
        s.update_bar(&bar("105","110")).unwrap();
        let v = s.update_bar(&bar("110","115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_nbb_all_bearish_gives_neg_one() {
        let mut s = NetBarBias::new("nbb", 3).unwrap();
        s.update_bar(&bar("105","100")).unwrap();
        s.update_bar(&bar("110","105")).unwrap();
        let v = s.update_bar(&bar("115","110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_nbb_balanced_gives_zero() {
        let mut s = NetBarBias::new("nbb", 4).unwrap();
        s.update_bar(&bar("100","105")).unwrap(); // up
        s.update_bar(&bar("105","100")).unwrap(); // down
        s.update_bar(&bar("100","105")).unwrap(); // up
        let v = s.update_bar(&bar("105","100")).unwrap(); // down; net=0 → 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_nbb_output_in_unit_interval() {
        let mut s = NetBarBias::new("nbb", 5).unwrap();
        for (o, c) in &[("100","105"),("105","102"),("102","107"),("107","104"),("104","109"),("109","106")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(o, c)).unwrap() {
                assert!(v >= dec!(-1) && v <= dec!(1), "out of [-1,1]: {v}");
            }
        }
    }

    #[test]
    fn test_nbb_reset() {
        let mut s = NetBarBias::new("nbb", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100","105")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
