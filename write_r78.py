import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. BodyMomentum
body_momentum = r'''//! Body Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of signed body sizes: `close - open` per bar.
///
/// Positive sums indicate net bullish body movement.
/// Negative sums indicate net bearish body movement.
/// Measures cumulative conviction of price direction over the window.
pub struct BodyMomentum {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyMomentum {
    /// Creates a new `BodyMomentum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BodyMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.close - bar.open;
        self.window.push_back(body);
        self.sum += body;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "BodyMomentum" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: c.parse().unwrap(),
            low: o.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_bm_all_bullish() {
        // Each bar: open=100, close=105 → body=+5, sum over 3 bars = 15
        let mut sig = BodyMomentum::new(3).unwrap();
        sig.update(&bar("100", "105")).unwrap();
        sig.update(&bar("100", "105")).unwrap();
        let v = sig.update(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_bm_mixed_zero() {
        // Alternating +5 and -5 → sum = 0 over period of 2
        let mut sig = BodyMomentum::new(2).unwrap();
        sig.update(&bar("100", "105")).unwrap(); // +5
        let v = sig.update(&bar("105", "100")).unwrap(); // -5, sum=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 2. GapFillRatio
gap_fill_ratio = r'''//! Gap Fill Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling proportion of bars that filled their opening gap.
///
/// An opening gap is filled when:
/// - Bullish gap: open > prev_close AND current low <= prev_close
/// - Bearish gap: open < prev_close AND current high >= prev_close
///
/// Returns fraction of gap bars (relative to total gap bars in window).
/// Returns 0 when no gaps occurred in the window.
pub struct GapFillRatio {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<(u8, u8)>, // (had_gap, filled_gap)
    gap_count: usize,
    fill_count: usize,
}

impl GapFillRatio {
    /// Creates a new `GapFillRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            gap_count: 0,
            fill_count: 0,
        })
    }
}

impl Signal for GapFillRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (had_gap, filled_gap) = if let Some(pc) = self.prev_close {
            let bullish_gap = bar.open > pc;
            let bearish_gap = bar.open < pc;
            if bullish_gap {
                let filled = if bar.low <= pc { 1u8 } else { 0u8 };
                (1u8, filled)
            } else if bearish_gap {
                let filled = if bar.high >= pc { 1u8 } else { 0u8 };
                (1u8, filled)
            } else {
                (0u8, 0u8)
            }
        } else {
            (0u8, 0u8)
        };
        self.prev_close = Some(bar.close);

        self.window.push_back((had_gap, filled_gap));
        self.gap_count += had_gap as usize;
        self.fill_count += filled_gap as usize;
        if self.window.len() > self.period {
            if let Some((og, of_)) = self.window.pop_front() {
                self.gap_count -= og as usize;
                self.fill_count -= of_ as usize;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.gap_count == 0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let ratio = Decimal::from(self.fill_count as u32) / Decimal::from(self.gap_count as u32);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.gap_count = 0;
        self.fill_count = 0;
    }
    fn name(&self) -> &str { "GapFillRatio" }
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
    fn test_gfr_gap_filled() {
        // prev_close=100, open=105 (gap up), low=99 (fills) → 1/1 = 1
        let mut sig = GapFillRatio::new(2).unwrap();
        sig.update(&bar("100", "100", "100", "100")).unwrap(); // sets prev_close=100
        sig.update(&bar("105", "107", "99", "103")).unwrap(); // gap up, filled
        let v = sig.update(&bar("103", "105", "102", "104")).unwrap(); // no gap
        // window=[filled_gap, no_gap], gap_count=1, fill_count=1 → 1.0
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_gfr_no_gaps() {
        // No gaps → ratio = 0
        let mut sig = GapFillRatio::new(2).unwrap();
        sig.update(&bar("100", "105", "98", "100")).unwrap();
        sig.update(&bar("100", "103", "98", "100")).unwrap(); // no gap (open==prev_close)
        let v = sig.update(&bar("100", "102", "98", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. PriceCompressionRatio
price_compression_ratio = r'''//! Price Compression Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Ratio of N-bar price range to sum of individual bar ranges.
///
/// `(rolling_high - rolling_low) / Σ(bar ranges)`
///
/// Values near 0: bars cancel each other (choppy, directionless).
/// Values near 1: bars stack in same direction (trending, no overlap).
/// Measures directional efficiency of recent price movement.
pub struct PriceCompressionRatio {
    period: usize,
    window: VecDeque<BarInput>,
    range_sum: Decimal,
}

impl PriceCompressionRatio {
    /// Creates a new `PriceCompressionRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            range_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceCompressionRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.range_sum += range;
        self.window.push_back(*bar);
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.range_sum -= old.high - old.low;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.range_sum.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let period_high = self.window.iter().map(|b| b.high).fold(Decimal::MIN, Decimal::max);
        let period_low = self.window.iter().map(|b| b.low).fold(Decimal::MAX, Decimal::min);
        let net_range = period_high - period_low;
        Ok(SignalValue::Scalar(net_range / self.range_sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.range_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceCompressionRatio" }
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
    fn test_pcr_non_overlapping() {
        // bar1: high=110, low=100 (range=10); bar2: high=120, low=110 (range=10)
        // net_range = 120-100 = 20, range_sum = 20 → ratio = 1.0
        let mut sig = PriceCompressionRatio::new(2).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        let v = sig.update(&bar("120", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_pcr_full_overlap() {
        // Identical bars: high=110, low=90 (range=20 each)
        // net_range = 110-90 = 20, range_sum = 40 → ratio = 0.5
        let mut sig = PriceCompressionRatio::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
'''

# 4. VolumeTrendSlope
volume_trend_slope = r'''//! Volume Trend Slope indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Linear regression slope of volume over the rolling period.
///
/// Positive slope: volume trending upward (growing interest).
/// Negative slope: volume trending downward (fading interest).
/// Uses ordinary least squares over the rolling window.
pub struct VolumeTrendSlope {
    period: usize,
    window: VecDeque<Decimal>,
}

impl VolumeTrendSlope {
    /// Creates a new `VolumeTrendSlope` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for VolumeTrendSlope {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let vals: Vec<f64> = self.window.iter().filter_map(|v| v.to_f64()).collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // OLS: slope = (n*Σ(x*y) - Σx*Σy) / (n*Σ(x²) - (Σx)²)
        // x = 0, 1, ..., n-1
        let sum_x: f64 = (0..self.period).map(|i| i as f64).sum();
        let sum_y: f64 = vals.iter().sum();
        let sum_xy: f64 = vals.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
        let sum_x2: f64 = (0..self.period).map(|i| (i as f64) * (i as f64)).sum();
        let denom = n * sum_x2 - sum_x * sum_x;
        if denom == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let slope = (n * sum_xy - sum_x * sum_y) / denom;
        match Decimal::from_f64_retain(slope) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "VolumeTrendSlope" }
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
    fn test_vts_constant_zero_slope() {
        // Constant volume → slope = 0
        let mut sig = VolumeTrendSlope::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vts_increasing_positive_slope() {
        // Volume: 1000, 2000, 3000 → slope = 1000
        let mut sig = VolumeTrendSlope::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("2000")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("3000")).unwrap() {
            assert!(v > dec!(0), "expected positive slope, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

files = [
    ("body_momentum.rs", body_momentum),
    ("gap_fill_ratio.rs", gap_fill_ratio),
    ("price_compression_ratio.rs", price_compression_ratio),
    ("volume_trend_slope.rs", volume_trend_slope),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
