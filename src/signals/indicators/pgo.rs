//! Pretty Good Oscillator (PGO) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Pretty Good Oscillator: `(close - SMA(period)) / ATR(period)`.
///
/// Values above +3 are considered very overbought; below -3 very oversold.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Pgo;
/// use fin_primitives::signals::Signal;
///
/// let mut pgo = Pgo::new("pgo14", 14).unwrap();
/// assert_eq!(pgo.period(), 14);
/// ```
pub struct Pgo {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    trs: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl Pgo {
    /// Constructs a new `Pgo`.
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
            closes: VecDeque::with_capacity(period),
            trs: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for Pgo {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = if let Some(pc) = self.prev_close {
            (bar.high - bar.low).max((bar.high - pc).abs()).max((bar.low - pc).abs())
        } else {
            bar.high - bar.low
        };
        self.prev_close = Some(bar.close);

        self.closes.push_back(bar.close);
        self.trs.push_back(tr);
        if self.closes.len() > self.period {
            self.closes.pop_front();
            self.trs.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma: Decimal = self.closes.iter().sum::<Decimal>();
        let atr: Decimal = self.trs.iter().sum::<Decimal>();
        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let sma = sma / n;
        let atr = atr / n;

        if atr == Decimal::ZERO {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar((bar.close - sma) / atr))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.trs.clear();
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pgo_period_0_error() {
        assert!(Pgo::new("p", 0).is_err());
    }

    #[test]
    fn test_pgo_unavailable_before_period() {
        let mut pgo = Pgo::new("p3", 3).unwrap();
        assert_eq!(pgo.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(pgo.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert!(pgo.update_bar(&bar("110","90","100")).unwrap().is_scalar());
    }

    #[test]
    fn test_pgo_flat_market_zero() {
        let mut pgo = Pgo::new("p3", 3).unwrap();
        for _ in 0..5 { pgo.update_bar(&bar("110","90","100")).unwrap(); }
        match pgo.update_bar(&bar("110","90","100")).unwrap() {
            SignalValue::Scalar(v) => assert_eq!(v, dec!(0)),
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_pgo_reset() {
        let mut pgo = Pgo::new("p3", 3).unwrap();
        for _ in 0..5 { pgo.update_bar(&bar("110","90","100")).unwrap(); }
        assert!(pgo.is_ready());
        pgo.reset();
        assert!(!pgo.is_ready());
    }
}
