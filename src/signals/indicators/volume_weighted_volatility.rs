//! Volume-Weighted Volatility indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted Volatility — standard deviation of close-to-close returns,
/// where each return is weighted by its bar's volume relative to the total volume
/// in the window.
///
/// ```text
/// ret[i]    = (close[i] - close[i-1]) / close[i-1]
/// w[i]      = volume[i] / sum(volume, window)
/// mean_w    = sum(w[i] × ret[i])
/// vwvol     = sqrt(sum(w[i] × (ret[i] - mean_w)^2))
/// ```
///
/// Higher-volume bars receive more weight, so the measure is dominated by periods
/// of elevated participation. Useful for detecting whether volatility is driven by
/// high-conviction moves (high volume) or low-liquidity noise.
///
/// Returns [`SignalValue::Unavailable`] until `period` returns are collected
/// (requires `period + 1` closes), or when total volume in the window is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedVolatility;
/// use fin_primitives::signals::Signal;
/// let vwv = VolumeWeightedVolatility::new("vwv_20", 20).unwrap();
/// assert_eq!(vwv.period(), 20);
/// ```
pub struct VolumeWeightedVolatility {
    name: String,
    period: usize,
    // (return, volume) pairs
    data: VecDeque<(f64, f64)>,
    prev_close: Option<f64>,
}

impl VolumeWeightedVolatility {
    /// Constructs a new `VolumeWeightedVolatility`.
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
            data: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for VolumeWeightedVolatility {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.data.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let c = bar.close.to_f64().unwrap_or(0.0);
        let v = bar.volume.to_f64().unwrap_or(0.0);

        if let Some(pc) = self.prev_close {
            if pc > 0.0 {
                let ret = (c - pc) / pc;
                self.data.push_back((ret, v));
                if self.data.len() > self.period {
                    self.data.pop_front();
                }
            }
        }
        self.prev_close = Some(c);

        if self.data.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_vol: f64 = self.data.iter().map(|(_, vol)| vol).sum();
        if total_vol == 0.0 {
            // Fall back to equal-weight standard deviation
            let rets: Vec<f64> = self.data.iter().map(|(r, _)| *r).collect();
            let n = rets.len() as f64;
            let mean = rets.iter().sum::<f64>() / n;
            let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
            let vol = var.sqrt();
            return Decimal::try_from(vol)
                .map(SignalValue::Scalar)
                .map_err(|_| FinError::ArithmeticOverflow);
        }

        let weights: Vec<f64> = self.data.iter().map(|(_, vol)| vol / total_vol).collect();
        let rets: Vec<f64> = self.data.iter().map(|(r, _)| *r).collect();

        let mean_w: f64 = weights.iter().zip(rets.iter()).map(|(w, r)| w * r).sum();
        let var_w: f64 = weights.iter().zip(rets.iter())
            .map(|(w, r)| w * (r - mean_w).powi(2))
            .sum();
        let vol = var_w.sqrt();

        Decimal::try_from(vol)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.data.clear();
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

    fn bar(c: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwv_invalid_period() {
        assert!(VolumeWeightedVolatility::new("v", 0).is_err());
        assert!(VolumeWeightedVolatility::new("v", 1).is_err());
    }

    #[test]
    fn test_vwv_unavailable_during_warmup() {
        let mut vwv = VolumeWeightedVolatility::new("v", 4).unwrap();
        for p in &["100", "101", "99", "102"] {
            assert_eq!(vwv.update_bar(&bar(p, "1000")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vwv.is_ready());
    }

    #[test]
    fn test_vwv_flat_prices_zero() {
        // Same close every bar → all returns = 0 → volatility = 0
        let mut vwv = VolumeWeightedVolatility::new("v", 3).unwrap();
        for _ in 0..4 {
            vwv.update_bar(&bar("100", "1000")).unwrap();
        }
        if let SignalValue::Scalar(v) = vwv.update_bar(&bar("100", "1000")).unwrap() {
            assert!(v == dec!(0) || v < dec!(0.0001), "flat prices → near-zero volatility: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwv_positive_output() {
        // Varying prices → positive volatility
        let mut vwv = VolumeWeightedVolatility::new("v", 3).unwrap();
        for (p, v) in &[("100","1000"),("105","2000"),("102","1500"),("108","3000")] {
            vwv.update_bar(&bar(p, v)).unwrap();
        }
        if let SignalValue::Scalar(v) = vwv.update_bar(&bar("104", "1000")).unwrap() {
            assert!(v > dec!(0), "varying prices → positive volatility: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwv_reset() {
        let mut vwv = VolumeWeightedVolatility::new("v", 3).unwrap();
        for p in &["100","102","99","104"] { vwv.update_bar(&bar(p, "100")).unwrap(); }
        assert!(vwv.is_ready());
        vwv.reset();
        assert!(!vwv.is_ready());
    }
}
