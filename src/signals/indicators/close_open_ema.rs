//! Close-Open EMA — exponential moving average of the bar body (close minus open).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close-Open EMA — EMA of `(close - open)` over `period` bars.
///
/// Smooths the raw bar body direction to reveal the prevailing intrabar bias:
/// - **Positive**: recent bars consistently close above their open — bullish bias.
/// - **Negative**: recent bars consistently close below their open — bearish bias.
/// - **Near zero**: mixed or doji-heavy price action — indecision.
///
/// Uses standard EMA smoothing: `k = 2 / (period + 1)`.
/// Returns [`SignalValue::Unavailable`] for the first bar (no prior EMA).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseOpenEma;
/// use fin_primitives::signals::Signal;
/// let coe = CloseOpenEma::new("coe_10", 10).unwrap();
/// assert_eq!(coe.period(), 10);
/// ```
pub struct CloseOpenEma {
    name: String,
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    bars_seen: usize,
}

impl CloseOpenEma {
    /// Constructs a new `CloseOpenEma`.
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
            bars_seen: 0,
        })
    }
}

impl Signal for CloseOpenEma {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bars_seen > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.net_move();
        let ema = match self.ema {
            None => body,
            Some(prev) => body * self.k + prev * (Decimal::ONE - self.k),
        };
        self.ema = Some(ema);
        self.bars_seen += 1;

        if self.bars_seen <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(ema))
    }

    fn reset(&mut self) {
        self.ema = None;
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
    fn test_coe_invalid_period() {
        assert!(CloseOpenEma::new("coe", 0).is_err());
    }

    #[test]
    fn test_coe_unavailable_during_warmup() {
        let mut s = CloseOpenEma::new("coe", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_coe_positive_for_consistent_bull_bars() {
        let mut s = CloseOpenEma::new("coe", 2).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100", "105")).unwrap(); }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100", "105")).unwrap() {
            assert!(v > dec!(0), "consistent bull bars should give positive EMA: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_coe_negative_for_consistent_bear_bars() {
        let mut s = CloseOpenEma::new("coe", 2).unwrap();
        for _ in 0..3 { s.update_bar(&bar("105", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105", "100")).unwrap() {
            assert!(v < dec!(0), "consistent bear bars should give negative EMA: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_coe_reset() {
        let mut s = CloseOpenEma::new("coe", 2).unwrap();
        for _ in 0..4 { s.update_bar(&bar("100", "105")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
