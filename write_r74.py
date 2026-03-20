import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. CumReturnMomentum (N-bar return)
cum_return_momentum = r'''//! Cumulative Return Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// N-bar cumulative return: `close[t] / close[t-N] - 1`.
///
/// Measures the raw return over the look-back period without smoothing.
/// Equivalent to the raw momentum denominator used in many academic studies.
pub struct CumReturnMomentum {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl CumReturnMomentum {
    /// Creates a new `CumReturnMomentum` with the given N-bar look-back.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for CumReturnMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let base = *self.closes.front().unwrap();
        let current = *self.closes.back().unwrap();
        if base.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(current / base - Decimal::ONE))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "CumReturnMomentum" }
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
    fn test_crm_flat_zero() {
        let mut sig = CumReturnMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_crm_ten_percent_return() {
        // 100 → 110 over 2 bars → 10% return
        let mut sig = CumReturnMomentum::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("105")).unwrap();
        let v = sig.update(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.1)));
    }
}
'''

# 2. HighLowOscillator
high_low_oscillator = r'''//! High-Low Oscillator indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// `(period_high + period_low) / 2 - SMA(close, period)`.
///
/// Measures how close the SMA is to the channel midpoint.
/// Positive: SMA above channel midpoint (upper channel bias).
/// Negative: SMA below channel midpoint (lower channel bias).
pub struct HighLowOscillator {
    period: usize,
    window: VecDeque<BarInput>,
    close_sum: Decimal,
}

impl HighLowOscillator {
    /// Creates a new `HighLowOscillator` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            close_sum: Decimal::ZERO,
        })
    }
}

impl Signal for HighLowOscillator {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.close_sum += bar.close;
        self.window.push_back(*bar);
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.close_sum -= old.close;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_high = self.window.iter().map(|b| b.high).fold(Decimal::MIN, Decimal::max);
        let period_low = self.window.iter().map(|b| b.low).fold(Decimal::MAX, Decimal::min);
        let channel_mid = (period_high + period_low) / Decimal::TWO;
        let sma = self.close_sum / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(sma - channel_mid))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.close_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "HighLowOscillator" }
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
    fn test_hlo_close_at_midpoint() {
        // SMA = channel midpoint → oscillator = 0
        let mut sig = HighLowOscillator::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        // channel_mid = (110+90)/2 = 100, sma = 100 → diff = 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hlo_close_above_midpoint() {
        let mut sig = HighLowOscillator::new(2).unwrap();
        sig.update(&bar("110", "90", "108")).unwrap();
        let v = sig.update(&bar("110", "90", "108")).unwrap();
        // sma=108, channel_mid=100 → diff=8
        assert_eq!(v, SignalValue::Scalar(dec!(8)));
    }
}
'''

# 3. BarStrengthIndex
bar_strength_index = r'''//! Bar Strength Index indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - low) / (high - low) * 100`.
///
/// Measures how strongly price closes within its own bar range.
/// 100 = close at high (maximum bullish strength per bar).
/// 0 = close at low (maximum bearish weakness per bar).
/// Bars with zero range contribute 50 (neutral).
pub struct BarStrengthIndex {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BarStrengthIndex {
    /// Creates a new `BarStrengthIndex` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BarStrengthIndex {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let strength = if range.is_zero() {
            Decimal::from(50u32)
        } else {
            (bar.close - bar.low) / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(strength);
        self.sum += strength;
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
    fn name(&self) -> &str { "BarStrengthIndex" }
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
    fn test_bsi_close_at_high() {
        let mut sig = BarStrengthIndex::new(2).unwrap();
        sig.update(&bar("110", "90", "110")).unwrap(); // strength=100
        let v = sig.update(&bar("110", "90", "110")).unwrap(); // avg=100
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_bsi_close_at_low() {
        let mut sig = BarStrengthIndex::new(2).unwrap();
        sig.update(&bar("110", "90", "90")).unwrap(); // strength=0
        let v = sig.update(&bar("110", "90", "90")).unwrap(); // avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. OpenToCloseReturn
open_to_close_return = r'''//! Open-to-Close Return indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - open) / open * 100`.
///
/// Measures the average intra-bar return (close vs open).
/// Positive = bullish intra-bar move on average; negative = bearish.
/// Excludes bars where open is zero.
pub struct OpenToCloseReturn {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenToCloseReturn {
    /// Creates a new `OpenToCloseReturn` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenToCloseReturn {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if !bar.open.is_zero() {
            let ret = (bar.close - bar.open) / bar.open * Decimal::ONE_HUNDRED;
            self.window.push_back(ret);
            self.sum += ret;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
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
    fn name(&self) -> &str { "OpenToCloseReturn" }
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
    fn test_otcr_flat_zero() {
        let mut sig = OpenToCloseReturn::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_otcr_bullish() {
        // open 100, close 110 → +10%
        let mut sig = OpenToCloseReturn::new(2).unwrap();
        sig.update(&bar("100", "110")).unwrap();
        let v = sig.update(&bar("100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
'''

files = [
    ("cum_return_momentum.rs", cum_return_momentum),
    ("high_low_oscillator.rs", high_low_oscillator),
    ("bar_strength_index.rs", bar_strength_index),
    ("open_to_close_return.rs", open_to_close_return),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
