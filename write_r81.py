import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. PriceMomentumOscillator
price_momentum_oscillator = r'''//! Price Momentum Oscillator indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Short-period minus long-period rolling close SMA.
///
/// Positive: short-term trend above long-term trend (bullish momentum).
/// Negative: short-term trend below long-term trend (bearish momentum).
/// `short_period` must be less than `long_period`.
pub struct PriceMomentumOscillator {
    short_period: usize,
    long_period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceMomentumOscillator {
    /// Creates a new `PriceMomentumOscillator` with short and long SMA periods.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period == 0 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            short_period,
            long_period,
            window: VecDeque::with_capacity(long_period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceMomentumOscillator {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.long_period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let long_sma = self.sum / Decimal::from(self.long_period as u32);
        let short_sum: Decimal = self.window.iter().rev().take(self.short_period).sum();
        let short_sma = short_sum / Decimal::from(self.short_period as u32);
        Ok(SignalValue::Scalar(short_sma - long_sma))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceMomentumOscillator" }
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
    fn test_pmo_equal_smas_zero() {
        // Constant price → both SMAs equal → oscillator = 0
        let mut sig = PriceMomentumOscillator::new(2, 4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pmo_trending_up_positive() {
        // Rising prices: short SMA > long SMA → positive
        let mut sig = PriceMomentumOscillator::new(2, 4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        // long_sma = (100+101+102+103)/4 = 101.5, short_sma = (102+103)/2 = 102.5
        // oscillator = 102.5 - 101.5 = 1.0
        assert_eq!(v, SignalValue::Scalar(dec!(1.0)));
    }
}
'''

# 2. VolumeRateOfChange
volume_rate_of_change = r'''//! Volume Rate of Change indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage change in volume from N bars ago: `(vol[t] - vol[t-N]) / vol[t-N] * 100`.
///
/// Positive: volume increasing relative to N bars ago (growing interest).
/// Negative: volume decreasing relative to N bars ago (fading interest).
/// Returns Unavailable until N+1 bars have been seen.
pub struct VolumeRateOfChange {
    period: usize,
    window: VecDeque<Decimal>,
}

impl VolumeRateOfChange {
    /// Creates a new `VolumeRateOfChange` with the given N-bar look-back.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for VolumeRateOfChange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        if self.window.len() > self.period + 1 {
            self.window.pop_front();
        }
        if self.window.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let base = *self.window.front().unwrap();
        if base.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let current = *self.window.back().unwrap();
        Ok(SignalValue::Scalar((current - base) / base * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "VolumeRateOfChange" }
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
    fn test_vroc_flat_zero() {
        let mut sig = VolumeRateOfChange::new(2).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vroc_double_volume() {
        // vol goes from 1000 to 2000 over 2 bars → +100%
        let mut sig = VolumeRateOfChange::new(2).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1500")).unwrap();
        let v = sig.update(&bar("2000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
'''

# 3. AverageGain
average_gain = r'''//! Average Gain indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of positive close returns (gains only).
///
/// Used as a component in RSI calculation and standalone bullish strength measure.
/// Negative returns contribute 0 to the average.
/// Returns 0 when no positive returns exist in the window.
pub struct AverageGain {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl AverageGain {
    /// Creates a new `AverageGain` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for AverageGain {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let gain = if bar.close > pc { bar.close - pc } else { Decimal::ZERO };
            self.window.push_back(gain);
            self.sum += gain;
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
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "AverageGain" }
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
    fn test_ag_all_up() {
        // +2, +2, +2 → avg_gain = 2
        let mut sig = AverageGain::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("104")).unwrap();
        let v = sig.update(&bar("106")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_ag_no_gains() {
        // All down → avg_gain = 0
        let mut sig = AverageGain::new(3).unwrap();
        sig.update(&bar("106")).unwrap();
        sig.update(&bar("104")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. AverageLoss
average_loss = r'''//! Average Loss indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of negative close returns (losses only), returned as positive values.
///
/// Used as a component in RSI calculation and standalone bearish strength measure.
/// Positive returns contribute 0 to the average.
/// Returns 0 when no losses exist in the window.
pub struct AverageLoss {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl AverageLoss {
    /// Creates a new `AverageLoss` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for AverageLoss {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let loss = if bar.close < pc { pc - bar.close } else { Decimal::ZERO };
            self.window.push_back(loss);
            self.sum += loss;
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
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "AverageLoss" }
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
    fn test_al_all_down() {
        // -2, -2, -2 → avg_loss = 2 (positive)
        let mut sig = AverageLoss::new(3).unwrap();
        sig.update(&bar("106")).unwrap();
        sig.update(&bar("104")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_al_no_losses() {
        // All up → avg_loss = 0
        let mut sig = AverageLoss::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("104")).unwrap();
        let v = sig.update(&bar("106")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("price_momentum_oscillator.rs", price_momentum_oscillator),
    ("volume_rate_of_change.rs", volume_rate_of_change),
    ("average_gain.rs", average_gain),
    ("average_loss.rs", average_loss),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
