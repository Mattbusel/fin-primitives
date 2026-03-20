import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. PriceRangeExpansion
price_range_expansion = r'''//! Price Range Expansion indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentage of bars where `range > prior bar range`.
///
/// Measures how often volatility is expanding bar-over-bar.
/// High values suggest accelerating volatility; low values suggest compression.
pub struct PriceRangeExpansion {
    period: usize,
    prev_range: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl PriceRangeExpansion {
    /// Creates a new `PriceRangeExpansion` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_range: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for PriceRangeExpansion {
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
        let pct = Decimal::from(self.count as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_range = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "PriceRangeExpansion" }
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
    fn test_range_expansion_always_expanding() {
        let mut sig = PriceRangeExpansion::new(2).unwrap();
        sig.update(&bar("105", "95")).unwrap(); // range=10, seeds prev
        sig.update(&bar("110", "90")).unwrap(); // range=20 > 10 ✓
        let v = sig.update(&bar("115", "85")).unwrap(); // range=30 > 20 ✓ → 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_range_expansion_never_expanding() {
        let mut sig = PriceRangeExpansion::new(2).unwrap();
        sig.update(&bar("120", "80")).unwrap(); // range=40
        sig.update(&bar("110", "90")).unwrap(); // range=20 < 40 ✗
        let v = sig.update(&bar("105", "95")).unwrap(); // range=10 < 20 ✗ → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 2. HigherHighCount
higher_high_count = r'''//! Higher High Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where `high > prior bar high`.
///
/// Measures upside momentum: higher counts indicate persistent upward price exploration.
pub struct HigherHighCount {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl HigherHighCount {
    /// Creates a new `HigherHighCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for HigherHighCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let hh: u8 = if bar.high > ph { 1 } else { 0 };
            self.window.push_back(hh);
            self.count += hh as usize;
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
    fn name(&self) -> &str { "HigherHighCount" }
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
    fn test_higher_high_count_all_higher() {
        let mut sig = HigherHighCount::new(2).unwrap();
        sig.update(&bar("100")).unwrap(); // seeds prev_high=100
        sig.update(&bar("105")).unwrap(); // 105>100 ✓
        let v = sig.update(&bar("110")).unwrap(); // 110>105 ✓ → count=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_higher_high_count_none_higher() {
        let mut sig = HigherHighCount::new(2).unwrap();
        sig.update(&bar("110")).unwrap(); // seeds prev_high=110
        sig.update(&bar("105")).unwrap(); // 105<110 ✗
        let v = sig.update(&bar("100")).unwrap(); // 100<105 ✗ → count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. VolumeSpikeScore
volume_spike_score = r'''//! Volume Spike Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Current volume divided by the rolling average volume.
///
/// Values > 1 indicate above-average volume (potential spike).
/// Values < 1 indicate below-average volume (quiet market).
/// Useful for confirming breakouts or detecting unusual activity.
pub struct VolumeSpikeScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeSpikeScore {
    /// Creates a new `VolumeSpikeScore` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeSpikeScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let avg = self.sum / Decimal::from(self.period as u32);
        if avg.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(bar.volume / avg))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeSpikeScore" }
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
    fn test_volume_spike_score_average() {
        // All same volume → score = 1
        let mut sig = VolumeSpikeScore::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_volume_spike_score_spike() {
        // 2x average volume → score = 2
        let mut sig = VolumeSpikeScore::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("4000")).unwrap(); // avg=(1000+1000+4000)/3=2000, score=4000/2000=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }
}
'''

# 4. CloseRelativeToEMA
close_relative_to_ema = r'''//! Close-Relative-to-EMA indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage deviation of close from its EMA: `(close - EMA) / EMA * 100`.
///
/// Positive values indicate close is above EMA (bullish extension).
/// Negative values indicate close is below EMA (bearish compression).
pub struct CloseRelativeToEma {
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    bars_seen: usize,
}

impl CloseRelativeToEma {
    /// Creates a new `CloseRelativeToEma` with the given EMA period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { period, k, ema: None, bars_seen: 0 })
    }
}

impl Signal for CloseRelativeToEma {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ema = match self.ema {
            None => bar.close,
            Some(prev) => bar.close * self.k + prev * (Decimal::ONE - self.k),
        };
        self.ema = Some(ema);
        self.bars_seen += 1;
        if self.bars_seen < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if ema.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((bar.close - ema) / ema * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.bars_seen >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.ema = None; self.bars_seen = 0; }
    fn name(&self) -> &str { "CloseRelativeToEma" }
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
    fn test_cre_not_ready() {
        let mut sig = CloseRelativeToEma::new(3).unwrap();
        assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cre_constant_close_zero_deviation() {
        // After warm-up with constant close, EMA = close → deviation = 0
        let mut sig = CloseRelativeToEma::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("price_range_expansion.rs", price_range_expansion),
    ("higher_high_count.rs", higher_high_count),
    ("volume_spike_score.rs", volume_spike_score),
    ("close_relative_to_ema.rs", close_relative_to_ema),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
