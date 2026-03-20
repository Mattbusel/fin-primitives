//! True Range EMA indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// True Range EMA — exponentially smoothed True Range (Wilder's ATR variant).
///
/// ```text
/// TR_t   = max(high−low, |high−prev_close|, |low−prev_close|)
/// TR_EMA = EMA(TR, period)   [standard EMA with k = 2/(period+1)]
/// ```
///
/// Unlike the classical Wilder ATR (which uses k = 1/period), this uses the
/// standard EMA smoothing factor for faster responsiveness to volatility changes.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen
/// (first bar uses high-low as TR since there is no previous close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrueRangeEma;
/// use fin_primitives::signals::Signal;
///
/// let tre = TrueRangeEma::new("tre", 14).unwrap();
/// assert_eq!(tre.period(), 14);
/// ```
pub struct TrueRangeEma {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    ema: Option<Decimal>,
    seed: Vec<Decimal>,
}

impl TrueRangeEma {
    /// Creates a new `TrueRangeEma`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            ema: None,
            seed: Vec::with_capacity(period),
        })
    }
}

impl Signal for TrueRangeEma {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => bar.high - bar.low,
            Some(pc) => (bar.high - bar.low)
                .max((bar.high - pc).abs())
                .max((bar.low - pc).abs()),
        };
        self.prev_close = Some(bar.close);

        let k = Decimal::from(2u32) / Decimal::from((self.period + 1) as u32);

        if self.ema.is_none() {
            self.seed.push(tr);
            if self.seed.len() == self.period {
                let sma = self.seed.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
                self.ema = Some(sma);
            }
            return Ok(SignalValue::Unavailable);
        }

        let prev = self.ema.unwrap();
        let new_ema = prev + k * (tr - prev);
        self.ema = Some(new_ema);
        Ok(SignalValue::Scalar(new_ema))
    }

    fn is_ready(&self) -> bool { self.ema.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.ema = None;
        self.seed.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar(c: &str) -> OhlcvBar { bar_hlc(c, c, c) }

    #[test]
    fn test_tre_invalid() {
        assert!(TrueRangeEma::new("t", 0).is_err());
    }

    #[test]
    fn test_tre_unavailable_before_warmup() {
        let mut t = TrueRangeEma::new("t", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(t.update_bar(&bar_hlc("105", "95", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_tre_constant_range() {
        // Each bar has range=10; EMA converges to 10
        let mut t = TrueRangeEma::new("t", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 { last = t.update_bar(&bar_hlc("105", "95", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            let diff = (v - dec!(10)).abs();
            assert!(diff < dec!(0.001), "expected ~10, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_tre_positive() {
        // EMA of TR must always be positive (TR >= 0)
        let mut t = TrueRangeEma::new("t", 3).unwrap();
        for h in ["105", "107", "103", "110", "98"] {
            if let SignalValue::Scalar(v) = t.update_bar(&bar_hlc(h, "90", "100")).unwrap() {
                assert!(v > dec!(0), "expected positive, got {v}");
            }
        }
    }

    #[test]
    fn test_tre_reset() {
        let mut t = TrueRangeEma::new("t", 3).unwrap();
        for _ in 0..10 { t.update_bar(&bar_hlc("105", "95", "100")).unwrap(); }
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
        assert_eq!(t.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
