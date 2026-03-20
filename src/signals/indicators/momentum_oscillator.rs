//! Momentum Oscillator — ATR-normalized price change over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Momentum Oscillator — `(close - close[N bars ago]) / ATR(N)`.
///
/// Measures price momentum normalized by recent volatility (ATR), making it
/// scale-independent across instruments and time periods:
/// - **Positive**: price is above N bars ago relative to recent volatility.
/// - **Negative**: price is below N bars ago.
/// - The magnitude indicates how many ATR units of move have occurred.
///
/// ATR uses Wilder's smoothing with the same `period` as the lookback.
///
/// Returns [`SignalValue::Unavailable`] until `period * 2` bars have been seen
/// (one period to seed the ATR, one period for the lookback close), or when ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MomentumOscillator;
/// use fin_primitives::signals::Signal;
/// let mo = MomentumOscillator::new("mo_14", 14).unwrap();
/// assert_eq!(mo.period(), 14);
/// ```
pub struct MomentumOscillator {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    atr: Option<Decimal>,
    prev_close: Option<Decimal>,
    bars_seen: usize,
}

impl MomentumOscillator {
    /// Constructs a new `MomentumOscillator`.
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
            closes: VecDeque::with_capacity(period + 1),
            atr: None,
            prev_close: None,
            bars_seen: 0,
        })
    }
}

impl Signal for MomentumOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen >= self.period * 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);
        self.bars_seen += 1;

        let period_d = Decimal::from(self.period as u32);
        self.atr = Some(match self.atr {
            None => tr,
            Some(prev_atr) => (prev_atr * (period_d - Decimal::ONE) + tr) / period_d,
        });

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        if self.bars_seen < self.period * 2 {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.atr.unwrap();
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        // closes[0] is the close from `period` bars ago
        let lookback_close = *self.closes.front().unwrap();
        let momentum = bar.close - lookback_close;
        let normalized = momentum.checked_div(atr).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(normalized))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.atr = None;
        self.prev_close = None;
        self.bars_seen = 0;
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
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_mo_invalid_period() {
        assert!(MomentumOscillator::new("mo", 0).is_err());
    }

    #[test]
    fn test_mo_unavailable_before_2x_period() {
        let mut mo = MomentumOscillator::new("mo", 3).unwrap();
        for _ in 0..5 {
            assert_eq!(mo.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!mo.is_ready());
    }

    #[test]
    fn test_mo_ready_after_2x_period() {
        let mut mo = MomentumOscillator::new("mo", 3).unwrap();
        for _ in 0..6 {
            mo.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(mo.is_ready());
    }

    #[test]
    fn test_mo_flat_prices_zero_momentum() {
        // Constant prices → momentum = 0
        let mut mo = MomentumOscillator::new("mo", 3).unwrap();
        for _ in 0..8 {
            mo.update_bar(&bar("100", "100", "100")).unwrap();
        }
        // ATR is zero with flat prices → Unavailable
        let v = mo.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_mo_rising_market_positive() {
        let mut mo = MomentumOscillator::new("mo", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..10 {
            let c = (100 + i * 5).to_string();
            let h = (105 + i * 5).to_string();
            last = mo.update_bar(&bar(&h, "95", &c)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "rising market should give positive momentum, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mo_reset() {
        let mut mo = MomentumOscillator::new("mo", 3).unwrap();
        for _ in 0..8 {
            mo.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(mo.is_ready());
        mo.reset();
        assert!(!mo.is_ready());
    }

    #[test]
    fn test_mo_period_and_name() {
        let mo = MomentumOscillator::new("my_mo", 14).unwrap();
        assert_eq!(mo.period(), 14);
        assert_eq!(mo.name(), "my_mo");
    }
}
