//! Price Volatility Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Volatility Ratio — the ratio of price return to its rolling volatility
/// (standard deviation of returns), providing a normalized measure of the current
/// bar's return significance.
///
/// ```text
/// ret[i]   = (close[i] - close[i-1]) / close[i-1]
/// std_dev  = StdDev(ret[t-period+1 .. t-1])    (prior `period` returns)
/// pvr[t]   = ret[t] / std_dev
/// ```
///
/// - **|pvr| > 2**: current return is more than 2 standard deviations — unusual.
/// - **|pvr| ≈ 0**: current return is within normal bounds.
/// - Positive: up-move; Negative: down-move.
///
/// This is similar to a z-score of the current return against its historical
/// distribution, but uses the *prior* period's std dev as the denominator,
/// making it a forward-looking surprise metric rather than a retrospective one.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` closes are collected,
/// or when the rolling standard deviation is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceVolatilityRatio;
/// use fin_primitives::signals::Signal;
/// let pvr = PriceVolatilityRatio::new("pvr_20", 20).unwrap();
/// assert_eq!(pvr.period(), 20);
/// ```
pub struct PriceVolatilityRatio {
    name: String,
    period: usize,
    prior_returns: VecDeque<f64>,
    prev_close: Option<f64>,
}

impl PriceVolatilityRatio {
    /// Constructs a new `PriceVolatilityRatio`.
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
            prior_returns: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for PriceVolatilityRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.prior_returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let c = bar.close.to_f64().unwrap_or(0.0);

        // Compute current return
        let current_ret = match self.prev_close {
            Some(pc) if pc > 0.0 => Some((c - pc) / pc),
            _ => None,
        };

        // Compute ratio using PRIOR returns (before adding current)
        let result = if self.prior_returns.len() >= self.period {
            if let Some(ret) = current_ret {
                let n = self.prior_returns.len() as f64;
                let mean = self.prior_returns.iter().sum::<f64>() / n;
                let var = self.prior_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
                let std_dev = var.sqrt();
                if std_dev == 0.0 {
                    SignalValue::Unavailable
                } else {
                    let pvr = ret / std_dev;
                    match Decimal::try_from(pvr) {
                        Ok(d) => SignalValue::Scalar(d),
                        Err(_) => return Err(FinError::ArithmeticOverflow),
                    }
                }
            } else {
                SignalValue::Unavailable
            }
        } else {
            SignalValue::Unavailable
        };

        // Now add current return to the rolling window
        if let Some(ret) = current_ret {
            self.prior_returns.push_back(ret);
            if self.prior_returns.len() > self.period {
                self.prior_returns.pop_front();
            }
        }
        self.prev_close = Some(c);

        Ok(result)
    }

    fn reset(&mut self) {
        self.prior_returns.clear();
        self.prev_close = None;
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
    fn test_pvr_invalid_period() {
        assert!(PriceVolatilityRatio::new("pvr", 0).is_err());
        assert!(PriceVolatilityRatio::new("pvr", 1).is_err());
    }

    #[test]
    fn test_pvr_unavailable_during_warmup() {
        // period=4: needs period+1=5 bars to collect period returns (first bar produces no return)
        let mut pvr = PriceVolatilityRatio::new("pvr", 4).unwrap();
        for p in &["100", "101", "99", "102"] {
            assert_eq!(pvr.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!pvr.is_ready());
        // 5th bar: prior_returns now has period=4 returns → is_ready() = true
        pvr.update_bar(&bar("100")).unwrap();
        assert!(pvr.is_ready());
    }

    #[test]
    fn test_pvr_large_move_high_ratio() {
        // Establish a volatile baseline, then a normal return → small ratio
        // Establish a stable baseline, then a huge move → large |ratio|
        let mut pvr = PriceVolatilityRatio::new("pvr", 4).unwrap();
        // Small oscillation to build baseline: 0.1% moves
        for p in &["100", "100.1", "100", "100.1", "100"] {
            pvr.update_bar(&bar(p)).unwrap();
        }
        // Now a large move: +10%
        if let SignalValue::Scalar(v) = pvr.update_bar(&bar("110")).unwrap() {
            assert!(v.abs() > dec!(2), "large move against low-vol baseline → |pvr| >> 2: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pvr_flat_baseline_unavailable() {
        // Perfectly flat prices → std_dev = 0 → Unavailable
        let mut pvr = PriceVolatilityRatio::new("pvr", 3).unwrap();
        for _ in 0..6 {
            pvr.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(pvr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pvr_reset() {
        let mut pvr = PriceVolatilityRatio::new("pvr", 3).unwrap();
        for p in &["100", "101", "99", "102", "100"] { pvr.update_bar(&bar(p)).unwrap(); }
        assert!(pvr.is_ready());
        pvr.reset();
        assert!(!pvr.is_ready());
    }
}
