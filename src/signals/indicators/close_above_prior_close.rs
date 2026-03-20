//! Close Above Prior Close — rolling fraction of bars where close exceeds the previous bar's close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above Prior Close — rolling fraction of bars where `close > prev_close`.
///
/// Over the last `period` bar-over-bar comparisons:
/// - **1.0**: every bar closed above the previous (persistent uptrend).
/// - **0.0**: no bar closed above the previous (persistent downtrend).
/// - **0.5**: balanced up/down closes.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAbovePriorClose;
/// use fin_primitives::signals::Signal;
/// let capc = CloseAbovePriorClose::new("capc_10", 10).unwrap();
/// assert_eq!(capc.period(), 10);
/// ```
pub struct CloseAbovePriorClose {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    above: VecDeque<u8>, // 1 if close > prev_close, 0 otherwise
    above_sum: u32,
}

impl CloseAbovePriorClose {
    /// Constructs a new `CloseAbovePriorClose`.
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
            prev_close: None,
            above: VecDeque::with_capacity(period),
            above_sum: 0,
        })
    }
}

impl Signal for CloseAbovePriorClose {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.above.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(prev) = self.prev_close {
            let flag: u8 = if bar.close > prev { 1 } else { 0 };
            self.above_sum += u32::from(flag);
            self.above.push_back(flag);

            if self.above.len() > self.period {
                let removed = self.above.pop_front().unwrap();
                self.above_sum -= u32::from(removed);
            }
        }

        self.prev_close = Some(bar.close);

        if self.above.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let fraction = Decimal::from(self.above_sum)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(fraction))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.above.clear();
        self.above_sum = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_capc_invalid_period() {
        assert!(CloseAbovePriorClose::new("capc", 0).is_err());
    }

    #[test]
    fn test_capc_unavailable_before_warm_up() {
        let mut s = CloseAbovePriorClose::new("capc", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_capc_all_up_gives_one() {
        let mut s = CloseAbovePriorClose::new("capc", 3).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        let v = s.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_capc_all_down_gives_zero() {
        let mut s = CloseAbovePriorClose::new("capc", 3).unwrap();
        s.update_bar(&bar("103")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        let v = s.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_capc_balanced() {
        let mut s = CloseAbovePriorClose::new("capc", 4).unwrap();
        // alternating: up, down, up, down
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        let v = s.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert_eq!(r, dec!(0.5));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_capc_output_in_unit_interval() {
        let mut s = CloseAbovePriorClose::new("capc", 5).unwrap();
        let prices = ["100", "102", "101", "103", "102", "104"];
        for p in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_capc_reset() {
        let mut s = CloseAbovePriorClose::new("capc", 3).unwrap();
        for p in &["100", "101", "102", "103"] {
            s.update_bar(&bar(p)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
