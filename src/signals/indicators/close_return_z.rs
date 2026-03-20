//! Close Return Z-Score — Z-score of the current bar return vs rolling return distribution.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Return Z-Score — `(current_return - mean(returns)) / std_dev(returns)`.
///
/// Measures how unusual the current bar's close-to-close return is relative to the
/// recent distribution of returns:
/// - **High positive**: unusually strong up move.
/// - **High negative**: unusually strong down move.
/// - **Near 0**: typical move.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen (to form
/// `period` returns), or when return std dev is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseReturnZ;
/// use fin_primitives::signals::Signal;
/// let crz = CloseReturnZ::new("crz_20", 20).unwrap();
/// assert_eq!(crz.period(), 20);
/// ```
pub struct CloseReturnZ {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl CloseReturnZ {
    /// Constructs a new `CloseReturnZ`.
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
        })
    }
}

impl Signal for CloseReturnZ {
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

        let returns: Vec<f64> = self
            .closes
            .iter()
            .collect::<Vec<_>>()
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].to_f64()?;
                let curr = w[1].to_f64()?;
                if prev == 0.0 { None } else { Some((curr - prev) / prev) }
            })
            .collect();

        let n = returns.len() as f64;
        if n < 1.0 {
            return Ok(SignalValue::Unavailable);
        }

        let current_return = *returns.last().unwrap();
        let mean = returns.iter().sum::<f64>() / n;
        let variance = returns.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let z = (current_return - mean) / std_dev;
        Decimal::try_from(z)
            .map(SignalValue::Scalar)
            .or(Ok(SignalValue::Unavailable))
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_crz_invalid_period() {
        assert!(CloseReturnZ::new("crz", 0).is_err());
        assert!(CloseReturnZ::new("crz", 1).is_err());
    }

    #[test]
    fn test_crz_unavailable_before_warm_up() {
        let mut s = CloseReturnZ::new("crz", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_crz_flat_prices_unavailable() {
        let mut s = CloseReturnZ::new("crz", 3).unwrap();
        for _ in 0..4 {
            s.update_bar(&bar("100")).unwrap();
        }
        // All returns = 0 → std_dev=0 → Unavailable
        let v = s.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_crz_outlier_has_high_z() {
        let mut s = CloseReturnZ::new("crz", 4).unwrap();
        // Small returns, then a big jump
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("100.1")).unwrap();
        s.update_bar(&bar("100.2")).unwrap();
        s.update_bar(&bar("100.3")).unwrap();
        let v = s.update_bar(&bar("110")).unwrap(); // big jump
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(1), "outlier return should give high Z-score: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_crz_reset() {
        let mut s = CloseReturnZ::new("crz", 3).unwrap();
        for p in &["100", "101", "102", "103"] {
            s.update_bar(&bar(p)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
