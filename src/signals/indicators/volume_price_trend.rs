//! Volume Price Trend indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Price Trend (VPT).
///
/// A cumulative indicator that adds/subtracts a volume-weighted percentage price change
/// to a running total each bar. Related to On-Balance Volume (OBV) but weights each
/// contribution by the magnitude of the price change.
///
/// Formula (per bar):
/// - If prev_close exists: `delta = volume * (close - prev_close) / prev_close`
/// - `vpt = vpt_prev + delta`
///
/// The cumulative sum starts at 0. Positive drift indicates accumulation; negative drift
/// indicates distribution.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePriceTrend;
/// use fin_primitives::signals::Signal;
/// let vpt = VolumePriceTrend::new("vpt");
/// assert_eq!(vpt.period(), 1);
/// ```
pub struct VolumePriceTrend {
    name: String,
    prev_close: Option<Decimal>,
    cumulative: Decimal,
}

impl VolumePriceTrend {
    /// Constructs a new `VolumePriceTrend`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None, cumulative: Decimal::ZERO }
    }
}

impl Signal for VolumePriceTrend {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(prev_c) = self.prev_close {
            if prev_c.is_zero() {
                self.cumulative
            } else {
                let pct_change = (bar.close - prev_c)
                    .checked_div(prev_c)
                    .ok_or(FinError::ArithmeticOverflow)?;
                let delta = bar.volume
                    .checked_mul(pct_change)
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.cumulative += delta;
                self.cumulative
            }
        } else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        self.prev_close = Some(bar.close);
        Ok(SignalValue::Scalar(result))
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some()
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.cumulative = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_first_bar_unavailable() {
        let mut vpt = VolumePriceTrend::new("vpt");
        assert_eq!(vpt.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rising_price_positive_vpt() {
        let mut vpt = VolumePriceTrend::new("vpt");
        vpt.update_bar(&bar("100", "1000")).unwrap();
        let v = vpt.update_bar(&bar("110", "1000")).unwrap();
        // delta = 1000 * 0.1 = 100
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_falling_price_negative_delta() {
        let mut vpt = VolumePriceTrend::new("vpt");
        vpt.update_bar(&bar("110", "1000")).unwrap();
        let v = vpt.update_bar(&bar("100", "1000")).unwrap();
        // delta ≈ 1000 * (-10/110) ≈ -90.909...
        if let SignalValue::Scalar(s) = v {
            assert!(s < dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_cumulative_accumulates() {
        let mut vpt = VolumePriceTrend::new("vpt");
        vpt.update_bar(&bar("100", "1000")).unwrap();
        vpt.update_bar(&bar("110", "1000")).unwrap(); // +100
        let v = vpt.update_bar(&bar("121", "1000")).unwrap(); // +100 more = 200
        assert_eq!(v, SignalValue::Scalar(dec!(200)));
    }

    #[test]
    fn test_reset() {
        let mut vpt = VolumePriceTrend::new("vpt");
        vpt.update_bar(&bar("100", "1000")).unwrap();
        vpt.update_bar(&bar("110", "1000")).unwrap();
        assert!(vpt.is_ready());
        vpt.reset();
        assert!(!vpt.is_ready());
        // After reset, first bar is Unavailable again
        assert_eq!(vpt.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }
}
