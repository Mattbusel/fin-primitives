//! ZigZag indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// ZigZag — identifies significant swing pivot highs and lows by tracking
/// reversals that exceed a minimum percentage threshold.
///
/// The indicator filters out small price oscillations and emits:
/// - `+1` when the current bar confirms a swing **low** (bottom pivot)
/// - `-1` when the current bar confirms a swing **high** (top pivot)
/// - `0` when no reversal has occurred yet
///
/// A reversal is triggered when price moves `threshold_pct` percent against the
/// current direction from the last confirmed extreme.
///
/// Use [`ZigZag::last_pivot`] and [`ZigZag::last_pivot_price`] to get the most
/// recently confirmed pivot type and price.
///
/// Returns [`SignalValue::Scalar(0)`] from the first bar (always ready) while
/// direction is being established.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ZigZag;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let z = ZigZag::new("zz", dec!(5)).unwrap();
/// assert_eq!(z.period(), 1);
/// ```
pub struct ZigZag {
    name: String,
    threshold_pct: Decimal,
    /// `true` = last confirmed swing was high, `false` = low.
    last_was_high: Option<bool>,
    /// Price of the last confirmed extreme.
    last_extreme: Option<Decimal>,
    /// Last confirmed pivot value (+1/-1).
    last_pivot: Option<Decimal>,
    last_pivot_price: Option<Decimal>,
}

impl ZigZag {
    /// Constructs a new `ZigZag` with the given reversal threshold percentage.
    ///
    /// `threshold_pct` must be > 0. Typical values: 1.0 to 10.0.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `threshold_pct <= 0`.
    pub fn new(name: impl Into<String>, threshold_pct: Decimal) -> Result<Self, FinError> {
        if threshold_pct <= Decimal::ZERO {
            return Err(FinError::InvalidPeriod(0));
        }
        Ok(Self {
            name: name.into(),
            threshold_pct,
            last_was_high: None,
            last_extreme: None,
            last_pivot: None,
            last_pivot_price: None,
        })
    }

    /// Returns `+1` if the last confirmed pivot was a swing low, `-1` for a swing high.
    pub fn last_pivot(&self) -> Option<Decimal> {
        self.last_pivot
    }

    /// Returns the price of the last confirmed pivot extreme.
    pub fn last_pivot_price(&self) -> Option<Decimal> {
        self.last_pivot_price
    }
}

impl Signal for ZigZag {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let high = bar.high;
        let low  = bar.low;
        let threshold = self.threshold_pct / Decimal::ONE_HUNDRED;

        match self.last_was_high {
            None => {
                // Bootstrap: treat first bar's high as initial extreme upward
                self.last_was_high = Some(true);
                self.last_extreme  = Some(high);
                return Ok(SignalValue::Scalar(Decimal::ZERO));
            }
            Some(was_high) => {
                let extreme = self.last_extreme.unwrap_or(high);
                if was_high {
                    // Looking for reversal down
                    if high > extreme {
                        // New high — extend current swing up
                        self.last_extreme = Some(high);
                        Ok(SignalValue::Scalar(Decimal::NEGATIVE_ONE))
                    } else if extreme > Decimal::ZERO
                        && (extreme - low) / extreme >= threshold
                    {
                        // Reversal confirmed — swing low
                        self.last_was_high = Some(false);
                        self.last_extreme  = Some(low);
                        self.last_pivot       = Some(Decimal::ONE);
                        self.last_pivot_price = Some(low);
                        Ok(SignalValue::Scalar(Decimal::ONE))
                    } else {
                        Ok(SignalValue::Scalar(Decimal::ZERO))
                    }
                } else {
                    // Looking for reversal up
                    if low < extreme {
                        // New low — extend current swing down
                        self.last_extreme = Some(low);
                        Ok(SignalValue::Scalar(Decimal::ONE))
                    } else if extreme > Decimal::ZERO
                        && (high - extreme) / extreme >= threshold
                    {
                        // Reversal confirmed — swing high
                        self.last_was_high = Some(true);
                        self.last_extreme  = Some(high);
                        self.last_pivot       = Some(Decimal::NEGATIVE_ONE);
                        self.last_pivot_price = Some(high);
                        Ok(SignalValue::Scalar(Decimal::NEGATIVE_ONE))
                    } else {
                        Ok(SignalValue::Scalar(Decimal::ZERO))
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        self.last_was_high    = None;
        self.last_extreme     = None;
        self.last_pivot       = None;
        self.last_pivot_price = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_zigzag_invalid_threshold() {
        assert!(ZigZag::new("zz", dec!(0)).is_err());
        assert!(ZigZag::new("zz", dec!(-1)).is_err());
    }

    #[test]
    fn test_zigzag_is_ready_immediately() {
        let z = ZigZag::new("zz", dec!(5)).unwrap();
        assert!(z.is_ready());
    }

    #[test]
    fn test_zigzag_first_bar_zero() {
        let mut z = ZigZag::new("zz", dec!(5)).unwrap();
        let v = z.update_bar(&bar("100", "95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_zigzag_detects_reversal_low() {
        let mut z = ZigZag::new("zz", dec!(5)).unwrap();
        // Start: high=100
        z.update_bar(&bar("100", "95")).unwrap();
        // Still going up
        z.update_bar(&bar("110", "105")).unwrap();
        // Drop > 5%: from 110, need to drop to 110 * 0.95 = 104.5 → 90 qualifies
        let v = z.update_bar(&bar("92", "88")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1))); // swing low detected
        assert_eq!(z.last_pivot(), Some(dec!(1)));
    }

    #[test]
    fn test_zigzag_detects_reversal_high() {
        let mut z = ZigZag::new("zz", dec!(5)).unwrap();
        z.update_bar(&bar("100", "95")).unwrap(); // init
        z.update_bar(&bar("90", "85")).unwrap();  // low extends
        // Rise > 5% from 85: need 85 * 1.05 = 89.25 → high=100 qualifies
        let v = z.update_bar(&bar("100", "88")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1))); // swing high detected
    }

    #[test]
    fn test_zigzag_reset() {
        let mut z = ZigZag::new("zz", dec!(5)).unwrap();
        z.update_bar(&bar("100", "95")).unwrap();
        z.update_bar(&bar("110", "105")).unwrap();
        z.reset();
        assert!(z.last_pivot().is_none());
        assert!(z.last_pivot_price().is_none());
    }
}
