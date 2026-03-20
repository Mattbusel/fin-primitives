//! Open-Close Momentum — EMA of the net bar body as a fraction of the prior close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open-Close Momentum — EMA of `(close - open) / prev_close` over `period` bars.
///
/// Measures the intrabar directional move normalized by prior close (a stable reference):
/// - **Positive and growing**: bars consistently closing above open with increasing size.
/// - **Negative**: bearish intrabar bias.
/// - **Near zero**: mixed or small-bodied bars.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs prior close as denominator).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenCloseMomentum;
/// use fin_primitives::signals::Signal;
/// let ocm = OpenCloseMomentum::new("ocm_10", 10).unwrap();
/// assert_eq!(ocm.period(), 10);
/// ```
pub struct OpenCloseMomentum {
    name: String,
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    prev_close: Option<Decimal>,
    bars_seen: usize,
}

impl OpenCloseMomentum {
    /// Constructs a new `OpenCloseMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            k,
            ema: None,
            prev_close: None,
            bars_seen: 0,
        })
    }
}

impl Signal for OpenCloseMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bars_seen > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pc) = self.prev_close {
            if pc.is_zero() {
                SignalValue::Unavailable
            } else {
                let body_ret = (bar.close - bar.open)
                    .checked_div(pc)
                    .ok_or(FinError::ArithmeticOverflow)?;
                let ema = match self.ema {
                    None => body_ret,
                    Some(prev) => body_ret * self.k + prev * (Decimal::ONE - self.k),
                };
                self.ema = Some(ema);
                self.bars_seen += 1;
                if self.bars_seen <= self.period {
                    SignalValue::Unavailable
                } else {
                    SignalValue::Scalar(ema)
                }
            }
        } else {
            SignalValue::Unavailable
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.ema = None;
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hp = if cp > op { cp } else { op };
        let lp = if cp < op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ocm_invalid_period() {
        assert!(OpenCloseMomentum::new("ocm", 0).is_err());
    }

    #[test]
    fn test_ocm_unavailable_during_warmup() {
        let mut s = OpenCloseMomentum::new("ocm", 2).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100","102")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_ocm_positive_for_consistent_bull_bars() {
        let mut s = OpenCloseMomentum::new("ocm", 2).unwrap();
        for _ in 0..4 { s.update_bar(&bar("100","105")).unwrap(); }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","105")).unwrap() {
            assert!(v > dec!(0), "consistent bull bars → positive OCM: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocm_negative_for_consistent_bear_bars() {
        let mut s = OpenCloseMomentum::new("ocm", 2).unwrap();
        for _ in 0..4 { s.update_bar(&bar("105","100")).unwrap(); }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105","100")).unwrap() {
            assert!(v < dec!(0), "consistent bear bars → negative OCM: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocm_reset() {
        let mut s = OpenCloseMomentum::new("ocm", 2).unwrap();
        for _ in 0..5 { s.update_bar(&bar("100","105")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
