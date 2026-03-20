import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. VolumeWeightedClose
volume_weighted_close = r'''//! Volume Weighted Close indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling volume-weighted average close price (VWAP-like over N bars).
///
/// `Σ(close * volume) / Σ(volume)` over the rolling period.
/// Gives more weight to high-volume bars, smoothing noise from low-volume sessions.
/// Returns Unavailable when total volume in the window is zero.
pub struct VolumeWeightedClose {
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (close, volume)
    sum_cv: Decimal,
    sum_v: Decimal,
}

impl VolumeWeightedClose {
    /// Creates a new `VolumeWeightedClose` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            sum_cv: Decimal::ZERO,
            sum_v: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeWeightedClose {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let cv = bar.close * bar.volume;
        self.window.push_back((bar.close, bar.volume));
        self.sum_cv += cv;
        self.sum_v += bar.volume;
        if self.window.len() > self.period {
            if let Some((old_c, old_v)) = self.window.pop_front() {
                self.sum_cv -= old_c * old_v;
                self.sum_v -= old_v;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.sum_v.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum_cv / self.sum_v))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum_cv = Decimal::ZERO; self.sum_v = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeWeightedClose" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vwc_equal_volume() {
        // Equal volumes → VWC = simple average
        let mut sig = VolumeWeightedClose::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("110", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(105)));
    }

    #[test]
    fn test_vwc_higher_volume_bar() {
        // Second bar has 4x volume → VWC biased toward 110
        // (100*1 + 110*4) / 5 = (100 + 440) / 5 = 540/5 = 108
        let mut sig = VolumeWeightedClose::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("110", "4000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(108)));
    }
}
'''

# 2. CloseAbovePrevHigh
close_above_prev_high = r'''//! Close Above Previous High indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where `close > previous bar's high`.
///
/// Signals breakout strength — how often price closes above the prior bar's high.
/// High values indicate persistent bullish breakouts over the period.
pub struct CloseAbovePrevHigh {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAbovePrevHigh {
    /// Creates a new `CloseAbovePrevHigh` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for CloseAbovePrevHigh {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let hit: u8 = if bar.close > ph { 1 } else { 0 };
            self.window.push_back(hit);
            self.count += hit as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_high = Some(bar.high);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "CloseAbovePrevHigh" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: h.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_caph_all_breakouts() {
        // Each close beats previous high
        let mut sig = CloseAbovePrevHigh::new(3).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_high=100
        sig.update(&bar("105", "105")).unwrap(); // close(105) > prev_high(100) ✓
        sig.update(&bar("110", "110")).unwrap(); // close(110) > prev_high(105) ✓
        let v = sig.update(&bar("115", "115")).unwrap(); // close(115) > prev_high(110) ✓ → count=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_caph_no_breakouts() {
        // Close never beats previous high
        let mut sig = CloseAbovePrevHigh::new(3).unwrap();
        sig.update(&bar("110", "100")).unwrap(); // seeds prev_high=110
        sig.update(&bar("110", "100")).unwrap(); // close(100) <= prev_high(110)
        sig.update(&bar("110", "100")).unwrap(); // close(100) <= prev_high(110)
        let v = sig.update(&bar("110", "100")).unwrap(); // close(100) <= prev_high(110) → count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. MeanReversionScore
mean_reversion_score = r'''//! Mean Reversion Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Z-score of current close relative to rolling SMA and standard deviation.
///
/// `(close - SMA) / std_dev(closes)`
/// Positive: price above mean (potential sell signal in mean-reverting markets).
/// Negative: price below mean (potential buy signal in mean-reverting markets).
/// Returns Scalar(0) when std_dev is zero (flat price series).
pub struct MeanReversionScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl MeanReversionScore {
    /// Creates a new `MeanReversionScore` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for MeanReversionScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let vals: Vec<f64> = self.window.iter().filter_map(|v| v.to_f64()).collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let current = bar.close.to_f64().unwrap_or(mean);
        let z = (current - mean) / std_dev;
        match Decimal::from_f64_retain(z) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "MeanReversionScore" }
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
    fn test_mrs_flat_is_zero() {
        // Constant price → z-score = 0
        let mut sig = MeanReversionScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mrs_above_mean_positive() {
        // [100, 100, 110] → mean≈103.3, current=110 → z > 0
        let mut sig = MeanReversionScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("110")).unwrap() {
            assert!(v > dec!(0), "expected positive z-score, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 4. RangePersistence
range_persistence = r'''//! Range Persistence indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where current range exceeds previous range.
///
/// Measures how often intraday ranges are expanding.
/// High values suggest sustained volatility expansion.
/// Low values suggest contracting or stable volatility.
pub struct RangePersistence {
    period: usize,
    prev_range: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl RangePersistence {
    /// Creates a new `RangePersistence` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_range: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for RangePersistence {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if let Some(pr) = self.prev_range {
            let expanded: u8 = if range > pr { 1 } else { 0 };
            self.window.push_back(expanded);
            self.count += expanded as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_range = Some(range);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_range = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "RangePersistence" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_rp_all_expanding() {
        // Each bar's range is larger than previous
        let mut sig = RangePersistence::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // range=20, seeds
        sig.update(&bar("115", "85")).unwrap();  // range=30 > 20 ✓
        sig.update(&bar("120", "80")).unwrap();  // range=40 > 30 ✓
        let v = sig.update(&bar("125", "75")).unwrap(); // range=50 > 40 ✓, count=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_rp_no_expansion() {
        // Each bar's range equals previous (not strictly greater)
        let mut sig = RangePersistence::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // range=20
        sig.update(&bar("110", "90")).unwrap();  // range=20, not > 20
        sig.update(&bar("110", "90")).unwrap();  // range=20
        let v = sig.update(&bar("110", "90")).unwrap(); // count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("volume_weighted_close.rs", volume_weighted_close),
    ("close_above_prev_high.rs", close_above_prev_high),
    ("mean_reversion_score.rs", mean_reversion_score),
    ("range_persistence.rs", range_persistence),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
