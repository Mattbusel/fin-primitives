import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. OpenGapSize
open_gap_size = r'''//! Open Gap Size indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `|open - prev_close| / prev_close * 100`.
///
/// Measures the average magnitude of opening gaps (overnight moves).
/// Does not distinguish direction — use `CloseToOpenReturn` for directional gaps.
pub struct OpenGapSize {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenGapSize {
    /// Creates a new `OpenGapSize` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenGapSize {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let gap = (bar.open - pc).abs() / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(gap);
                self.sum += gap;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() {
                        self.sum -= old;
                    }
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "OpenGapSize" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(200),
            low: dec!(1),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_open_gap_size_no_gap() {
        let mut sig = OpenGapSize::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_close=100
        sig.update(&bar("100", "100")).unwrap(); // gap=0
        let v = sig.update(&bar("100", "100")).unwrap(); // gap=0, avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_open_gap_size_symmetric() {
        let mut sig = OpenGapSize::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_close=100
        sig.update(&bar("102", "100")).unwrap(); // gap=2%, seeds prev_close=100
        let v = sig.update(&bar("98", "100")).unwrap();  // gap=2%, avg=2%
        if let SignalValue::Scalar(x) = v {
            assert!((x - dec!(2)).abs() < dec!(0.001), "avg gap should be 2%, got {}", x);
        }
    }
}
'''

# 2. RangeZScore
range_z_score = r'''//! Range Z-Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Z-score of the current bar's range relative to the rolling window.
///
/// `(range - mean_range) / std_dev_range` — measures how unusual the current
/// bar's volatility is. +2 means an unusually wide bar; -2 means unusually narrow.
pub struct RangeZScore {
    period: usize,
    window: VecDeque<Decimal>,
}

impl RangeZScore {
    /// Creates a new `RangeZScore` with the given rolling period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for RangeZScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.window.iter()
            .filter_map(|r| r.to_f64())
            .collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let current = match range.to_f64() {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };
        let z = (current - mean) / std_dev;
        match Decimal::from_f64_retain(z) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "RangeZScore" }
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
    fn test_range_z_score_not_ready() {
        let mut sig = RangeZScore::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_range_z_score_constant_zero() {
        // All same range → std_dev = 0 → z = 0
        let mut sig = RangeZScore::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. BearishBarRatio
bearish_bar_ratio = r'''//! Bearish Bar Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of bars where `close < open` (bearish/down bars).
///
/// Complement of bullish bar ratio. High values suggest persistent selling pressure.
pub struct BearishBarRatio {
    period: usize,
    window: VecDeque<u8>,
    count: usize,
}

impl BearishBarRatio {
    /// Creates a new `BearishBarRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for BearishBarRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let bearish: u8 = if bar.close < bar.open { 1 } else { 0 };
        self.window.push_back(bearish);
        self.count += bearish as usize;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.count -= old as usize;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = Decimal::from(self.count as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "BearishBarRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(200),
            low: dec!(1),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_bearish_bar_ratio_all_bearish() {
        let mut sig = BearishBarRatio::new(3).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        let v = sig.update(&bar("110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_bar_ratio_none_bearish() {
        let mut sig = BearishBarRatio::new(3).unwrap();
        sig.update(&bar("100", "110")).unwrap();
        sig.update(&bar("100", "110")).unwrap();
        let v = sig.update(&bar("100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bearish_bar_ratio_half() {
        let mut sig = BearishBarRatio::new(2).unwrap();
        sig.update(&bar("100", "110")).unwrap(); // bullish
        let v = sig.update(&bar("110", "100")).unwrap(); // bearish → 1/2
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
'''

# 4. CloseBelowLowPrev
close_below_low_prev = r'''//! Close-Below-Prior-Low indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentage of bars where `close < prior bar low`.
///
/// Measures breakdown frequency — how often price closes below the previous bar's low.
/// High values indicate persistent downside breakdowns.
pub struct CloseBelowLowPrev {
    period: usize,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseBelowLowPrev {
    /// Creates a new `CloseBelowLowPrev` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_low: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for CloseBelowLowPrev {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pl) = self.prev_low {
            let below: u8 = if bar.close < pl { 1 } else { 0 };
            self.window.push_back(below);
            self.count += below as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let pct = Decimal::from(self.count as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_low = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "CloseBelowLowPrev" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(200),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_close_below_low_prev_always_below() {
        let mut sig = CloseBelowLowPrev::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_low=100
        sig.update(&bar("90", "95")).unwrap(); // 95 < 100 ✓, seeds prev_low=90
        let v = sig.update(&bar("80", "85")).unwrap(); // 85 < 90 ✓ → 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_close_below_low_prev_never_below() {
        let mut sig = CloseBelowLowPrev::new(2).unwrap();
        sig.update(&bar("80", "90")).unwrap(); // seeds prev_low=80
        sig.update(&bar("85", "95")).unwrap(); // 95 > 80 ✗
        let v = sig.update(&bar("90", "100")).unwrap(); // 100 > 85 ✗ → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("open_gap_size.rs", open_gap_size),
    ("range_z_score.rs", range_z_score),
    ("bearish_bar_ratio.rs", bearish_bar_ratio),
    ("close_below_low_prev.rs", close_below_low_prev),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
