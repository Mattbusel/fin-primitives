//! Low-Minus-PrevClose indicator.
//!
//! Tracks the EMA of `(low[t] - close[t-1])`, measuring the downside gap or
//! extension below the prior bar's close each bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `low[t] − close[t−1]`.
///
/// - **Negative**: the bar's low consistently falls below the prior close —
///   downside gaps or bearish extensions.
/// - **Near zero**: the low is at or near the prior close — stable or flat opens.
/// - **Positive**: the bar's low consistently stays above the prior close —
///   bullish gap regime (the market opens and stays elevated).
///
/// Returns [`SignalValue::Unavailable`] on the first bar. `is_ready()` is `true`
/// after the second bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowMinusPrevClose;
/// use fin_primitives::signals::Signal;
///
/// let lmpc = LowMinusPrevClose::new("lmpc", 10).unwrap();
/// assert_eq!(lmpc.period(), 10);
/// assert!(!lmpc.is_ready());
/// ```
pub struct LowMinusPrevClose {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    ema: Option<Decimal>,
    k: Decimal,
    seen_bars: usize,
}

impl LowMinusPrevClose {
    /// Constructs a new `LowMinusPrevClose`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            ema: None,
            k,
            seen_bars: 0,
        })
    }
}

impl crate::signals::Signal for LowMinusPrevClose {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.seen_bars >= 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.seen_bars += 1;

        let result = if let Some(pc) = self.prev_close {
            let delta = bar.low - pc;
            let ema = match self.ema {
                None => { self.ema = Some(delta); delta }
                Some(prev) => {
                    let next = delta * self.k + prev * (Decimal::ONE - self.k);
                    self.ema = Some(next);
                    next
                }
            };
            SignalValue::Scalar(ema)
        } else {
            SignalValue::Unavailable
        };

        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.ema = None;
        self.seen_bars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(low: &str, close: &str) -> OhlcvBar {
        let l = Price::new(low.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let high = if c > l { c } else { l };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high, low: l, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_lmpc_invalid_period() {
        assert!(LowMinusPrevClose::new("lmpc", 0).is_err());
    }

    #[test]
    fn test_lmpc_first_bar_unavailable() {
        let mut lmpc = LowMinusPrevClose::new("lmpc", 5).unwrap();
        assert_eq!(lmpc.update_bar(&bar("90", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!lmpc.is_ready());
    }

    #[test]
    fn test_lmpc_ready_after_second_bar() {
        let mut lmpc = LowMinusPrevClose::new("lmpc", 5).unwrap();
        lmpc.update_bar(&bar("90", "105")).unwrap();
        lmpc.update_bar(&bar("85", "100")).unwrap();
        assert!(lmpc.is_ready());
    }

    #[test]
    fn test_lmpc_gap_down_negative() {
        let mut lmpc = LowMinusPrevClose::new("lmpc", 5).unwrap();
        lmpc.update_bar(&bar("90", "100")).unwrap(); // close=100
        // low=85 < 100 → delta = -15
        let v = lmpc.update_bar(&bar("85", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-15)));
    }

    #[test]
    fn test_lmpc_gap_up_positive() {
        let mut lmpc = LowMinusPrevClose::new("lmpc", 5).unwrap();
        lmpc.update_bar(&bar("90", "100")).unwrap(); // close=100
        // low=105 > 100 → delta = 5
        let v = lmpc.update_bar(&bar("105", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_lmpc_reset() {
        let mut lmpc = LowMinusPrevClose::new("lmpc", 5).unwrap();
        lmpc.update_bar(&bar("90", "105")).unwrap();
        lmpc.update_bar(&bar("85", "100")).unwrap();
        assert!(lmpc.is_ready());
        lmpc.reset();
        assert!(!lmpc.is_ready());
    }
}
