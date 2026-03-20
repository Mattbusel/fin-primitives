import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. PriceEfficiencyRatio (Kaufman's ER)
price_efficiency_ratio = r'''//! Price Efficiency Ratio indicator (Kaufman's ER).

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Kaufman's Efficiency Ratio: `|close[t] - close[t-N]| / sum(|close[i] - close[i-1]|)`.
///
/// Measures how efficiently price moves. 1 = straight-line trend, ~0 = choppy/noisy market.
pub struct PriceEfficiencyRatio {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceEfficiencyRatio {
    /// Creates a new `PriceEfficiencyRatio` with the given period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for PriceEfficiencyRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let net = (self.closes.back().unwrap() - self.closes.front().unwrap()).abs();
        let path: Decimal = self.closes.iter()
            .zip(self.closes.iter().skip(1))
            .map(|(a, b)| (*b - *a).abs())
            .sum();
        if path.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(net / path))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "PriceEfficiencyRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_efficiency_ratio_straight_line() {
        // Monotonic trend: path == net => ER = 1
        let mut sig = PriceEfficiencyRatio::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_efficiency_ratio_choppy() {
        // Up-down alternating: net < path => ER < 1
        let mut sig = PriceEfficiencyRatio::new(4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        // net = 0, path = 8
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 2. UpVolumeRatio
up_volume_ratio = r'''//! Up-Volume Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of total volume that occurs on up bars (close > open).
///
/// Values > 0.5 indicate buying pressure dominating. Values < 0.5 indicate selling pressure.
pub struct UpVolumeRatio {
    period: usize,
    window: VecDeque<(Decimal, bool)>, // (volume, is_up)
    total_vol: Decimal,
    up_vol: Decimal,
}

impl UpVolumeRatio {
    /// Creates a new `UpVolumeRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            total_vol: Decimal::ZERO,
            up_vol: Decimal::ZERO,
        })
    }
}

impl Signal for UpVolumeRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let is_up = bar.close > bar.open;
        self.window.push_back((bar.volume, is_up));
        self.total_vol += bar.volume;
        if is_up { self.up_vol += bar.volume; }

        if self.window.len() > self.period {
            if let Some((ov, ou)) = self.window.pop_front() {
                self.total_vol -= ov;
                if ou { self.up_vol -= ov; }
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.total_vol.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(self.up_vol / self.total_vol))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.total_vol = Decimal::ZERO; self.up_vol = Decimal::ZERO; }
    fn name(&self) -> &str { "UpVolumeRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, v: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(200),
            low: dec!(1),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_up_volume_ratio_all_up() {
        let mut sig = UpVolumeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "1000")).unwrap();
        let v = sig.update(&bar("100", "110", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_up_volume_ratio_mixed() {
        let mut sig = UpVolumeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "1000")).unwrap(); // up 1000
        let v = sig.update(&bar("110", "100", "1000")).unwrap(); // down 1000
        // up_vol = 1000, total = 2000, ratio = 0.5
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
'''

# 3. VolatilityAdjustedMomentum
volatility_adjusted_momentum = r'''//! Volatility-Adjusted Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Momentum scaled by rolling volatility: `(close[t] - close[t-N]) / std_dev(returns)`.
///
/// Normalizes raw momentum by recent volatility so readings are comparable
/// across different market regimes and instruments.
pub struct VolatilityAdjustedMomentum {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl VolatilityAdjustedMomentum {
    /// Creates a new `VolatilityAdjustedMomentum` with the given period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for VolatilityAdjustedMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // Compute returns
        let returns: Vec<f64> = self.closes.iter()
            .zip(self.closes.iter().skip(1))
            .filter_map(|(a, b)| {
                if a.is_zero() { return None; }
                ((*b - *a) / *a).to_f64()
            })
            .collect();

        if returns.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;
        let var = returns.iter().map(|r| { let d = r - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let oldest = *self.closes.front().unwrap();
        let newest = *self.closes.back().unwrap();
        let mom = match (newest - oldest).to_f64() {
            Some(m) => m,
            None => return Ok(SignalValue::Unavailable),
        };

        let adj = mom / std_dev;
        match Decimal::from_f64_retain(adj) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "VolatilityAdjustedMomentum" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_vam_not_ready() {
        let mut sig = VolatilityAdjustedMomentum::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vam_constant_prices_zero() {
        // No momentum, no volatility => result = 0
        let mut sig = VolatilityAdjustedMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. TrueRangePercentile
true_range_percentile = r'''//! True Range Percentile indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentile rank of the current True Range within the period window.
///
/// Returns 0-100: how large the current TR is relative to the last N bars.
/// Useful for identifying unusually high or low volatility bars.
pub struct TrueRangePercentile {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl TrueRangePercentile {
    /// Creates a new `TrueRangePercentile` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for TrueRangePercentile {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => bar.high - bar.low,
            Some(pc) => {
                let hl = bar.high - bar.low;
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);

        self.window.push_back(tr);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let current = tr;
        let below = self.window.iter().filter(|&&v| v < current).count();
        let pct = Decimal::from(below as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); }
    fn name(&self) -> &str { "TrueRangePercentile" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_tr_percentile_largest() {
        // All bars same TR except last which is largest
        let mut sig = TrueRangePercentile::new(3).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap(); // TR=20
        sig.update(&bar("110", "90", "100")).unwrap(); // TR=20
        let v = sig.update(&bar("130", "70", "100")).unwrap(); // TR=60 -> largest
        // 2 values below 60 out of 3 => 2/3 * 100 ≈ 66.6
        if let SignalValue::Scalar(x) = v {
            assert!(x > dec!(50), "largest TR should be in high percentile, got {}", x);
        }
    }

    #[test]
    fn test_tr_percentile_smallest() {
        let mut sig = TrueRangePercentile::new(3).unwrap();
        sig.update(&bar("130", "70", "100")).unwrap(); // TR=60
        sig.update(&bar("130", "70", "100")).unwrap(); // TR=60
        let v = sig.update(&bar("110", "90", "100")).unwrap(); // TR=20 -> smallest
        // 0 values below 20 => 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("price_efficiency_ratio.rs", price_efficiency_ratio),
    ("up_volume_ratio.rs", up_volume_ratio),
    ("volatility_adjusted_momentum.rs", volatility_adjusted_momentum),
    ("true_range_percentile.rs", true_range_percentile),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
