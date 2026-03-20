//! Breakout Signal — detects when close breaks above or below its N-period channel.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Breakout Signal — outputs directional channel breakout state.
///
/// Tracks the highest high and lowest low over the last `period` bars. On each bar:
///
/// - **`+1`**: current close > `period`-bar channel high (upside breakout).
/// - **`-1`**: current close < `period`-bar channel low (downside breakout).
/// - **` 0`**: close is inside the channel.
///
/// The channel is evaluated *before* including the current bar's high/low, so the
/// current bar can itself trigger a breakout against prior bars' extremes.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BreakoutSignal;
/// use fin_primitives::signals::Signal;
/// let bs = BreakoutSignal::new("bo_20", 20).unwrap();
/// assert_eq!(bs.period(), 20);
/// ```
pub struct BreakoutSignal {
    name: String,
    period: usize,
    /// Rolling window of (high, low) for the last `period` bars.
    window: VecDeque<(Decimal, Decimal)>,
    ready: bool,
}

impl BreakoutSignal {
    /// Constructs a new `BreakoutSignal`.
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
            ready: false,
        })
    }
}

impl Signal for BreakoutSignal {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Evaluate breakout against existing window *before* adding current bar.
        let signal = if self.window.len() >= self.period {
            let channel_high = self.window.iter().map(|(h, _)| *h).fold(Decimal::MIN, Decimal::max);
            let channel_low = self.window.iter().map(|(_, l)| *l).fold(Decimal::MAX, Decimal::min);
            if bar.close > channel_high {
                Decimal::ONE
            } else if bar.close < channel_low {
                -Decimal::ONE
            } else {
                Decimal::ZERO
            }
        } else {
            // Not yet ready — will return Unavailable below.
            Decimal::ZERO
        };

        let was_ready = self.window.len() >= self.period;

        self.window.push_back((bar.high, bar.low));
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if !was_ready {
            return Ok(SignalValue::Unavailable);
        }

        self.ready = true;
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(c.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bs_invalid_period() {
        assert!(BreakoutSignal::new("bs", 0).is_err());
    }

    #[test]
    fn test_bs_unavailable_before_period() {
        let mut bs = BreakoutSignal::new("bs", 3).unwrap();
        for _ in 0..3 {
            let v = bs.update_bar(&bar("105", "95", "100")).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!bs.is_ready());
    }

    #[test]
    fn test_bs_inside_channel_is_zero() {
        let mut bs = BreakoutSignal::new("bs", 3).unwrap();
        // Build channel high=105, low=95 over 3 bars.
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        bs.update_bar(&bar("104", "96", "101")).unwrap();
        bs.update_bar(&bar("103", "97", "100")).unwrap();
        // Bar within channel: close=101
        let v = bs.update_bar(&bar("102", "98", "101")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bs_upside_breakout_is_one() {
        let mut bs = BreakoutSignal::new("bs", 3).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        // Close above channel high (105).
        let v = bs.update_bar(&bar("110", "106", "108")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bs_downside_breakout_is_neg_one() {
        let mut bs = BreakoutSignal::new("bs", 3).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        // Close below channel low (95).
        let v = bs.update_bar(&bar("94", "88", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_bs_reset() {
        // period=2: need period+1=3 bars before first scalar.
        let mut bs = BreakoutSignal::new("bs", 2).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        bs.update_bar(&bar("105", "95", "100")).unwrap();
        assert!(bs.is_ready());
        bs.reset();
        assert!(!bs.is_ready());
    }

    #[test]
    fn test_bs_period_and_name() {
        let bs = BreakoutSignal::new("my_bs", 20).unwrap();
        assert_eq!(bs.period(), 20);
        assert_eq!(bs.name(), "my_bs");
    }
}
