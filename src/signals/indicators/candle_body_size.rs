//! Candle Body Size indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Candle Body Size — absolute size of the candle body as a percentage of the high-low range.
///
/// ```text
/// body_size_pct = |close - open| / (high - low) * 100
/// ```
///
/// 100 means the candle is a pure Marubozu (no wicks); 0 means the open equals the close
/// (a Doji). Returns [`SignalValue::Unavailable`] when the high-low range is zero
/// (flat bars). Ready immediately (no warm-up required).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandleBodySize;
/// use fin_primitives::signals::Signal;
///
/// let cbs = CandleBodySize::new("cbs");
/// assert_eq!(cbs.period(), 1);
/// ```
pub struct CandleBodySize {
    name: String,
    ready: bool,
}

impl CandleBodySize {
    /// Constructs a new `CandleBodySize`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for CandleBodySize {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            // Cannot compute body-to-range for flat bars; mark ready but return Unavailable
            self.ready = true;
            return Ok(SignalValue::Unavailable);
        }
        let body = (bar.close - bar.open).abs();
        let pct = body
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);
        self.ready = true;
        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
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
    fn test_cbs_marubozu_100() {
        // O=90, H=110, L=90, C=110 => body=20, range=20 => 100%
        let mut cbs = CandleBodySize::new("cbs");
        let v = cbs.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_cbs_doji_zero() {
        // O=C=100, H=110, L=90 => body=0, range=20 => 0%
        let mut cbs = CandleBodySize::new("cbs");
        let v = cbs.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cbs_flat_bar_unavailable() {
        // H=L=100 => range=0 => Unavailable
        let mut cbs = CandleBodySize::new("cbs");
        let v = cbs.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_cbs_half_body() {
        // O=100, H=110, L=90, C=110 => body=10, range=20 => 50%
        let mut cbs = CandleBodySize::new("cbs");
        let v = cbs.update_bar(&bar("100", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_cbs_is_ready_after_first_bar() {
        let mut cbs = CandleBodySize::new("cbs");
        assert!(!cbs.is_ready());
        cbs.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(cbs.is_ready());
    }

    #[test]
    fn test_cbs_reset() {
        let mut cbs = CandleBodySize::new("cbs");
        cbs.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(cbs.is_ready());
        cbs.reset();
        assert!(!cbs.is_ready());
    }
}
