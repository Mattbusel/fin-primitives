import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. VolumeAccumulation
volume_accumulation = r'''//! Volume Accumulation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of signed volume: `+volume` on up bars, `-volume` on down bars.
///
/// Similar to On-Balance Volume (OBV) but windowed over N bars.
/// Positive: net buying pressure over the window.
/// Negative: net selling pressure over the window.
/// Flat bars contribute 0.
pub struct VolumeAccumulation {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeAccumulation {
    /// Creates a new `VolumeAccumulation` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeAccumulation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let signed_vol = if bar.close > pc {
                bar.volume
            } else if bar.close < pc {
                -bar.volume
            } else {
                Decimal::ZERO
            };
            self.window.push_back(signed_vol);
            self.sum += signed_vol;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeAccumulation" }
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
    fn test_va_all_up() {
        // All up bars → sum = total volume
        let mut sig = VolumeAccumulation::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap(); // +1000
        let v = sig.update(&bar("102", "1000")).unwrap(); // +1000, sum=2000
        assert_eq!(v, SignalValue::Scalar(dec!(2000)));
    }

    #[test]
    fn test_va_all_down() {
        // All down bars → sum = -total volume
        let mut sig = VolumeAccumulation::new(2).unwrap();
        sig.update(&bar("102", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap(); // -1000
        let v = sig.update(&bar("100", "1000")).unwrap(); // -1000, sum=-2000
        assert_eq!(v, SignalValue::Scalar(dec!(-2000)));
    }
}
'''

# 2. CloseDistanceFromEMA
close_distance_from_ema = r'''//! Close Distance From EMA indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Signed distance of close from its EMA: `(close - EMA) / EMA * 100`.
///
/// Positive: close above EMA (bullish momentum).
/// Negative: close below EMA (bearish momentum).
/// Uses standard EMA smoothing: `k = 2 / (period + 1)`.
pub struct CloseDistanceFromEma {
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    warm_up: usize,
    warm_up_sum: Decimal,
}

impl CloseDistanceFromEma {
    /// Creates a new `CloseDistanceFromEma` with the given smoothing period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self { period, k, ema: None, warm_up: 0, warm_up_sum: Decimal::ZERO })
    }
}

impl Signal for CloseDistanceFromEma {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ema = match self.ema {
            None => {
                self.warm_up_sum += bar.close;
                self.warm_up += 1;
                if self.warm_up >= self.period {
                    let seed = self.warm_up_sum / Decimal::from(self.period as u32);
                    self.ema = Some(seed);
                    seed
                } else {
                    return Ok(SignalValue::Unavailable);
                }
            }
            Some(prev) => {
                let new_ema = bar.close * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(new_ema);
                new_ema
            }
        };

        if ema.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar((bar.close - ema) / ema * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.ema.is_some() }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.ema = None; self.warm_up = 0; self.warm_up_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "CloseDistanceFromEma" }
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
    fn test_cdfe_seed_bar_zero_distance() {
        // After warmup, close = EMA → distance = 0
        let mut sig = CloseDistanceFromEma::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap(); // EMA=100, close=100 → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cdfe_above_ema_positive() {
        // Price surges above EMA → positive distance
        let mut sig = CloseDistanceFromEma::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap(); // EMA seeded at 100
        if let SignalValue::Scalar(v) = sig.update(&bar("120")).unwrap() {
            // EMA updated: 120*(2/3) + 100*(1/3) = 80+33.33 = 113.33..., distance = (120-113.33)/113.33*100 > 0
            assert!(v > dec!(0), "expected positive, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 3. UpVolumeFraction
up_volume_fraction = r'''//! Up Volume Fraction indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of total volume on up bars: `sum(up_volume) / sum(all_volume)`.
///
/// Values near 1.0: most volume transacted on up bars (accumulation/bullish).
/// Values near 0.0: most volume transacted on down bars (distribution/bearish).
/// Values near 0.5: balanced volume distribution.
pub struct UpVolumeFraction {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<(Decimal, Decimal)>, // (up_vol, total_vol)
    up_sum: Decimal,
    total_sum: Decimal,
}

impl UpVolumeFraction {
    /// Creates a new `UpVolumeFraction` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            up_sum: Decimal::ZERO,
            total_sum: Decimal::ZERO,
        })
    }
}

impl Signal for UpVolumeFraction {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let up_vol = if bar.close > pc { bar.volume } else { Decimal::ZERO };
            self.window.push_back((up_vol, bar.volume));
            self.up_sum += up_vol;
            self.total_sum += bar.volume;
            if self.window.len() > self.period {
                if let Some((old_up, old_total)) = self.window.pop_front() {
                    self.up_sum -= old_up;
                    self.total_sum -= old_total;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.total_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.up_sum / self.total_sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.up_sum = Decimal::ZERO;
        self.total_sum = Decimal::ZERO;
    }
    fn name(&self) -> &str { "UpVolumeFraction" }
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
    fn test_uvf_all_up() {
        // All up bars → fraction = 1
        let mut sig = UpVolumeFraction::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap();
        let v = sig.update(&bar("102", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_uvf_all_down() {
        // All down bars → up_vol = 0 → fraction = 0
        let mut sig = UpVolumeFraction::new(2).unwrap();
        sig.update(&bar("102", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap();
        let v = sig.update(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. CloseAccelerationSign
close_acceleration_sign = r'''//! Close Acceleration Sign indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Sign of price acceleration: +1 if momentum increasing, -1 if decreasing, 0 if flat.
///
/// Requires 3 bars. Let:
/// - `d1 = close[t] - close[t-1]` (current velocity)
/// - `d2 = close[t-1] - close[t-2]` (previous velocity)
/// - Returns sign of `d1 - d2` (acceleration)
///
/// +1: price accelerating upward (or decelerating downward).
/// -1: price decelerating upward (or accelerating downward).
///  0: constant velocity (linear movement).
pub struct CloseAccelerationSign {
    c0: Option<Decimal>,
    c1: Option<Decimal>,
}

impl CloseAccelerationSign {
    /// Creates a new `CloseAccelerationSign` indicator.
    pub fn new() -> Self {
        Self { c0: None, c1: None }
    }
}

impl Default for CloseAccelerationSign {
    fn default() -> Self { Self::new() }
}

impl Signal for CloseAccelerationSign {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let (Some(c0), Some(c1)) = (self.c0, self.c1) {
            let d1 = bar.close - c1;
            let d2 = c1 - c0;
            let accel = d1 - d2;
            let sign: i32 = if accel > Decimal::ZERO { 1 } else if accel < Decimal::ZERO { -1 } else { 0 };
            SignalValue::Scalar(Decimal::from(sign))
        } else {
            SignalValue::Unavailable
        };
        self.c0 = self.c1;
        self.c1 = Some(bar.close);
        Ok(result)
    }

    fn is_ready(&self) -> bool { self.c0.is_some() && self.c1.is_some() }
    fn period(&self) -> usize { 3 }
    fn reset(&mut self) { self.c0 = None; self.c1 = None; }
    fn name(&self) -> &str { "CloseAccelerationSign" }
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
    fn test_cas_accelerating() {
        // Moves: +1, +2 → acceleration = +1 → sign = +1
        let mut sig = CloseAccelerationSign::new();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        let v = sig.update(&bar("103")).unwrap(); // d1=2, d2=1 → accel=+1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cas_decelerating() {
        // Moves: +3, +1 → acceleration = -2 → sign = -1
        let mut sig = CloseAccelerationSign::new();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("103")).unwrap();
        let v = sig.update(&bar("104")).unwrap(); // d1=1, d2=3 → accel=-2
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cas_constant_velocity_zero() {
        // +2, +2 → accel=0
        let mut sig = CloseAccelerationSign::new();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("104")).unwrap(); // d1=2, d2=2 → accel=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("volume_accumulation.rs", volume_accumulation),
    ("close_distance_from_ema.rs", close_distance_from_ema),
    ("up_volume_fraction.rs", up_volume_fraction),
    ("close_acceleration_sign.rs", close_acceleration_sign),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
