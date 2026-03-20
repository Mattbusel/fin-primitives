//! Momentum Divergence indicator — price ROC minus volume ROC.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Momentum Divergence — the spread between price rate-of-change and volume rate-of-change.
///
/// Computed as `price_roc - volume_roc` where both ROC values are calculated over `period`
/// bars as `(current - previous) / previous * 100`.
///
/// Interpretation:
/// - **Positive**: price momentum outpaces volume momentum (price-led move).
/// - **Near zero**: price and volume momentum are aligned.
/// - **Negative**: volume is expanding faster than price (potential exhaustion or distribution).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen, or when
/// the denominator for either ROC is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MomentumDivergence;
/// use fin_primitives::signals::Signal;
/// let md = MomentumDivergence::new("md_10", 10).unwrap();
/// assert_eq!(md.period(), 10);
/// ```
pub struct MomentumDivergence {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
}

impl MomentumDivergence {
    /// Constructs a new `MomentumDivergence`.
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
            closes: VecDeque::with_capacity(period + 1),
            volumes: VecDeque::with_capacity(period + 1),
        })
    }

    fn roc(current: Decimal, past: Decimal) -> Option<Decimal> {
        if past.is_zero() {
            return None;
        }
        (current - past).checked_div(past)?.checked_mul(Decimal::ONE_HUNDRED)
    }
}

impl Signal for MomentumDivergence {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.closes.len() > self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        self.volumes.push_back(bar.volume);

        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
            self.volumes.pop_front();
        }

        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let past_close = self.closes[0];
        let curr_close = *self.closes.back().unwrap();
        let past_vol = self.volumes[0];
        let curr_vol = *self.volumes.back().unwrap();

        let Some(price_roc) = Self::roc(curr_close, past_close) else {
            return Ok(SignalValue::Unavailable);
        };

        // Volume ROC: if past volume is zero, emit Unavailable.
        let Some(vol_roc) = Self::roc(curr_vol, past_vol) else {
            return Ok(SignalValue::Scalar(price_roc));
        };

        let divergence = price_roc - vol_roc;
        Ok(SignalValue::Scalar(divergence))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.volumes.clear();
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
    fn test_md_invalid_period() {
        assert!(MomentumDivergence::new("md", 0).is_err());
    }

    #[test]
    fn test_md_unavailable_before_period_plus_one() {
        let mut md = MomentumDivergence::new("md", 3).unwrap();
        for _ in 0..3 {
            let v = md.update_bar(&bar("100", "1000")).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!md.is_ready());
    }

    #[test]
    fn test_md_produces_value_after_period_plus_one() {
        let mut md = MomentumDivergence::new("md", 2).unwrap();
        md.update_bar(&bar("100", "1000")).unwrap();
        md.update_bar(&bar("105", "1100")).unwrap();
        let v = md.update_bar(&bar("110", "1050")).unwrap();
        assert!(v.is_scalar());
        assert!(md.is_ready());
    }

    #[test]
    fn test_md_aligned_momentum_near_zero() {
        let mut md = MomentumDivergence::new("md", 1).unwrap();
        // Both price and volume up 10% → divergence ≈ 0
        md.update_bar(&bar("100", "1000")).unwrap();
        let v = md.update_bar(&bar("110", "1100")).unwrap();
        if let SignalValue::Scalar(d) = v {
            assert!(d.abs() < dec!(0.001), "expected ~0, got {d}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_md_price_led_positive() {
        let mut md = MomentumDivergence::new("md", 1).unwrap();
        // Price up 10%, volume flat → divergence = 10 - 0 = 10
        md.update_bar(&bar("100", "1000")).unwrap();
        let v = md.update_bar(&bar("110", "1000")).unwrap();
        if let SignalValue::Scalar(d) = v {
            assert!(d > dec!(0), "expected positive divergence, got {d}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_md_volume_led_negative() {
        let mut md = MomentumDivergence::new("md", 1).unwrap();
        // Price flat, volume up 10% → divergence = 0 - 10 = -10
        md.update_bar(&bar("100", "1000")).unwrap();
        let v = md.update_bar(&bar("100", "1100")).unwrap();
        if let SignalValue::Scalar(d) = v {
            assert!(d < dec!(0), "expected negative divergence, got {d}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_md_reset() {
        let mut md = MomentumDivergence::new("md", 2).unwrap();
        for _ in 0..3 {
            md.update_bar(&bar("100", "1000")).unwrap();
        }
        assert!(md.is_ready());
        md.reset();
        assert!(!md.is_ready());
    }

    #[test]
    fn test_md_period_and_name() {
        let md = MomentumDivergence::new("my_md", 10).unwrap();
        assert_eq!(md.period(), 10);
        assert_eq!(md.name(), "my_md");
    }
}
