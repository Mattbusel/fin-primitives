//! Rolling Open Bias — SMA of (close − open) over a rolling window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Open Bias — the average body direction of the last `period` bars.
///
/// Defined as the simple moving average of `(close - open)` over `period` bars.
///
/// - **Positive**: recent bars consistently close above their open (bullish bias).
/// - **Negative**: recent bars consistently close below their open (bearish bias).
/// - **Near zero**: no persistent directional bias.
///
/// Unlike momentum indicators that compare closes across bars, this indicator captures
/// intrabar directionality — the bias that shows up inside each candle body.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingOpenBias;
/// use fin_primitives::signals::Signal;
/// let rob = RollingOpenBias::new("rob_10", 10).unwrap();
/// assert_eq!(rob.period(), 10);
/// ```
pub struct RollingOpenBias {
    name: String,
    period: usize,
    bodies: VecDeque<Decimal>,
}

impl RollingOpenBias {
    /// Constructs a new `RollingOpenBias`.
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
            bodies: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingOpenBias {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.bodies.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bodies.push_back(bar.net_move());
        if self.bodies.len() > self.period {
            self.bodies.pop_front();
        }
        if self.bodies.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let sum: Decimal = self.bodies.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn reset(&mut self) {
        self.bodies.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let h = if o.value() >= c.value() { o } else { c };
        let l = if o.value() <= c.value() { o } else { c };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high: h, low: l, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rob_invalid_period() {
        assert!(RollingOpenBias::new("rob", 0).is_err());
    }

    #[test]
    fn test_rob_unavailable_before_period() {
        let mut rob = RollingOpenBias::new("rob", 3).unwrap();
        assert_eq!(rob.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rob.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!rob.is_ready());
    }

    #[test]
    fn test_rob_all_bullish_positive() {
        let mut rob = RollingOpenBias::new("rob", 3).unwrap();
        // 3 bars each with body = +5
        for _ in 0..3 {
            rob.update_bar(&bar("100", "105")).unwrap();
        }
        let v = rob.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_rob_all_bearish_negative() {
        let mut rob = RollingOpenBias::new("rob", 3).unwrap();
        for _ in 0..3 {
            rob.update_bar(&bar("105", "100")).unwrap();
        }
        let v = rob.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-5)));
    }

    #[test]
    fn test_rob_mixed_near_zero() {
        let mut rob = RollingOpenBias::new("rob", 2).unwrap();
        // +5 and -5 → average = 0
        rob.update_bar(&bar("100", "105")).unwrap();
        let v = rob.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rob_reset() {
        let mut rob = RollingOpenBias::new("rob", 2).unwrap();
        rob.update_bar(&bar("100", "105")).unwrap();
        rob.update_bar(&bar("100", "105")).unwrap();
        assert!(rob.is_ready());
        rob.reset();
        assert!(!rob.is_ready());
    }

    #[test]
    fn test_rob_period_and_name() {
        let rob = RollingOpenBias::new("my_rob", 10).unwrap();
        assert_eq!(rob.period(), 10);
        assert_eq!(rob.name(), "my_rob");
    }
}
