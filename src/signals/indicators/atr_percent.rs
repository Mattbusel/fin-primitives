//! ATR Percent — Average True Range as a percentage of the closing price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// ATR Percent — `ATR(period) / close * 100`.
///
/// Expresses volatility as a scale-independent percentage, enabling comparison
/// across instruments with different price levels.
///
/// The ATR is computed using Wilder's smoothing (same as [`crate::signals::indicators::Atr`]):
/// `ATR[t] = (ATR[t-1] * (period - 1) + TR[t]) / period`.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when
/// the close price is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AtrPercent;
/// use fin_primitives::signals::Signal;
/// let ap = AtrPercent::new("atr_pct_14", 14).unwrap();
/// assert_eq!(ap.period(), 14);
/// ```
pub struct AtrPercent {
    name: String,
    period: usize,
    atr: Option<Decimal>,
    prev_close: Option<Decimal>,
    bars_seen: usize,
}

impl AtrPercent {
    /// Constructs a new `AtrPercent`.
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
            atr: None,
            prev_close: None,
            bars_seen: 0,
        })
    }
}

impl Signal for AtrPercent {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);
        self.bars_seen += 1;

        self.atr = Some(match self.atr {
            None => tr,
            Some(prev_atr) => {
                let period_d = Decimal::from(self.period as u32);
                (prev_atr * (period_d - Decimal::ONE) + tr) / period_d
            }
        });

        if self.bars_seen < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.atr.unwrap();
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let pct = atr
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::ONE_HUNDRED)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
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
    fn test_ap_invalid_period() {
        assert!(AtrPercent::new("ap", 0).is_err());
    }

    #[test]
    fn test_ap_unavailable_before_period() {
        let mut ap = AtrPercent::new("ap", 3).unwrap();
        assert_eq!(ap.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ap.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!ap.is_ready());
    }

    #[test]
    fn test_ap_non_negative() {
        let mut ap = AtrPercent::new("ap", 3).unwrap();
        let bars = [
            bar("110", "90", "100"),
            bar("112", "88", "102"),
            bar("108", "92", "98"),
            bar("115", "85", "105"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = ap.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "ATR% must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_ap_flat_bars_low_atr() {
        // Very tight bars → low ATR%
        let mut ap = AtrPercent::new("ap", 3).unwrap();
        for _ in 0..5 {
            ap.update_bar(&bar("100.1", "99.9", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = ap.update_bar(&bar("100.1", "99.9", "100")).unwrap() {
            assert!(v < dec!(1), "tight bars should have < 1% ATR, got {v}");
        }
    }

    #[test]
    fn test_ap_reset() {
        let mut ap = AtrPercent::new("ap", 3).unwrap();
        for _ in 0..4 {
            ap.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(ap.is_ready());
        ap.reset();
        assert!(!ap.is_ready());
    }

    #[test]
    fn test_ap_period_and_name() {
        let ap = AtrPercent::new("my_ap", 14).unwrap();
        assert_eq!(ap.period(), 14);
        assert_eq!(ap.name(), "my_ap");
    }
}
