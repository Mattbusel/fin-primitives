//! Return Sign Changes indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Sign Changes — the number of times the close-to-close return changes
/// sign over the last `period` returns.
///
/// A high count indicates frequent direction reversals (choppy market).
/// A low count indicates directional persistence (trending market).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnSignChanges;
/// use fin_primitives::signals::Signal;
///
/// let rsc = ReturnSignChanges::new("rsc", 10).unwrap();
/// assert_eq!(rsc.period(), 10);
/// ```
pub struct ReturnSignChanges {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    signs: VecDeque<i8>, // 1, -1, or 0
}

impl ReturnSignChanges {
    /// Constructs a new `ReturnSignChanges`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            signs: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ReturnSignChanges {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.signs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let sign: i8 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
                self.signs.push_back(sign);
                if self.signs.len() > self.period { self.signs.pop_front(); }

                if self.signs.len() < self.period {
                    SignalValue::Unavailable
                } else {
                    let changes = self.signs.iter().zip(self.signs.iter().skip(1))
                        .filter(|(&a, &b)| a != 0 && b != 0 && a != b)
                        .count();
                    SignalValue::Scalar(Decimal::from(changes as u32))
                }
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.signs.clear();
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
    fn test_rsc_invalid_period() {
        assert!(ReturnSignChanges::new("rsc", 0).is_err());
        assert!(ReturnSignChanges::new("rsc", 1).is_err());
    }

    #[test]
    fn test_rsc_unavailable_before_warm_up() {
        let mut rsc = ReturnSignChanges::new("rsc", 3).unwrap();
        rsc.update_bar(&bar("100")).unwrap();
        assert_eq!(rsc.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rsc_trending_gives_zero_changes() {
        // All bars go up → sign=+1 always → zero sign changes
        let mut rsc = ReturnSignChanges::new("rsc", 4).unwrap();
        rsc.update_bar(&bar("100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 1u32..=4 {
            last = rsc.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        // signs = [+1, +1, +1, +1] → 0 changes
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rsc_alternating_gives_max_changes() {
        // +1, -1, +1, -1 → 3 sign changes
        let mut rsc = ReturnSignChanges::new("rsc", 4).unwrap();
        let prices = ["100", "101", "100", "101", "100"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = rsc.update_bar(&bar(p)).unwrap();
        }
        // signs over window = [-1, +1, -1, +1] or [+1,-1,+1,-1] → 3 changes
        if let SignalValue::Scalar(v) = last {
            assert!(v >= dec!(2), "alternating prices should give multiple changes: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rsc_reset() {
        let mut rsc = ReturnSignChanges::new("rsc", 3).unwrap();
        for p in ["100", "101", "100", "101"] { rsc.update_bar(&bar(p)).unwrap(); }
        assert!(rsc.is_ready());
        rsc.reset();
        assert!(!rsc.is_ready());
    }
}
