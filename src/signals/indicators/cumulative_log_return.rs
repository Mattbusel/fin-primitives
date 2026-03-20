//! Cumulative Log Return — running sum of log returns since the last reset.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Cumulative Log Return — sum of `ln(close[t] / close[t-1])` from the first bar seen.
///
/// The cumulative log return equals `ln(current_close / initial_close)`, which
/// approximates the total percentage return for small moves. For large moves, it
/// correctly accounts for compounding.
///
/// - **Positive**: price has risen since the start.
/// - **Negative**: price has fallen since the start.
/// - **Zero**: price is unchanged.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close to compare).
/// Returns [`SignalValue::Unavailable`] if a close price is zero or negative.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CumulativeLogReturn;
/// use fin_primitives::signals::Signal;
/// let clr = CumulativeLogReturn::new("clr");
/// assert_eq!(clr.period(), 1);
/// ```
pub struct CumulativeLogReturn {
    name: String,
    prev_close: Option<Decimal>,
    cumulative: f64,
}

impl CumulativeLogReturn {
    /// Constructs a new `CumulativeLogReturn`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prev_close: None,
            cumulative: 0.0,
        }
    }
}

impl Signal for CumulativeLogReturn {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let Some(prev) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        if prev <= Decimal::ZERO || bar.close <= Decimal::ZERO {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        }

        let prev_f = prev.to_f64().unwrap_or(0.0);
        let curr_f = bar.close.to_f64().unwrap_or(0.0);
        if prev_f <= 0.0 || curr_f <= 0.0 {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        }

        self.cumulative += (curr_f / prev_f).ln();
        self.prev_close = Some(bar.close);

        let result = Decimal::try_from(self.cumulative).unwrap_or(Decimal::ZERO);
        Ok(SignalValue::Scalar(result))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.cumulative = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_clr_unavailable_on_first_bar() {
        let mut clr = CumulativeLogReturn::new("clr");
        assert!(!clr.is_ready());
        assert_eq!(clr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        // After seeding prev_close, is_ready=true (next bar will produce a value)
        assert!(clr.is_ready());
    }

    #[test]
    fn test_clr_no_change_zero() {
        let mut clr = CumulativeLogReturn::new("clr");
        clr.update_bar(&bar("100")).unwrap();
        let v = clr.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r.abs() < dec!(0.000001), "no price change should give ~0: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_clr_accumulates_over_bars() {
        let mut clr = CumulativeLogReturn::new("clr");
        clr.update_bar(&bar("100")).unwrap();
        clr.update_bar(&bar("110")).unwrap(); // ln(110/100)
        let v = clr.update_bar(&bar("121")).unwrap(); // ln(121/110) → total = ln(121/100)
        if let SignalValue::Scalar(r) = v {
            // ln(1.21) ≈ 0.19062
            assert!(r > dec!(0.18) && r < dec!(0.20), "expected ~ln(1.21), got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_clr_roundtrip_returns_zero() {
        // 100 → 110 → 100 should give 0 cumulative log return
        let mut clr = CumulativeLogReturn::new("clr");
        clr.update_bar(&bar("100")).unwrap();
        clr.update_bar(&bar("110")).unwrap();
        let v = clr.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r.abs() < dec!(0.000001), "roundtrip should give ~0: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_clr_falling_market_negative() {
        let mut clr = CumulativeLogReturn::new("clr");
        clr.update_bar(&bar("100")).unwrap();
        clr.update_bar(&bar("90")).unwrap();
        let v = clr.update_bar(&bar("80")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(0), "falling market should give negative cumulative log return: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_clr_reset() {
        let mut clr = CumulativeLogReturn::new("clr");
        clr.update_bar(&bar("100")).unwrap();
        clr.update_bar(&bar("110")).unwrap();
        assert!(clr.is_ready());
        clr.reset();
        assert!(!clr.is_ready());
        assert_eq!(clr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_clr_period_and_name() {
        let clr = CumulativeLogReturn::new("my_clr");
        assert_eq!(clr.period(), 1);
        assert_eq!(clr.name(), "my_clr");
    }
}
