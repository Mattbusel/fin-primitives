import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. VolumeMomentumDivergence
volume_momentum_divergence = r'''//! Volume Momentum Divergence indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Difference between rolling volume change and rolling price change direction.
///
/// `sign(volume_change) - sign(price_change)` averaged over the period.
/// Values near +2 or -2 indicate strong divergence (price/volume disagree).
/// Values near 0 indicate convergence (price and volume agree on direction).
pub struct VolumeMomentumDivergence {
    period: usize,
    prev_close: Option<Decimal>,
    prev_volume: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeMomentumDivergence {
    /// Creates a new `VolumeMomentumDivergence` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            prev_volume: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeMomentumDivergence {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(pc), Some(pv)) = (self.prev_close, self.prev_volume) {
            let price_sign: i32 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            let vol_sign: i32 = if bar.volume > pv { 1 } else if bar.volume < pv { -1 } else { 0 };
            let divergence = Decimal::from(vol_sign - price_sign);
            self.window.push_back(divergence);
            self.sum += divergence;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_close = Some(bar.close);
        self.prev_volume = Some(bar.volume);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.prev_volume = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeMomentumDivergence" }
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
    fn test_vmd_convergence() {
        // Price up, volume up → divergence = 0 (both agree)
        let mut sig = VolumeMomentumDivergence::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "1100")).unwrap();
        let v = sig.update(&bar("102", "1200")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vmd_divergence() {
        // Price up (+1), volume down (-1) → vol_sign - price_sign = -2
        let mut sig = VolumeMomentumDivergence::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "900")).unwrap();
        let v = sig.update(&bar("102", "800")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-2)));
    }
}
'''

# 2. RangeExpansionIndex
range_expansion_index = r'''//! Range Expansion Index indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of current range divided by previous range.
///
/// Values > 1 indicate expanding ranges (increasing volatility).
/// Values < 1 indicate contracting ranges (decreasing volatility).
/// Bars with zero previous range are skipped.
pub struct RangeExpansionIndex {
    period: usize,
    prev_range: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeExpansionIndex {
    /// Creates a new `RangeExpansionIndex` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_range: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeExpansionIndex {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if let Some(pr) = self.prev_range {
            if !pr.is_zero() {
                let ratio = range / pr;
                self.window.push_back(ratio);
                self.sum += ratio;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() {
                        self.sum -= old;
                    }
                }
            }
        }
        self.prev_range = Some(range);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_range = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "RangeExpansionIndex" }
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
    fn test_rei_constant_range() {
        // Same range each bar → ratio = 1
        let mut sig = RangeExpansionIndex::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rei_doubling_range() {
        // Range doubles each bar → ratio = 2
        let mut sig = RangeExpansionIndex::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();   // range=20
        sig.update(&bar("120", "80")).unwrap();   // range=40, ratio=2
        let v = sig.update(&bar("140", "60")).unwrap(); // range=80, ratio=2, avg=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }
}
'''

# 3. PricePositionRank
price_position_rank = r'''//! Price Position Rank indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentile rank of current close within the rolling window.
///
/// Returns a value from 0 to 1:
/// - 0.0 = current close is the lowest in the window
/// - 1.0 = current close is the highest in the window
/// Useful for identifying overbought/oversold conditions over N bars.
pub struct PricePositionRank {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PricePositionRank {
    /// Creates a new `PricePositionRank` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period) })
    }
}

impl Signal for PricePositionRank {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let current = bar.close;
        let below = self.closes.iter().filter(|&&c| c < current).count();
        let total = self.closes.len() - 1; // exclude current bar itself
        if total == 0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let rank = Decimal::from(below as u32) / Decimal::from(total as u32);
        Ok(SignalValue::Scalar(rank))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "PricePositionRank" }
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
    fn test_ppr_at_top() {
        // Final bar is highest → rank = 1
        let mut sig = PricePositionRank::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        let v = sig.update(&bar("102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ppr_at_bottom() {
        // Final bar is lowest → rank = 0
        let mut sig = PricePositionRank::new(3).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("101")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ppr_at_middle() {
        // Final bar is middle → rank = 0.5
        let mut sig = PricePositionRank::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("101")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
'''

# 4. TailRatio
tail_ratio = r'''//! Tail Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of upper wick length divided by lower wick length.
///
/// `upper_wick = high - max(open, close)`
/// `lower_wick = min(open, close) - low`
///
/// Values > 1: upper wicks dominate (selling pressure / rejection at highs).
/// Values < 1: lower wicks dominate (buying support / rejection at lows).
/// Bars where lower wick is zero are skipped.
pub struct TailRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl TailRatio {
    /// Creates a new `TailRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for TailRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body_high = bar.open.max(bar.close);
        let body_low = bar.open.min(bar.close);
        let upper_wick = bar.high - body_high;
        let lower_wick = body_low - bar.low;

        if !lower_wick.is_zero() {
            let ratio = upper_wick / lower_wick;
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
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "TailRatio" }
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
    fn test_tail_ratio_equal_wicks() {
        // open=100, close=100, high=110, low=90 → upper=10, lower=10 → ratio=1
        let mut sig = TailRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_tail_ratio_no_upper_wick() {
        // close=high, lower wick exists → ratio=0
        let mut sig = TailRatio::new(2).unwrap();
        sig.update(&bar("95", "110", "90", "110")).unwrap(); // upper=0, lower=5 → ratio=0
        let v = sig.update(&bar("95", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("volume_momentum_divergence.rs", volume_momentum_divergence),
    ("range_expansion_index.rs", range_expansion_index),
    ("price_position_rank.rs", price_position_rank),
    ("tail_ratio.rs", tail_ratio),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
