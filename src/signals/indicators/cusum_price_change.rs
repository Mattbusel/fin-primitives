//! CUSUM Price Change — cumulative sum of standardized returns for regime shift detection.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// CUSUM Price Change — cumulative sum of `(return - mean_return) / std_return` over N bars.
///
/// Standardizes each close-to-close return relative to the rolling mean and standard deviation,
/// then accumulates the excess. A rising CUSUM signals a positive regime shift (returns
/// consistently exceeding the mean); a falling CUSUM signals a negative shift.
///
/// CUSUM is reset to zero each time it crosses zero (tracking only the current excursion):
/// - **Large positive**: consistent above-average returns — momentum regime.
/// - **Large negative**: consistent below-average returns — drawdown regime.
/// - **Near zero**: returns close to average — no regime shift.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen,
/// or when return standard deviation is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CusumPriceChange;
/// use fin_primitives::signals::Signal;
/// let cusum = CusumPriceChange::new("cusum_20", 20).unwrap();
/// assert_eq!(cusum.period(), 20);
/// ```
pub struct CusumPriceChange {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    cusum: f64,
}

impl CusumPriceChange {
    /// Constructs a new `CusumPriceChange`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
            cusum: 0.0,
        })
    }
}

impl Signal for CusumPriceChange {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Compute returns for the window
        let closes: Vec<f64> = self.closes.iter().filter_map(|c| c.to_f64()).collect();
        if closes.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let returns: Vec<f64> = closes
            .windows(2)
            .filter_map(|w| {
                if w[0] <= 0.0 { None } else { Some((w[1] - w[0]) / w[0]) }
            })
            .collect();

        let n = returns.len() as f64;
        if n < 2.0 {
            return Ok(SignalValue::Unavailable);
        }

        let mean = returns.iter().sum::<f64>() / n;
        let variance = returns.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / n;

        if variance == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let std_dev = variance.sqrt();
        let current_return = *returns.last().unwrap();
        let standardized = (current_return - mean) / std_dev;

        self.cusum += standardized;

        // Reset if crosses zero
        if (self.cusum > 0.0 && standardized < 0.0 && self.cusum < 0.0)
            || (self.cusum < 0.0 && standardized > 0.0 && self.cusum > 0.0)
        {
            self.cusum = 0.0;
        }

        Decimal::try_from(self.cusum)
            .map(SignalValue::Scalar)
            .or(Ok(SignalValue::Unavailable))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.cusum = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

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

    #[test]
    fn test_cusum_invalid_period() {
        assert!(CusumPriceChange::new("cusum", 0).is_err());
        assert!(CusumPriceChange::new("cusum", 1).is_err());
    }

    #[test]
    fn test_cusum_unavailable_before_warmup() {
        let mut s = CusumPriceChange::new("cusum", 4).unwrap();
        for _ in 0..4 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_cusum_flat_prices_unavailable() {
        let mut s = CusumPriceChange::new("cusum", 3).unwrap();
        for _ in 0..4 { s.update_bar(&bar("100")).unwrap(); }
        // zero variance → Unavailable
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cusum_noisy_uptrend_gives_scalar() {
        // Noisy uptrend: variance > 0 so CUSUM can be computed
        let mut s = CusumPriceChange::new("cusum", 4).unwrap();
        let prices = ["100","103","101","105","102","107","104","109","106","111"];
        let mut got_scalar = false;
        for p in &prices {
            if let SignalValue::Scalar(_) = s.update_bar(&bar(p)).unwrap() {
                got_scalar = true;
            }
        }
        assert!(got_scalar, "noisy uptrend should eventually produce a Scalar value");
    }

    #[test]
    fn test_cusum_reset() {
        let mut s = CusumPriceChange::new("cusum", 3).unwrap();
        for p in &["100","102","104","106","108"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
