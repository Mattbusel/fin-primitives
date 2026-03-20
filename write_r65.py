import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. VolumeOscillator
volume_oscillator = r'''//! Volume Oscillator indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Volume Oscillator: `(fast_ema_vol - slow_ema_vol) / slow_ema_vol * 100`.
///
/// Measures whether short-term volume is expanding or contracting
/// relative to long-term volume. Positive = volume expansion, negative = contraction.
pub struct VolumeOscillator {
    fast_period: usize,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
    bars_seen: usize,
}

impl VolumeOscillator {
    /// Creates a new `VolumeOscillator` with the given fast and slow EMA periods.
    pub fn new(fast_period: usize, slow_period: usize) -> Result<Self, FinError> {
        if fast_period == 0 || slow_period == 0 || fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(slow_period));
        }
        let fast_k = Decimal::TWO / (Decimal::from(fast_period as u32) + Decimal::ONE);
        let slow_k = Decimal::TWO / (Decimal::from(slow_period as u32) + Decimal::ONE);
        Ok(Self {
            fast_period,
            slow_period,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
            bars_seen: 0,
        })
    }
}

impl Signal for VolumeOscillator {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        self.fast_ema = Some(match self.fast_ema {
            None => vol,
            Some(prev) => vol * self.fast_k + prev * (Decimal::ONE - self.fast_k),
        });
        self.slow_ema = Some(match self.slow_ema {
            None => vol,
            Some(prev) => vol * self.slow_k + prev * (Decimal::ONE - self.slow_k),
        });
        self.bars_seen += 1;
        if self.bars_seen < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }
        let slow = self.slow_ema.unwrap();
        if slow.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let fast = self.fast_ema.unwrap();
        Ok(SignalValue::Scalar((fast - slow) / slow * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.bars_seen >= self.slow_period }
    fn period(&self) -> usize { self.slow_period }
    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.bars_seen = 0;
    }
    fn name(&self) -> &str { "VolumeOscillator" }
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
    fn test_volume_oscillator_not_ready() {
        let mut sig = VolumeOscillator::new(3, 6).unwrap();
        for _ in 0..5 {
            assert_eq!(sig.update(&bar("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_volume_oscillator_constant_volume() {
        // Constant volume: fast_ema == slow_ema => oscillator = 0
        let mut sig = VolumeOscillator::new(3, 6).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = sig.update(&bar("1000")).unwrap();
        }
        if let SignalValue::Scalar(x) = last {
            assert!(x.abs() < dec!(0.0001), "constant vol should give ~0 oscillator, got {}", x);
        }
    }
}
'''

# 2. PriceChannelPosition
price_channel_position = r'''//! Price Channel Position indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Position of close within rolling highest-high / lowest-low channel (0-100%).
///
/// 100 = close at period high, 0 = close at period low.
/// Equivalent to a %K stochastic using high/low channel instead of bar extremes.
pub struct PriceChannelPosition {
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceChannelPosition {
    /// Creates a new `PriceChannelPosition` with the given rolling period.
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

impl Signal for PriceChannelPosition {
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
        let highest = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let lowest = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let range = highest - lowest;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::FIFTY));
        }
        Ok(SignalValue::Scalar((bar.close - lowest) / range * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.highs.clear(); self.lows.clear(); }
    fn name(&self) -> &str { "PriceChannelPosition" }
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
    fn test_price_channel_position_at_high() {
        let mut sig = PriceChannelPosition::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("120", "80", "120")).unwrap(); // close = period high
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_price_channel_position_at_low() {
        let mut sig = PriceChannelPosition::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("120", "80", "80")).unwrap(); // close = period low
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. BodyToRangeRatio
body_to_range_ratio = r'''//! Body-to-Range Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `|close - open| / (high - low)`.
///
/// Measures how much of the total bar range is covered by the candle body.
/// 1.0 = marubozu (no wicks), near 0 = spinning top / doji.
pub struct BodyToRangeRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyToRangeRatio {
    /// Creates a new `BodyToRangeRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BodyToRangeRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open).abs() / range
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
    fn name(&self) -> &str { "BodyToRangeRatio" }
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
    fn test_body_to_range_marubozu() {
        // Full body: open=low, close=high => ratio=1
        let mut sig = BodyToRangeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "100", "110")).unwrap();
        let v = sig.update(&bar("100", "110", "100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_body_to_range_doji() {
        // Doji: open == close => body=0, ratio=0
        let mut sig = BodyToRangeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. CloseAboveHighPrev
close_above_high_prev = r'''//! Close-Above-Prior-High indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentage of bars where `close > prior bar high`.
///
/// Measures breakout frequency — how often price closes above the previous bar's high.
/// High values indicate persistent upside breakouts.
pub struct CloseAboveHighPrev {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAboveHighPrev {
    /// Creates a new `CloseAboveHighPrev` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for CloseAboveHighPrev {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let above: u8 = if bar.close > ph { 1 } else { 0 };
            self.window.push_back(above);
            self.count += above as usize;
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
        let pct = Decimal::from(self.count as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "CloseAboveHighPrev" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: dec!(90),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_close_above_high_prev_always_above() {
        let mut sig = CloseAboveHighPrev::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_high=100
        sig.update(&bar("110", "105")).unwrap(); // 105 > 100 ✓, seeds prev_high=110
        let v = sig.update(&bar("120", "115")).unwrap(); // 115 > 110 ✓ → 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_close_above_high_prev_never_above() {
        let mut sig = CloseAboveHighPrev::new(2).unwrap();
        sig.update(&bar("110", "100")).unwrap(); // seeds prev_high=110
        sig.update(&bar("115", "105")).unwrap(); // 105 < 110 ✗
        let v = sig.update(&bar("120", "110")).unwrap(); // 110 < 115 ✗ → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("volume_oscillator.rs", volume_oscillator),
    ("price_channel_position.rs", price_channel_position),
    ("body_to_range_ratio.rs", body_to_range_ratio),
    ("close_above_high_prev.rs", close_above_high_prev),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
