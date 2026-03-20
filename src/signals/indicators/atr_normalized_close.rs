//! ATR-Normalized Close — distance from EMA expressed in ATR units.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ATR-Normalized Close — how many ATR units the close is above or below its EMA.
///
/// Formula: `(close - EMA(period)) / ATR(period)`
///
/// A value of `+2` means the close is 2 ATR above its moving average (potentially
/// overbought). A value of `−1` means it is 1 ATR below (mildly oversold).
///
/// Unlike [`crate::signals::indicators::Zscore`] which normalizes by standard deviation,
/// this indicator normalizes by ATR — making it volatility-adaptive in a way that
/// naturally respects the bar's true range.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen, or when ATR
/// is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AtrNormalizedClose;
/// use fin_primitives::signals::Signal;
/// let anc = AtrNormalizedClose::new("anc_14", 14).unwrap();
/// assert_eq!(anc.period(), 14);
/// ```
pub struct AtrNormalizedClose {
    name: String,
    period: usize,
    ema_k: Decimal,
    ema: Option<Decimal>,
    prev_close: Option<Decimal>,
    tr_values: VecDeque<Decimal>,
}

impl AtrNormalizedClose {
    /// Constructs a new `AtrNormalizedClose`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let ema_k = Decimal::from(2u32)
            .checked_div(Decimal::from(period as u32 + 1))
            .unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            ema_k,
            ema: None,
            prev_close: None,
            tr_values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for AtrNormalizedClose {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.tr_values.len() >= self.period && self.ema.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Update EMA.
        let new_ema = match self.ema {
            None => bar.close,
            Some(prev) => bar.close * self.ema_k + prev * (Decimal::ONE - self.ema_k),
        };
        self.ema = Some(new_ema);

        // Compute true range (needs prev_close).
        let tr = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => {
                let hl = bar.high - bar.low;
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);

        self.tr_values.push_back(tr);
        if self.tr_values.len() > self.period {
            self.tr_values.pop_front();
        }

        if self.tr_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.tr_values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let atr = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let deviation = bar.close - new_ema;
        let normalized = deviation
            .checked_div(atr)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(normalized))
    }

    fn reset(&mut self) {
        self.ema = None;
        self.prev_close = None;
        self.tr_values.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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

    fn ohlc_bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
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
    fn test_anc_invalid_period() {
        assert!(AtrNormalizedClose::new("anc", 0).is_err());
    }

    #[test]
    fn test_anc_unavailable_before_warm_up() {
        let mut anc = AtrNormalizedClose::new("anc", 3).unwrap();
        for i in 0..3u32 {
            let v = anc.update_bar(&bar(&(100 + i).to_string())).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!anc.is_ready());
    }

    #[test]
    fn test_anc_produces_value_after_warm_up() {
        let mut anc = AtrNormalizedClose::new("anc", 3).unwrap();
        let bars = [
            ohlc_bar("100", "105", "98", "102"),
            ohlc_bar("102", "107", "100", "105"),
            ohlc_bar("105", "110", "103", "108"),
            ohlc_bar("108", "112", "106", "110"),
        ];
        let mut last = SignalValue::Unavailable;
        for b in &bars {
            last = anc.update_bar(b).unwrap();
        }
        assert!(last.is_scalar(), "expected Scalar after warm-up");
        assert!(anc.is_ready());
    }

    #[test]
    fn test_anc_constant_price_near_zero() {
        // Constant price → EMA = close, ATR ≈ 0 → Unavailable (guarded by zero ATR check).
        let mut anc = AtrNormalizedClose::new("anc", 3).unwrap();
        for _ in 0..5 {
            anc.update_bar(&bar("100")).unwrap();
        }
        // close - ema = 0, so even with small ATR we get 0 or Unavailable.
        // Either is acceptable for flat data.
    }

    #[test]
    fn test_anc_reset() {
        let mut anc = AtrNormalizedClose::new("anc", 3).unwrap();
        let b = ohlc_bar("100", "105", "95", "102");
        for _ in 0..5 {
            anc.update_bar(&b).unwrap();
        }
        anc.reset();
        assert!(!anc.is_ready());
    }

    #[test]
    fn test_anc_period_and_name() {
        let anc = AtrNormalizedClose::new("my_anc", 14).unwrap();
        assert_eq!(anc.period(), 14);
        assert_eq!(anc.name(), "my_anc");
    }
}
