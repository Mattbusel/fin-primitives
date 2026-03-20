//! Parabolic SAR (Stop and Reverse) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Parabolic SAR — a trailing stop-and-reverse trend-following indicator.
///
/// The SAR value flips between a rising SAR (uptrend) and a falling SAR
/// (downtrend) whenever price crosses through it.
///
/// Default parameters: `step = 0.02`, `max_af = 0.20`.
///
/// Returns [`SignalValue::Unavailable`] for the first bar (needs a prior bar to
/// establish direction).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ParabolicSar;
/// use fin_primitives::signals::Signal;
///
/// let mut psar = ParabolicSar::new("psar", "0.02".parse().unwrap(), "0.20".parse().unwrap()).unwrap();
/// assert_eq!(psar.period(), 1);
/// ```
pub struct ParabolicSar {
    name: String,
    step: Decimal,
    max_af: Decimal,
    // State
    sar: Option<Decimal>,
    ep: Decimal,  // extreme point
    af: Decimal,  // acceleration factor
    uptrend: bool,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
}

impl ParabolicSar {
    /// Constructs a new `ParabolicSar`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `step <= 0` or `max_af <= 0` or `step > max_af`.
    pub fn new(name: impl Into<String>, step: Decimal, max_af: Decimal) -> Result<Self, FinError> {
        if step <= Decimal::ZERO {
            return Err(FinError::InvalidInput(format!("step must be > 0, got {step}")));
        }
        if max_af <= Decimal::ZERO {
            return Err(FinError::InvalidInput(format!("max_af must be > 0, got {max_af}")));
        }
        if step > max_af {
            return Err(FinError::InvalidInput(format!(
                "step ({step}) must be <= max_af ({max_af})"
            )));
        }
        Ok(Self {
            name: name.into(),
            step,
            max_af,
            sar: None,
            ep: Decimal::ZERO,
            af: step,
            uptrend: true,
            prev_high: None,
            prev_low: None,
        })
    }
}

impl Signal for ParabolicSar {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let high = bar.high;
        let low = bar.low;

        let (prev_high, prev_low) = match (self.prev_high, self.prev_low) {
            (Some(h), Some(l)) => (h, l),
            _ => {
                // First bar: seed state, return Unavailable
                self.prev_high = Some(high);
                self.prev_low = Some(low);
                return Ok(SignalValue::Unavailable);
            }
        };

        let sar = match self.sar {
            Some(s) => s,
            None => {
                // Second bar: initialize SAR from first bar
                // Start as uptrend if this bar's close > prev low
                self.uptrend = high >= prev_high;
                let sar_init = if self.uptrend { prev_low } else { prev_high };
                self.ep = if self.uptrend { high } else { low };
                self.af = self.step;
                self.sar = Some(sar_init);
                self.prev_high = Some(high);
                self.prev_low = Some(low);
                return Ok(SignalValue::Scalar(sar_init));
            }
        };

        // Compute next SAR
        let mut next_sar = sar + self.af * (self.ep - sar);

        if self.uptrend {
            // SAR cannot be above the previous two lows
            next_sar = next_sar.min(prev_low).min(low);

            if low < next_sar {
                // Reversal to downtrend
                self.uptrend = false;
                next_sar = self.ep; // SAR flips to extreme point
                self.ep = low;
                self.af = self.step;
            } else {
                // Continue uptrend
                if high > self.ep {
                    self.ep = high;
                    self.af = (self.af + self.step).min(self.max_af);
                }
            }
        } else {
            // SAR cannot be below the previous two highs
            next_sar = next_sar.max(prev_high).max(high);

            if high > next_sar {
                // Reversal to uptrend
                self.uptrend = true;
                next_sar = self.ep;
                self.ep = high;
                self.af = self.step;
            } else {
                // Continue downtrend
                if low < self.ep {
                    self.ep = low;
                    self.af = (self.af + self.step).min(self.max_af);
                }
            }
        }

        self.sar = Some(next_sar);
        self.prev_high = Some(high);
        self.prev_low = Some(low);
        Ok(SignalValue::Scalar(next_sar))
    }

    fn is_ready(&self) -> bool {
        self.sar.is_some()
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.sar = None;
        self.ep = Decimal::ZERO;
        self.af = self.step;
        self.uptrend = true;
        self.prev_high = None;
        self.prev_low = None;
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
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo,
            high: hi,
            low: lo,
            close: hi,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_psar_invalid_params() {
        assert!(ParabolicSar::new("p", dec!(0), dec!(0.2)).is_err());
        assert!(ParabolicSar::new("p", dec!(0.02), dec!(0)).is_err());
        assert!(ParabolicSar::new("p", dec!(0.3), dec!(0.2)).is_err());
    }

    #[test]
    fn test_psar_first_bar_unavailable() {
        let mut psar = ParabolicSar::new("p", dec!(0.02), dec!(0.20)).unwrap();
        assert_eq!(psar.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!psar.is_ready());
    }

    #[test]
    fn test_psar_second_bar_ready() {
        let mut psar = ParabolicSar::new("p", dec!(0.02), dec!(0.20)).unwrap();
        psar.update_bar(&bar("110", "90")).unwrap();
        let v = psar.update_bar(&bar("115", "95")).unwrap();
        assert!(psar.is_ready());
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_psar_reset_clears_state() {
        let mut psar = ParabolicSar::new("p", dec!(0.02), dec!(0.20)).unwrap();
        psar.update_bar(&bar("110", "90")).unwrap();
        psar.update_bar(&bar("115", "95")).unwrap();
        assert!(psar.is_ready());
        psar.reset();
        assert!(!psar.is_ready());
    }
}
