import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. CloseVsOpenRange (body position in range)
close_vs_open_range = r'''//! Close-vs-Open Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - open) / (high - low)`.
///
/// Measures where the close sits relative to the bar's range, normalized by direction:
/// - +1.0 = close at high (full bullish body, no upper wick)
/// - -1.0 = close at low (full bearish body, no lower wick)
/// - 0.0 = close at midpoint
///
/// Bars with zero range are skipped.
pub struct CloseVsOpenRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseVsOpenRange {
    /// Creates a new `CloseVsOpenRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseVsOpenRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if !range.is_zero() {
            let ratio = (bar.close - bar.open) / range;
            self.window.push_back(ratio);
            self.sum += ratio;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.window.len() as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "CloseVsOpenRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_close_vs_open_range_full_bull() {
        // open=low, close=high → (close-open)/(high-low) = range/range = 1
        let mut sig = CloseVsOpenRange::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_close_vs_open_range_full_bear() {
        // open=high, close=low → (close-open)/(high-low) = -range/range = -1
        let mut sig = CloseVsOpenRange::new(2).unwrap();
        sig.update(&bar("110", "110", "90", "90")).unwrap();
        let v = sig.update(&bar("110", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }
}
'''

# 2. VolatilityRegimeDetector
volatility_regime_detector = r'''//! Volatility Regime Detector indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Ratio of short-term return std dev to long-term return std dev.
///
/// Values > 1 indicate a high-volatility regime (current vol > baseline).
/// Values < 1 indicate a low-volatility / quiet regime.
/// `short_period` must be less than `long_period`.
pub struct VolatilityRegimeDetector {
    short_period: usize,
    long_period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl VolatilityRegimeDetector {
    /// Creates a new `VolatilityRegimeDetector`.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period < 2 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            short_period,
            long_period,
            prev_close: None,
            returns: VecDeque::with_capacity(long_period),
        })
    }

    fn std_dev(vals: &[f64]) -> f64 {
        let n = vals.len() as f64;
        if n < 2.0 { return 0.0; }
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        var.sqrt()
    }
}

impl Signal for VolatilityRegimeDetector {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc;
                self.returns.push_back(ret);
                if self.returns.len() > self.long_period {
                    self.returns.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.returns.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let all: Vec<f64> = self.returns.iter().filter_map(|r| r.to_f64()).collect();
        if all.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let short_vals = &all[all.len() - self.short_period..];
        let long_std = Self::std_dev(&all);
        let short_std = Self::std_dev(short_vals);

        if long_std == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }

        let ratio = short_std / long_std;
        match Decimal::from_f64_retain(ratio) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "VolatilityRegimeDetector" }
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
    fn test_vrd_not_ready() {
        let mut sig = VolatilityRegimeDetector::new(3, 6).unwrap();
        for _ in 0..6 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vrd_constant_returns_one() {
        // Constant prices → all std devs = 0 → returns 1.0
        let mut sig = VolatilityRegimeDetector::new(3, 6).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..7 {
            last = sig.update(&bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }
}
'''

# 3. ReturnSignSum
return_sign_sum = r'''//! Return Sign Sum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of return signs: +1 (up bar), -1 (down bar), 0 (flat).
///
/// Also known as "directional bias" — measures the net number of up vs down moves.
/// Positive values indicate bullish dominance; negative values indicate bearish dominance.
pub struct ReturnSignSum {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<i8>,
    sum: i32,
}

impl ReturnSignSum {
    /// Creates a new `ReturnSignSum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: 0 })
    }
}

impl Signal for ReturnSignSum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let sign: i8 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            self.window.push_back(sign);
            self.sum += sign as i32;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old as i32;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.sum)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = 0; }
    fn name(&self) -> &str { "ReturnSignSum" }
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
    fn test_return_sign_sum_all_up() {
        let mut sig = ReturnSignSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap(); // window=[+1,+1,+1], sum=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_return_sign_sum_mixed() {
        let mut sig = ReturnSignSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap(); // +1
        sig.update(&bar("100")).unwrap(); // -1
        let v = sig.update(&bar("101")).unwrap(); // +1, window=[+1,-1,+1], sum=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
'''

# 4. CloseToRangeTop
close_to_range_top = r'''//! Close-to-Range-Top indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(high - close) / (high - low) * 100`.
///
/// Measures how far the close is from the bar high as a percentage of total range.
/// Low values (near 0) indicate closes near the high (bullish strength).
/// High values (near 100) indicate closes near the low (bearish weakness).
pub struct CloseToRangeTop {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToRangeTop {
    /// Creates a new `CloseToRangeTop` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToRangeTop {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.high - bar.close) / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(ratio);
        self.sum += ratio;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "CloseToRangeTop" }
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
    fn test_close_to_range_top_at_high() {
        // close = high → (high-close)/range = 0
        let mut sig = CloseToRangeTop::new(2).unwrap();
        sig.update(&bar("110", "90", "110")).unwrap();
        let v = sig.update(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_close_to_range_top_at_low() {
        // close = low → (high-close)/range = 1 → 100%
        let mut sig = CloseToRangeTop::new(2).unwrap();
        sig.update(&bar("110", "90", "90")).unwrap();
        let v = sig.update(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
'''

files = [
    ("close_vs_open_range.rs", close_vs_open_range),
    ("volatility_regime_detector.rs", volatility_regime_detector),
    ("return_sign_sum.rs", return_sign_sum),
    ("close_to_range_top.rs", close_to_range_top),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
