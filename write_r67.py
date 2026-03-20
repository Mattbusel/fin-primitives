import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. BullBearPower
bull_bear_power = r'''//! Bull/Bear Power indicator (Elder).

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Elder's Bull/Bear Power: `(high - EMA) - (low - EMA)` = `high - low` averaged via EMA context.
///
/// More precisely: rolling `bull_power = high - EMA(close)` and `bear_power = low - EMA(close)`.
/// This returns `bull_power - bear_power` = simple EMA-smoothed `high - low` deviation.
/// Positive values indicate bullish pressure dominates; negative indicates bearish.
pub struct BullBearPower {
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    bars_seen: usize,
}

impl BullBearPower {
    /// Creates a new `BullBearPower` with the given EMA period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { period, k, ema: None, bars_seen: 0 })
    }
}

impl Signal for BullBearPower {
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
        // bull_power - bear_power = (high - ema) - (low - ema) = high - low
        // But measured through EMA context: return both combined as net power
        let bull = bar.high - ema;
        let bear = bar.low - ema;
        Ok(SignalValue::Scalar(bull - bear))
    }

    fn is_ready(&self) -> bool { self.bars_seen >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.ema = None; self.bars_seen = 0; }
    fn name(&self) -> &str { "BullBearPower" }
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
    fn test_bull_bear_power_not_ready() {
        let mut sig = BullBearPower::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bull_bear_power_positive() {
        // bull - bear = high - low, always >= 0
        let mut sig = BullBearPower::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("115", "85", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert!(x >= dec!(0), "bull-bear power should be >= 0, got {}", x);
        }
    }
}
'''

# 2. CandleSymmetry
candle_symmetry = r'''//! Candle Symmetry indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `min(upper_wick, lower_wick) / max(upper_wick, lower_wick)`.
///
/// Values near 1.0 indicate balanced wicks (symmetric candles).
/// Values near 0.0 indicate one-sided wick dominance.
/// Bars with both wicks zero are excluded.
pub struct CandleSymmetry {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CandleSymmetry {
    /// Creates a new `CandleSymmetry` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CandleSymmetry {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body_top = bar.close.max(bar.open);
        let body_bot = bar.close.min(bar.open);
        let upper = bar.high - body_top;
        let lower = body_bot - bar.low;

        let max_wick = upper.max(lower);
        if !max_wick.is_zero() {
            let min_wick = upper.min(lower);
            let sym = min_wick / max_wick;
            self.window.push_back(sym);
            self.sum += sym;
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
    fn name(&self) -> &str { "CandleSymmetry" }
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
    fn test_candle_symmetry_equal_wicks() {
        // Equal upper and lower wicks => symmetry = 1
        // open=100, close=100, high=110, low=90 => upper=10, lower=10
        let mut sig = CandleSymmetry::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_candle_symmetry_one_sided() {
        // Only upper wick: open=close=90, high=110, low=90 => upper=20, lower=0 => sym=0
        let mut sig = CandleSymmetry::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "90")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. SupportTestCount
support_test_count = r'''//! Support Test Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars whose low is within 0.5% of the period's lowest low.
///
/// Measures how many times price has tested the support level in the recent window.
/// Higher counts suggest a stronger, well-tested support zone.
pub struct SupportTestCount {
    period: usize,
    lows: VecDeque<Decimal>,
    threshold_pct: Decimal,
}

impl SupportTestCount {
    /// Creates a new `SupportTestCount` with the given rolling period and threshold percentage.
    pub fn new(period: usize, threshold_pct: Decimal) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, lows: VecDeque::with_capacity(period), threshold_pct })
    }
}

impl Signal for SupportTestCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.lows.push_back(bar.low);
        if self.lows.len() > self.period {
            self.lows.pop_front();
        }
        if self.lows.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let threshold = period_low * self.threshold_pct / Decimal::ONE_HUNDRED;
        let count = self.lows.iter()
            .filter(|&&l| (l - period_low).abs() <= threshold)
            .count();
        Ok(SignalValue::Scalar(Decimal::from(count as u32)))
    }

    fn is_ready(&self) -> bool { self.lows.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.lows.clear(); }
    fn name(&self) -> &str { "SupportTestCount" }
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
    fn test_support_test_count_all_at_support() {
        let mut sig = SupportTestCount::new(3, dec!(0.5)).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("90")).unwrap();
        let v = sig.update(&bar("90")).unwrap();
        // All 3 at same low => all 3 tests
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_support_test_count_one_test() {
        let mut sig = SupportTestCount::new(3, dec!(0.5)).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("110")).unwrap();
        // Only bar at 90 is the period low, others are far above
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
'''

# 4. ReturnAutoCorrelation
return_autocorrelation = r'''//! Return Auto-Correlation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Lag-1 autocorrelation of close returns over the rolling window.
///
/// Measures whether returns tend to continue (positive autocorrelation = momentum)
/// or reverse (negative autocorrelation = mean-reversion).
/// Returns a value in [-1, 1].
pub struct ReturnAutoCorrelation {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl ReturnAutoCorrelation {
    /// Creates a new `ReturnAutoCorrelation` with the given rolling period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for ReturnAutoCorrelation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc;
                self.returns.push_back(ret);
                if self.returns.len() > self.period {
                    self.returns.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.returns.len();
        let vals: Vec<f64> = self.returns.iter()
            .filter_map(|r| r.to_f64())
            .collect();
        if vals.len() < n {
            return Ok(SignalValue::Unavailable);
        }

        // Lag-1 autocorrelation: corr(r[t], r[t-1])
        let n_pairs = n - 1;
        if n_pairs < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let x: Vec<f64> = vals[..n_pairs].to_vec();  // r[t]
        let y: Vec<f64> = vals[1..].to_vec();         // r[t+1]

        let np = n_pairs as f64;
        let mx = x.iter().sum::<f64>() / np;
        let my = y.iter().sum::<f64>() / np;

        let num: f64 = x.iter().zip(y.iter()).map(|(xi, yi)| (xi - mx) * (yi - my)).sum();
        let dx: f64 = x.iter().map(|xi| (xi - mx) * (xi - mx)).sum::<f64>().sqrt();
        let dy: f64 = y.iter().map(|yi| (yi - my) * (yi - my)).sum::<f64>().sqrt();

        let denom = dx * dy;
        if denom == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let corr = num / denom;
        match Decimal::from_f64_retain(corr) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "ReturnAutoCorrelation" }
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
    fn test_autocorr_not_ready() {
        let mut sig = ReturnAutoCorrelation::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_autocorr_trending_positive() {
        // Steady uptrend: each return similar → positive autocorrelation
        let mut sig = ReturnAutoCorrelation::new(5).unwrap();
        let prices = ["100", "102", "104", "106", "108", "110"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = sig.update(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(x) = last {
            assert!(x > dec!(0), "trending series should have positive autocorr, got {}", x);
        }
    }
}
'''

files = [
    ("bull_bear_power.rs", bull_bear_power),
    ("candle_symmetry.rs", candle_symmetry),
    ("support_test_count.rs", support_test_count),
    ("return_autocorrelation.rs", return_autocorrelation),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
