import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. CloseToOpenGap
close_to_open_gap = r'''//! Close-to-Open Gap indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of overnight gap: `(open - prev_close) / prev_close * 100`.
///
/// Positive values indicate upward overnight gaps on average.
/// Negative values indicate downward overnight gaps on average.
/// Skips bars where prev_close is zero.
pub struct CloseToOpenGap {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToOpenGap {
    /// Creates a new `CloseToOpenGap` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToOpenGap {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let gap = (bar.open - pc) / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(gap);
                self.sum += gap;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() {
                        self.sum -= old;
                    }
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
    fn name(&self) -> &str { "CloseToOpenGap" }
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
    fn test_ctog_no_gap() {
        // open = prev_close → gap = 0
        let mut sig = CloseToOpenGap::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ctog_upward_gap() {
        // prev_close=100, open=102 → gap=+2%
        let mut sig = CloseToOpenGap::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("102", "102")).unwrap(); // gap=+2
        let v = sig.update(&bar("102", "102")).unwrap(); // gap=0 (open=prev_close=102)
        // window=[2, 0], avg=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
'''

# 2. HighLowReturnCorrelation
high_low_return_correlation = r'''//! High-Low Return Correlation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Pearson correlation between high returns and low returns over rolling period.
///
/// `high_return[t] = (high[t] - high[t-1]) / high[t-1]`
/// `low_return[t] = (low[t] - low[t-1]) / low[t-1]`
///
/// High correlation (~1): highs and lows move together (trending channels).
/// Low/negative correlation: highs and lows diverge (range expansion/contraction).
pub struct HighLowReturnCorrelation {
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    high_rets: VecDeque<f64>,
    low_rets: VecDeque<f64>,
}

impl HighLowReturnCorrelation {
    /// Creates a new `HighLowReturnCorrelation` with the given period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_high: None,
            prev_low: None,
            high_rets: VecDeque::with_capacity(period),
            low_rets: VecDeque::with_capacity(period),
        })
    }

    fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
        let n = xs.len() as f64;
        if n < 2.0 { return 0.0; }
        let mx = xs.iter().sum::<f64>() / n;
        let my = ys.iter().sum::<f64>() / n;
        let num: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| (x - mx) * (y - my)).sum();
        let dx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>().sqrt();
        let dy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum::<f64>().sqrt();
        if dx == 0.0 || dy == 0.0 { return 0.0; }
        num / (dx * dy)
    }
}

impl Signal for HighLowReturnCorrelation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            if !ph.is_zero() && !pl.is_zero() {
                if let (Some(hr), Some(lr)) = (
                    ((bar.high - ph) / ph).to_f64(),
                    ((bar.low - pl) / pl).to_f64(),
                ) {
                    self.high_rets.push_back(hr);
                    self.low_rets.push_back(lr);
                    if self.high_rets.len() > self.period {
                        self.high_rets.pop_front();
                        self.low_rets.pop_front();
                    }
                }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        if self.high_rets.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let xs: Vec<f64> = self.high_rets.iter().cloned().collect();
        let ys: Vec<f64> = self.low_rets.iter().cloned().collect();
        let corr = Self::pearson(&xs, &ys);
        match Decimal::from_f64_retain(corr) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.high_rets.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
        self.high_rets.clear();
        self.low_rets.clear();
    }
    fn name(&self) -> &str { "HighLowReturnCorrelation" }
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
    fn test_hlrc_not_ready() {
        let mut sig = HighLowReturnCorrelation::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("115", "85")).unwrap();
        let v = sig.update(&bar("120", "80")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_hlrc_perfect_correlation() {
        // High and low move proportionally together → correlation near 1
        let mut sig = HighLowReturnCorrelation::new(3).unwrap();
        sig.update(&bar("100", "90")).unwrap();
        sig.update(&bar("110", "99")).unwrap();  // +10%, +10%
        sig.update(&bar("121", "108.9")).unwrap(); // +10%, +10%
        sig.update(&bar("133.1", "119.79")).unwrap(); // +10%, +10%
        if let SignalValue::Scalar(v) = sig.update(&bar("146.41", "131.769")).unwrap() {
            // All returns identical → correlation = 1 (or very close)
            assert!(v > dec!(0.99), "expected near-perfect correlation, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 3. UpperWickPct
upper_wick_pct = r'''//! Upper Wick Percentage indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of upper wick as a percentage of total bar range.
///
/// `upper_wick = high - max(open, close)`
/// `ratio = upper_wick / (high - low) * 100`
///
/// High values indicate consistent selling pressure at highs (bearish rejection).
/// Zero for doji bars (range = 0) or bars with no upper wick.
pub struct UpperWickPct {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl UpperWickPct {
    /// Creates a new `UpperWickPct` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for UpperWickPct {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let pct = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body_top = bar.open.max(bar.close);
            let upper_wick = bar.high - body_top;
            upper_wick / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(pct);
        self.sum += pct;
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
    fn name(&self) -> &str { "UpperWickPct" }
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
    fn test_uwp_no_upper_wick() {
        // close = high → upper_wick = 0 → pct = 0
        let mut sig = UpperWickPct::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_uwp_half_upper_wick() {
        // open=90, close=90, high=110, low=90 → range=20, body_top=90, upper_wick=20 → 100%
        let mut sig = UpperWickPct::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "90")).unwrap(); // upper_wick=20, range=20 → 100%
        let v = sig.update(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
'''

# 4. LowerWickPct
lower_wick_pct = r'''//! Lower Wick Percentage indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of lower wick as a percentage of total bar range.
///
/// `lower_wick = min(open, close) - low`
/// `ratio = lower_wick / (high - low) * 100`
///
/// High values indicate consistent buying support at lows (bullish rejection).
/// Zero for doji bars (range = 0) or bars with no lower wick.
pub struct LowerWickPct {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl LowerWickPct {
    /// Creates a new `LowerWickPct` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for LowerWickPct {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let pct = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body_bottom = bar.open.min(bar.close);
            let lower_wick = body_bottom - bar.low;
            lower_wick / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(pct);
        self.sum += pct;
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
    fn name(&self) -> &str { "LowerWickPct" }
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
    fn test_lwp_no_lower_wick() {
        // open = close = low → lower_wick = 0 → pct = 0
        let mut sig = LowerWickPct::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "90")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lwp_full_lower_wick() {
        // open=110, close=110, high=110, low=90 → body_bottom=110, lower_wick=20, range=20 → 100%
        let mut sig = LowerWickPct::new(2).unwrap();
        sig.update(&bar("110", "110", "90", "110")).unwrap(); // lower_wick=20/20 = 100%
        let v = sig.update(&bar("110", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
'''

files = [
    ("close_to_open_gap.rs", close_to_open_gap),
    ("high_low_return_correlation.rs", high_low_return_correlation),
    ("upper_wick_pct.rs", upper_wick_pct),
    ("lower_wick_pct.rs", lower_wick_pct),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
