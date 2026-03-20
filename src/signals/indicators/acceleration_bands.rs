//! Acceleration Bands indicator (Price Headley).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Acceleration Bands — dynamic bands that expand/contract with volatility.
///
/// Developed by Price Headley. The bands widen in trending markets and narrow
/// in ranging markets, making them useful for breakout identification.
///
/// ```text
/// upper_raw[t] = high × (1 + 4 × (high − low) / (high + low))
/// lower_raw[t] = low  × (1 − 4 × (high − low) / (high + low))
///
/// upper  = SMA(upper_raw, period)
/// middle = SMA(close,     period)
/// lower  = SMA(lower_raw, period)
/// ```
///
/// `update()` returns the **middle band** (SMA of close).
/// Use `upper()` and `lower()` to access the band levels after each `update()` call.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AccelerationBands;
/// use fin_primitives::signals::Signal;
///
/// let mut ab = AccelerationBands::new("ab20", 20).unwrap();
/// assert_eq!(ab.period(), 20);
/// assert!(ab.upper().is_none());
/// ```
pub struct AccelerationBands {
    name: String,
    period: usize,
    upper_window: VecDeque<Decimal>,
    middle_window: VecDeque<Decimal>,
    lower_window: VecDeque<Decimal>,
    upper_sum: Decimal,
    middle_sum: Decimal,
    lower_sum: Decimal,
    upper_band: Option<Decimal>,
    lower_band: Option<Decimal>,
}

impl AccelerationBands {
    /// Constructs a new `AccelerationBands`.
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
            upper_window: VecDeque::with_capacity(period),
            middle_window: VecDeque::with_capacity(period),
            lower_window: VecDeque::with_capacity(period),
            upper_sum: Decimal::ZERO,
            middle_sum: Decimal::ZERO,
            lower_sum: Decimal::ZERO,
            upper_band: None,
            lower_band: None,
        })
    }

    /// Returns the current upper acceleration band, or `None` if not yet ready.
    pub fn upper(&self) -> Option<Decimal> {
        self.upper_band
    }

    /// Returns the current lower acceleration band, or `None` if not yet ready.
    pub fn lower(&self) -> Option<Decimal> {
        self.lower_band
    }

    fn push_window(
        window: &mut VecDeque<Decimal>,
        sum: &mut Decimal,
        value: Decimal,
        period: usize,
    ) -> Option<Decimal> {
        window.push_back(value);
        *sum += value;
        if window.len() > period {
            if let Some(old) = window.pop_front() {
                *sum -= old;
            }
        }
        if window.len() == period {
            #[allow(clippy::cast_possible_truncation)]
            Some(*sum / Decimal::from(period as u32))
        } else {
            None
        }
    }
}

impl Signal for AccelerationBands {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let hl_sum = bar.high + bar.low;
        let spread_factor = if hl_sum.is_zero() {
            Decimal::ZERO
        } else {
            Decimal::from(4u32) * (bar.range()) / hl_sum
        };

        let upper_raw = bar.high * (Decimal::ONE + spread_factor);
        let lower_raw = bar.low * (Decimal::ONE - spread_factor);

        let upper_sma = Self::push_window(
            &mut self.upper_window,
            &mut self.upper_sum,
            upper_raw,
            self.period,
        );
        let middle_sma = Self::push_window(
            &mut self.middle_window,
            &mut self.middle_sum,
            bar.close,
            self.period,
        );
        let lower_sma = Self::push_window(
            &mut self.lower_window,
            &mut self.lower_sum,
            lower_raw,
            self.period,
        );

        match (upper_sma, middle_sma, lower_sma) {
            (Some(u), Some(m), Some(l)) => {
                self.upper_band = Some(u);
                self.lower_band = Some(l);
                Ok(SignalValue::Scalar(m))
            }
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.upper_band.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.upper_window.clear();
        self.middle_window.clear();
        self.lower_window.clear();
        self.upper_sum = Decimal::ZERO;
        self.middle_sum = Decimal::ZERO;
        self.lower_sum = Decimal::ZERO;
        self.upper_band = None;
        self.lower_band = None;
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
        let h_p = Price::new(h.parse().unwrap()).unwrap();
        let l_p = Price::new(l.parse().unwrap()).unwrap();
        let c_p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: c_p, high: h_p, low: l_p, close: c_p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ab_period_0_error() {
        assert!(AccelerationBands::new("ab", 0).is_err());
    }

    #[test]
    fn test_ab_unavailable_before_period() {
        let mut ab = AccelerationBands::new("ab3", 3).unwrap();
        assert_eq!(ab.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ab.update_bar(&bar("115", "95", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!ab.is_ready());
    }

    #[test]
    fn test_ab_ready_after_period() {
        let mut ab = AccelerationBands::new("ab3", 3).unwrap();
        for _ in 0..3 {
            ab.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(ab.is_ready());
        assert!(ab.upper().is_some());
        assert!(ab.lower().is_some());
    }

    #[test]
    fn test_ab_upper_above_lower() {
        let mut ab = AccelerationBands::new("ab3", 3).unwrap();
        for _ in 0..5 {
            ab.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let u = ab.upper().expect("upper must be set");
        let l = ab.lower().expect("lower must be set");
        assert!(u > l, "upper {u} must be > lower {l}");
    }

    #[test]
    fn test_ab_constant_high_low_zero_spread() {
        // When high == low == close, spread_factor = 0, upper = lower = close
        let mut ab = AccelerationBands::new("ab3", 3).unwrap();
        for _ in 0..3 {
            ab.update_bar(&bar("100", "100", "100")).unwrap();
        }
        assert_eq!(ab.upper(), Some(dec!(100)));
        assert_eq!(ab.lower(), Some(dec!(100)));
    }

    #[test]
    fn test_ab_reset() {
        let mut ab = AccelerationBands::new("ab3", 3).unwrap();
        for _ in 0..5 {
            ab.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(ab.is_ready());
        ab.reset();
        assert!(!ab.is_ready());
        assert!(ab.upper().is_none());
        assert!(ab.lower().is_none());
        assert_eq!(ab.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }
}
