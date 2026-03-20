import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. WeightedCloseVolatility
weighted_close_volatility = r'''//! Weighted Close Volatility indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Rolling standard deviation of the weighted close: `(high + low + 2*close) / 4`.
///
/// Gives double weight to the close price, making it more responsive to
/// closing price changes while still incorporating intra-bar range information.
pub struct WeightedCloseVolatility {
    period: usize,
    window: VecDeque<Decimal>,
}

impl WeightedCloseVolatility {
    /// Creates a new `WeightedCloseVolatility` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for WeightedCloseVolatility {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let wc = (bar.high + bar.low + bar.close + bar.close) / Decimal::from(4u32);
        self.window.push_back(wc);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.window.iter()
            .filter_map(|v| v.to_f64())
            .collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);

        match Decimal::from_f64_retain(var.sqrt()) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "WeightedCloseVolatility" }
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
    fn test_wcv_not_ready() {
        let mut sig = WeightedCloseVolatility::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_wcv_constant_zero() {
        // Identical bars → WC constant → std_dev = 0
        let mut sig = WeightedCloseVolatility::new(3).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 2. VolumeStreakCount
volume_streak_count = r'''//! Volume Streak Count indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Streak count of consecutive bars with increasing (+) or decreasing (-) volume.
///
/// - Positive values: N consecutive bars of increasing volume
/// - Negative values: N consecutive bars of decreasing volume
/// - 0: volume unchanged from prior bar (resets streak)
/// - Always ready after the first bar.
pub struct VolumeStreakCount {
    prev_vol: Option<Decimal>,
    streak: i32,
}

impl VolumeStreakCount {
    /// Creates a new `VolumeStreakCount`.
    pub fn new() -> Self {
        Self { prev_vol: None, streak: 0 }
    }
}

impl Default for VolumeStreakCount {
    fn default() -> Self { Self::new() }
}

impl Signal for VolumeStreakCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pv) = self.prev_vol {
            if bar.volume > pv {
                self.streak = if self.streak > 0 { self.streak + 1 } else { 1 };
            } else if bar.volume < pv {
                self.streak = if self.streak < 0 { self.streak - 1 } else { -1 };
            } else {
                self.streak = 0;
            }
        }
        self.prev_vol = Some(bar.volume);
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn is_ready(&self) -> bool { true }
    fn period(&self) -> usize { 1 }
    fn reset(&mut self) { self.prev_vol = None; self.streak = 0; }
    fn name(&self) -> &str { "VolumeStreakCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: dec!(90),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vsc_increasing_streak() {
        let mut sig = VolumeStreakCount::new();
        sig.update(&bar("1000")).unwrap(); // first bar, streak=0
        sig.update(&bar("1100")).unwrap(); // +1 streak
        sig.update(&bar("1200")).unwrap(); // +2 streak
        let v = sig.update(&bar("1300")).unwrap(); // +3 streak
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_vsc_decreasing_streak() {
        let mut sig = VolumeStreakCount::new();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("900")).unwrap(); // -1
        let v = sig.update(&bar("800")).unwrap(); // -2
        assert_eq!(v, SignalValue::Scalar(dec!(-2)));
    }

    #[test]
    fn test_vsc_reset_on_equal() {
        let mut sig = VolumeStreakCount::new();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1100")).unwrap(); // +1
        let v = sig.update(&bar("1100")).unwrap(); // equal → 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. MomentumReversal
momentum_reversal = r'''//! Momentum Reversal indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where the return sign flips from the prior bar.
///
/// Measures how often the market reverses direction. High values indicate
/// a choppy, mean-reverting market; low values indicate trending behaviour.
pub struct MomentumReversal {
    period: usize,
    prev_close: Option<Decimal>,
    prev_sign: i8,
    window: VecDeque<u8>,
    count: usize,
}

impl MomentumReversal {
    /// Creates a new `MomentumReversal` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, prev_sign: 0, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for MomentumReversal {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let sign: i8 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            // Reversal = sign changed AND both are non-zero
            let reversed: u8 = if sign != 0 && self.prev_sign != 0 && sign != self.prev_sign { 1 } else { 0 };
            self.window.push_back(reversed);
            self.count += reversed as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
            self.prev_sign = sign;
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.prev_sign = 0; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "MomentumReversal" }
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
    fn test_momentum_reversal_alternating() {
        let mut sig = MomentumReversal::new(3).unwrap();
        sig.update(&bar("100")).unwrap(); // seeds
        sig.update(&bar("102")).unwrap(); // +1, prev_sign=+1
        sig.update(&bar("100")).unwrap(); // -1, reversal=1
        sig.update(&bar("102")).unwrap(); // +1, reversal=1
        let v = sig.update(&bar("100")).unwrap(); // -1, reversal=1, window=[1,1,1]=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_momentum_reversal_trending() {
        let mut sig = MomentumReversal::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap(); // +1
        sig.update(&bar("102")).unwrap(); // +1, no reversal
        sig.update(&bar("103")).unwrap(); // +1, no reversal
        let v = sig.update(&bar("104")).unwrap(); // +1, no reversal, count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. CloseToMidRange
close_to_mid_range = r'''//! Close-to-Mid-Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `close - (high + low) / 2`.
///
/// Measures whether the close tends to finish above or below the midpoint of the bar.
/// Positive = close bias toward high (bullish intra-bar momentum).
/// Negative = close bias toward low (bearish intra-bar momentum).
pub struct CloseToMidRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToMidRange {
    /// Creates a new `CloseToMidRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToMidRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::TWO;
        let diff = bar.close - mid;
        self.window.push_back(diff);
        self.sum += diff;
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
    fn name(&self) -> &str { "CloseToMidRange" }
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
    fn test_close_to_mid_at_high() {
        // close = high = 110, mid = 100 → diff = 10
        let mut sig = CloseToMidRange::new(2).unwrap();
        sig.update(&bar("110", "90", "110")).unwrap();
        let v = sig.update(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_close_to_mid_at_mid() {
        // close = midpoint → diff = 0
        let mut sig = CloseToMidRange::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("weighted_close_volatility.rs", weighted_close_volatility),
    ("volume_streak_count.rs", volume_streak_count),
    ("momentum_reversal.rs", momentum_reversal),
    ("close_to_mid_range.rs", close_to_mid_range),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
