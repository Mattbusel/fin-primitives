//! Elastic Volume Weighted Moving Average (EVWMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Elastic Volume Weighted Moving Average (EVWMA).
///
/// A volume-adaptive MA where the smoothing factor is proportional to the
/// current bar's volume relative to the total volume in the lookback window.
/// Bars with high relative volume exert a stronger pull on the average.
///
/// Formula:
/// - `alpha = volume_t / Σ volume_{t−period+1..t}`
/// - `EVWMA_t = (1 − alpha) · EVWMA_{t−1} + alpha · close_t`
///
/// On the first bar, EVWMA is initialized to the closing price.
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Evwma;
/// use fin_primitives::signals::Signal;
/// let ev = Evwma::new("evwma_14", 14).unwrap();
/// assert_eq!(ev.period(), 14);
/// ```
pub struct Evwma {
    name: String,
    period: usize,
    evwma: Option<Decimal>,
    volumes: VecDeque<Decimal>,
    count: usize,
}

impl Evwma {
    /// Constructs a new `Evwma` with the given name and period.
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
            evwma: None,
            volumes: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for Evwma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.period {
            self.volumes.pop_front();
        }
        self.count += 1;

        let vol_sum: Decimal = self.volumes.iter().copied().sum();

        match self.evwma {
            None => {
                self.evwma = Some(bar.close);
                if self.count >= self.period {
                    return Ok(SignalValue::Scalar(bar.close));
                }
                return Ok(SignalValue::Unavailable);
            }
            Some(prev) => {
                let new_evwma = if vol_sum.is_zero() {
                    // Zero volume window: treat as equal weight
                    bar.close
                } else {
                    let alpha = bar.volume
                        .checked_div(vol_sum)
                        .ok_or(FinError::ArithmeticOverflow)?;
                    let one_minus_alpha = Decimal::ONE
                        .checked_sub(alpha)
                        .ok_or(FinError::ArithmeticOverflow)?;
                    let term1 = one_minus_alpha
                        .checked_mul(prev)
                        .ok_or(FinError::ArithmeticOverflow)?;
                    let term2 = alpha
                        .checked_mul(bar.close)
                        .ok_or(FinError::ArithmeticOverflow)?;
                    term1.checked_add(term2).ok_or(FinError::ArithmeticOverflow)?
                };
                self.evwma = Some(new_evwma);
                if self.count >= self.period {
                    return Ok(SignalValue::Scalar(new_evwma));
                }
                Ok(SignalValue::Unavailable)
            }
        }
    }

    fn is_ready(&self) -> bool {
        self.count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.evwma = None;
        self.volumes.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str, volume: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::new(volume.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(Evwma::new("ev", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut ev = Evwma::new("ev", 3).unwrap();
        let v = ev.update_bar(&bar("10", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!ev.is_ready());
    }

    #[test]
    fn test_ready_after_period() {
        let mut ev = Evwma::new("ev", 2).unwrap();
        ev.update_bar(&bar("10", "100")).unwrap();
        let v = ev.update_bar(&bar("20", "100")).unwrap();
        assert!(ev.is_ready());
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_constant_price_returns_constant() {
        let mut ev = Evwma::new("ev", 3).unwrap();
        for _ in 0..3 {
            ev.update_bar(&bar("50", "100")).unwrap();
        }
        let v = ev.update_bar(&bar("50", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_high_volume_bar_pulls_more() {
        // Two indicators: one gets high volume at the end (close = 100),
        // one gets low volume at the end. The high-volume one should be closer to 100.
        let mut ev_high = Evwma::new("ev", 3).unwrap();
        let mut ev_low = Evwma::new("ev", 3).unwrap();
        for _ in 0..2 {
            ev_high.update_bar(&bar("50", "100")).unwrap();
            ev_low.update_bar(&bar("50", "100")).unwrap();
        }
        let vh = match ev_high.update_bar(&bar("100", "1000")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        let vl = match ev_low.update_bar(&bar("100", "1")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        // High volume bar pulls average more toward 100
        assert!(vh > vl);
    }

    #[test]
    fn test_zero_volume_uses_close() {
        let mut ev = Evwma::new("ev", 2).unwrap();
        ev.update_bar(&bar("50", "0")).unwrap();
        let v = ev.update_bar(&bar("80", "0")).unwrap();
        // Zero volume: falls back to using close directly
        assert_eq!(v, SignalValue::Scalar(dec!(80)));
    }

    #[test]
    fn test_reset_clears_state() {
        let mut ev = Evwma::new("ev", 2).unwrap();
        ev.update_bar(&bar("10", "100")).unwrap();
        ev.update_bar(&bar("20", "100")).unwrap();
        assert!(ev.is_ready());
        ev.reset();
        assert!(!ev.is_ready());
        assert!(ev.evwma.is_none());
    }
}
