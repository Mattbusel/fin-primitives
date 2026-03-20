import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. PriceVelocity
price_velocity = r'''//! Price Velocity indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of close-to-close change (absolute price velocity).
///
/// `(close[t] - close[t-1])` averaged over the rolling period.
/// Positive: average upward momentum in price units.
/// Negative: average downward momentum in price units.
/// Unlike percentage return, this preserves the price scale.
pub struct PriceVelocity {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceVelocity {
    /// Creates a new `PriceVelocity` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for PriceVelocity {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let change = bar.close - pc;
            self.window.push_back(change);
            self.sum += change;
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
    fn name(&self) -> &str { "PriceVelocity" }
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
    fn test_pv_flat_zero() {
        // Constant price → velocity = 0
        let mut sig = PriceVelocity::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pv_constant_up() {
        // +2 each bar → velocity = 2
        let mut sig = PriceVelocity::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("104")).unwrap();
        let v = sig.update(&bar("106")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }
}
'''

# 2. HigherLowCount
higher_low_count = r'''//! Higher Low Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where low > previous bar's low.
///
/// Measures uptrend quality — higher lows indicate buyers supporting the market.
/// High count: strong uptrend with consistent demand at higher levels.
/// Low count: trend weakening, failing to make higher lows.
pub struct HigherLowCount {
    period: usize,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl HigherLowCount {
    /// Creates a new `HigherLowCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_low: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for HigherLowCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pl) = self.prev_low {
            let higher: u8 = if bar.low > pl { 1 } else { 0 };
            self.window.push_back(higher);
            self.count += higher as usize;
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
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_low = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "HigherLowCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_hlc_all_higher_lows() {
        let mut sig = HigherLowCount::new(3).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("92")).unwrap();
        sig.update(&bar("94")).unwrap();
        let v = sig.update(&bar("96")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_hlc_no_higher_lows() {
        let mut sig = HigherLowCount::new(3).unwrap();
        sig.update(&bar("96")).unwrap();
        sig.update(&bar("94")).unwrap();
        sig.update(&bar("92")).unwrap();
        let v = sig.update(&bar("90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. LowerHighCount
lower_high_count = r'''//! Lower High Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where high < previous bar's high.
///
/// Measures downtrend quality — lower highs indicate sellers capping rallies.
/// High count: strong downtrend with consistent resistance at lower levels.
/// Low count: trend weakening, failing to make lower highs.
pub struct LowerHighCount {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl LowerHighCount {
    /// Creates a new `LowerHighCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for LowerHighCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let lower: u8 = if bar.high < ph { 1 } else { 0 };
            self.window.push_back(lower);
            self.count += lower as usize;
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
    fn name(&self) -> &str { "LowerHighCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: dec!(90),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_lhc_all_lower_highs() {
        let mut sig = LowerHighCount::new(3).unwrap();
        sig.update(&bar("120")).unwrap();
        sig.update(&bar("118")).unwrap();
        sig.update(&bar("116")).unwrap();
        let v = sig.update(&bar("114")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_lhc_no_lower_highs() {
        let mut sig = LowerHighCount::new(3).unwrap();
        sig.update(&bar("110")).unwrap();
        sig.update(&bar("112")).unwrap();
        sig.update(&bar("114")).unwrap();
        let v = sig.update(&bar("116")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. CloseRelativeToRange
close_relative_to_range = r'''//! Close Relative to Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - rolling_low) / (rolling_high - rolling_low)`.
///
/// Measures where the current close sits within the N-bar price channel:
/// - 1.0: close at the rolling high
/// - 0.0: close at the rolling low
/// - 0.5: close at the midpoint of the channel
pub struct CloseRelativeToRange {
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl CloseRelativeToRange {
    /// Creates a new `CloseRelativeToRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseRelativeToRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rolling_high = self.highs.iter().cloned().fold(Decimal::MIN, Decimal::max);
        let rolling_low = self.lows.iter().cloned().fold(Decimal::MAX, Decimal::min);
        let channel = rolling_high - rolling_low;
        if channel.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from_str_exact("0.5").unwrap()));
        }
        Ok(SignalValue::Scalar((bar.close - rolling_low) / channel))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.highs.clear(); self.lows.clear(); }
    fn name(&self) -> &str { "CloseRelativeToRange" }
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
    fn test_crtr_at_top() {
        // close = rolling_high → 1.0
        let mut sig = CloseRelativeToRange::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("120", "95", "120")).unwrap();
        // rolling_high=120, rolling_low=90, close=120 → (120-90)/(120-90) = 1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_crtr_at_bottom() {
        // close = rolling_low → 0.0
        let mut sig = CloseRelativeToRange::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("105", "85", "85")).unwrap();
        // rolling_high=110, rolling_low=85, close=85 → (85-85)/(110-85) = 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("price_velocity.rs", price_velocity),
    ("higher_low_count.rs", higher_low_count),
    ("lower_high_count.rs", lower_high_count),
    ("close_relative_to_range.rs", close_relative_to_range),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
