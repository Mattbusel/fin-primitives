//! Open-Close Velocity indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open-Close Velocity — the rolling average of the signed intrabar move
/// (`close - open`) per bar, measuring the average directional conviction
/// expressed within each bar.
///
/// ```text
/// net_move[i] = close[i] - open[i]
/// output[t]   = mean(net_move[t-period+1 .. t])
/// ```
///
/// - **Positive**: bars are closing above their opens on average (bullish intrabar bias).
/// - **Negative**: bars are closing below their opens on average (bearish intrabar bias).
/// - **Near zero**: intrabar moves cancel out — no directional conviction.
///
/// Unlike momentum (which compares closes across bars), this measures the intrabar
/// resolve — how much the price moves within each individual bar on average.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenCloseVelocity;
/// use fin_primitives::signals::Signal;
/// let ocv = OpenCloseVelocity::new("ocv_10", 10).unwrap();
/// assert_eq!(ocv.period(), 10);
/// ```
pub struct OpenCloseVelocity {
    name: String,
    period: usize,
    moves: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenCloseVelocity {
    /// Constructs a new `OpenCloseVelocity`.
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
            moves: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for OpenCloseVelocity {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.moves.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let m = bar.net_move();
        self.sum += m;
        self.moves.push_back(m);
        if self.moves.len() > self.period {
            let removed = self.moves.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.moves.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mean = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.moves.clear();
        self.sum = Decimal::ZERO;
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
        let hi = op.value().max(cp.value());
        let lo = op.value().min(cp.value());
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op,
            high: Price::new(hi).unwrap(),
            low: Price::new(lo).unwrap(),
            close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ocv_invalid_period() {
        assert!(OpenCloseVelocity::new("ocv", 0).is_err());
    }

    #[test]
    fn test_ocv_unavailable_during_warmup() {
        let mut ocv = OpenCloseVelocity::new("ocv", 3).unwrap();
        assert_eq!(ocv.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ocv.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(!ocv.is_ready());
    }

    #[test]
    fn test_ocv_all_bullish_positive() {
        let mut ocv = OpenCloseVelocity::new("ocv", 3).unwrap();
        for _ in 0..4 {
            ocv.update_bar(&bar("100", "105")).unwrap(); // net_move = +5
        }
        if let SignalValue::Scalar(v) = ocv.update_bar(&bar("100", "105")).unwrap() {
            assert_eq!(v, dec!(5));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocv_all_bearish_negative() {
        let mut ocv = OpenCloseVelocity::new("ocv", 3).unwrap();
        for _ in 0..4 {
            ocv.update_bar(&bar("105", "100")).unwrap(); // net_move = -5
        }
        if let SignalValue::Scalar(v) = ocv.update_bar(&bar("105", "100")).unwrap() {
            assert_eq!(v, dec!(-5));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocv_mixed_near_zero() {
        // +5 and -5 alternating → mean ≈ 0
        let mut ocv = OpenCloseVelocity::new("ocv", 4).unwrap();
        ocv.update_bar(&bar("100", "105")).unwrap();
        ocv.update_bar(&bar("105", "100")).unwrap();
        ocv.update_bar(&bar("100", "105")).unwrap();
        ocv.update_bar(&bar("105", "100")).unwrap();
        if let SignalValue::Scalar(v) = ocv.update_bar(&bar("100", "105")).unwrap() {
            assert!(v.abs() <= dec!(3), "mixed bars → near-zero velocity: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocv_reset() {
        let mut ocv = OpenCloseVelocity::new("ocv", 3).unwrap();
        for _ in 0..3 { ocv.update_bar(&bar("100", "102")).unwrap(); }
        assert!(ocv.is_ready());
        ocv.reset();
        assert!(!ocv.is_ready());
    }
}
