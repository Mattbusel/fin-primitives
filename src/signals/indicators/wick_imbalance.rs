//! Wick Imbalance — difference between upper and lower wick sizes, normalized by ATR.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Wick Imbalance — `(upper_wick - lower_wick) / ATR(period)`.
///
/// Measures whether the bar's wicks are skewed upward or downward relative to
/// recent volatility:
/// - **Positive**: upper wick dominates (bearish rejection of higher prices).
/// - **Negative**: lower wick dominates (bullish rejection of lower prices).
/// - **Near zero**: balanced wicks or no wicks.
///
/// ATR uses Wilder's smoothing.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen,
/// or when ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WickImbalance;
/// use fin_primitives::signals::Signal;
/// let wi = WickImbalance::new("wi_14", 14).unwrap();
/// assert_eq!(wi.period(), 14);
/// ```
pub struct WickImbalance {
    name: String,
    period: usize,
    atr: Option<Decimal>,
    prev_close: Option<Decimal>,
    bars_seen: usize,
}

impl WickImbalance {
    /// Constructs a new `WickImbalance`.
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

impl Signal for WickImbalance {
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

        let period_d = Decimal::from(self.period as u32);
        self.atr = Some(match self.atr {
            None => tr,
            Some(prev) => (prev * (period_d - Decimal::ONE) + tr) / period_d,
        });

        if self.bars_seen < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.atr.unwrap();
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let upper_wick = bar.high - bar.open.max(bar.close);
        let lower_wick = bar.open.min(bar.close) - bar.low;
        let imbalance = upper_wick - lower_wick;

        let normalized = imbalance.checked_div(atr).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(normalized))
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_wi_invalid_period() {
        assert!(WickImbalance::new("wi", 0).is_err());
    }

    #[test]
    fn test_wi_unavailable_before_period() {
        let mut wi = WickImbalance::new("wi", 3).unwrap();
        assert_eq!(wi.update_bar(&bar("100", "110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(wi.update_bar(&bar("100", "110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!wi.is_ready());
    }

    #[test]
    fn test_wi_upper_wick_dominant_positive() {
        // open=close=100, high=120, low=95 → upper_wick=20, lower_wick=5 → imbalance > 0
        let mut wi = WickImbalance::new("wi", 3).unwrap();
        for _ in 0..4 {
            wi.update_bar(&bar("100", "120", "95", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = wi.update_bar(&bar("100", "120", "95", "100")).unwrap() {
            assert!(v > dec!(0), "upper wick > lower wick should give positive imbalance: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wi_lower_wick_dominant_negative() {
        // open=close=100, high=105, low=80 → upper_wick=5, lower_wick=20 → imbalance < 0
        let mut wi = WickImbalance::new("wi", 3).unwrap();
        for _ in 0..4 {
            wi.update_bar(&bar("100", "105", "80", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = wi.update_bar(&bar("100", "105", "80", "100")).unwrap() {
            assert!(v < dec!(0), "lower wick > upper wick should give negative imbalance: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wi_balanced_wicks_near_zero() {
        // open=close=100, high=110, low=90 → upper=lower=10 → imbalance=0
        let mut wi = WickImbalance::new("wi", 2).unwrap();
        wi.update_bar(&bar("100", "110", "90", "100")).unwrap();
        let v = wi.update_bar(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r.abs() < dec!(0.001), "balanced wicks should give ~0 imbalance: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wi_reset() {
        let mut wi = WickImbalance::new("wi", 2).unwrap();
        for _ in 0..3 {
            wi.update_bar(&bar("100", "110", "90", "100")).unwrap();
        }
        assert!(wi.is_ready());
        wi.reset();
        assert!(!wi.is_ready());
    }

    #[test]
    fn test_wi_period_and_name() {
        let wi = WickImbalance::new("my_wi", 14).unwrap();
        assert_eq!(wi.period(), 14);
        assert_eq!(wi.name(), "my_wi");
    }
}
