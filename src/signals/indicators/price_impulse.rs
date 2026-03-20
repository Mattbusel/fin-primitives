//! Price Impulse indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Impulse — the `n`-bar price change normalized by the ATR(n).
///
/// ```text
/// impulse = (close - close[n]) / ATR(n)
/// ```
///
/// This provides a volatility-adjusted momentum signal. Values > 2 or < -2
/// suggest a strong directional move relative to normal volatility.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceImpulse;
/// use fin_primitives::signals::Signal;
///
/// let pi = PriceImpulse::new("pi", 14).unwrap();
/// assert_eq!(pi.period(), 14);
/// ```
pub struct PriceImpulse {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    closes: VecDeque<Decimal>,
    trs: VecDeque<Decimal>,
    tr_sum: Decimal,
}

impl PriceImpulse {
    /// Constructs a new `PriceImpulse`.
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
            closes: VecDeque::with_capacity(period + 1),
            trs: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceImpulse {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period && self.trs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        self.tr_sum += tr;
        if self.trs.len() > self.period {
            self.tr_sum -= self.trs.pop_front().unwrap();
        }

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        if self.closes.len() <= self.period || self.trs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let nd = Decimal::from(self.period as u32);
        let atr = self.tr_sum / nd;
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let old_close = self.closes[0];
        Ok(SignalValue::Scalar((bar.close - old_close) / atr))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.closes.clear();
        self.trs.clear();
        self.tr_sum = Decimal::ZERO;
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
    fn test_pi_invalid_period() {
        assert!(PriceImpulse::new("pi", 0).is_err());
    }

    #[test]
    fn test_pi_unavailable_before_warm_up() {
        let mut pi = PriceImpulse::new("pi", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(pi.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pi_positive_impulse() {
        let mut pi = PriceImpulse::new("pi", 3).unwrap();
        // Seed 3 bars at 100 (TR=10 each), then spike to 130
        for _ in 0..3 { pi.update_bar(&bar("105", "95", "100")).unwrap(); }
        let result = pi.update_bar(&bar("135", "125", "130")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(0), "rising close should give positive impulse: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pi_reset() {
        let mut pi = PriceImpulse::new("pi", 3).unwrap();
        for _ in 0..4 { pi.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(pi.is_ready());
        pi.reset();
        assert!(!pi.is_ready());
    }
}
