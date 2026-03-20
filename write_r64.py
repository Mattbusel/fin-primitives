import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. ReturnDispersion
return_dispersion = r'''//! Return Dispersion indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Rolling std dev of close returns divided by rolling mean absolute return.
///
/// Measures how dispersed returns are relative to their typical magnitude.
/// Higher values indicate more erratic, less consistent return behaviour.
pub struct ReturnDispersion {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl ReturnDispersion {
    /// Creates a new `ReturnDispersion` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for ReturnDispersion {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc;
                self.window.push_back(ret);
                if self.window.len() > self.period {
                    self.window.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.window.len() as f64;
        let vals: Vec<f64> = self.window.iter()
            .filter_map(|r| r.to_f64())
            .collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = vals.iter().sum::<f64>() / n;
        let variance = vals.iter().map(|r| { let d = r - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = variance.sqrt();
        let mean_abs = vals.iter().map(|r| r.abs()).sum::<f64>() / n;

        if mean_abs == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let disp = std_dev / mean_abs;
        match Decimal::from_f64_retain(disp) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); }
    fn name(&self) -> &str { "ReturnDispersion" }
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
    fn test_return_dispersion_not_ready() {
        let mut sig = ReturnDispersion::new(3).unwrap();
        for _ in 0..3 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_return_dispersion_constant_returns() {
        // Constant 1% returns — std dev = 0, dispersion = 0
        let mut sig = ReturnDispersion::new(3).unwrap();
        let prices = ["100", "101", "102.01", "103.0301"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = sig.update(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(x) = last {
            assert!(x < dec!(0.001), "constant returns should have near-zero dispersion, got {}", x);
        }
    }
}
'''

# 2. OpenCloseRatio
open_close_ratio = r'''//! Open-Close Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `open / close`.
///
/// Values > 1 indicate the bar consistently opened above where it closed (bearish bias).
/// Values < 1 indicate the bar consistently opened below where it closed (bullish bias).
pub struct OpenCloseRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenCloseRatio {
    /// Creates a new `OpenCloseRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenCloseRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = bar.open / bar.close;
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
    fn name(&self) -> &str { "OpenCloseRatio" }
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
    fn test_open_close_ratio_equal() {
        // open == close => ratio = 1 for all bars
        let mut sig = OpenCloseRatio::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_open_close_ratio_bullish() {
        // open < close (bullish) => ratio < 1
        let mut sig = OpenCloseRatio::new(2).unwrap();
        sig.update(&bar("95", "100")).unwrap();
        let v = sig.update(&bar("95", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert!(x < dec!(1), "bullish bars should produce ratio < 1, got {}", x);
        }
    }
}
'''

# 3. WickRejectionScore
wick_rejection_score = r'''//! Wick Rejection Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `max(upper_wick, lower_wick) / body`.
///
/// Measures how dominant wicks are relative to the candle body.
/// High values indicate strong price rejection and potential reversals.
/// Returns `Unavailable` when body is zero (doji), and excludes those bars.
pub struct WickRejectionScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl WickRejectionScore {
    /// Creates a new `WickRejectionScore` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for WickRejectionScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = (bar.close - bar.open).abs();
        if !body.is_zero() {
            let upper = bar.high - bar.close.max(bar.open);
            let lower = bar.open.min(bar.close) - bar.low;
            let dom_wick = upper.max(lower);
            let score = dom_wick / body;
            self.window.push_back(score);
            self.sum += score;
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
    fn name(&self) -> &str { "WickRejectionScore" }
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
    fn test_wick_rejection_no_wicks() {
        // No wicks: open=low, close=high => dom_wick = 0, score = 0
        let mut sig = WickRejectionScore::new(2).unwrap();
        sig.update(&bar("100", "110", "100", "110")).unwrap();
        let v = sig.update(&bar("100", "110", "100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_wick_rejection_large_wick() {
        // Large upper wick: open=100, close=101, high=111 => upper_wick=10, body=1, score=10
        let mut sig = WickRejectionScore::new(2).unwrap();
        sig.update(&bar("100", "111", "99", "101")).unwrap();
        let v = sig.update(&bar("100", "111", "99", "101")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert!(x > dec!(1), "large wick should score > 1, got {}", x);
        }
    }
}
'''

# 4. HighLowMidpoint
high_low_midpoint = r'''//! High-Low Midpoint indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of the bar midpoint: `(high + low) / 2`.
///
/// A simple measure of the central price level over the period,
/// less affected by open/close noise than a simple close SMA.
pub struct HighLowMidpoint {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighLowMidpoint {
    /// Creates a new `HighLowMidpoint` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for HighLowMidpoint {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::TWO;
        self.window.push_back(mid);
        self.sum += mid;
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
    fn name(&self) -> &str { "HighLowMidpoint" }
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
    fn test_high_low_midpoint_basic() {
        let mut sig = HighLowMidpoint::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // mid = 100
        let v = sig.update(&bar("120", "80")).unwrap();  // mid = 100
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_high_low_midpoint_rolling() {
        let mut sig = HighLowMidpoint::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // mid=100
        sig.update(&bar("120", "100")).unwrap(); // mid=110, avg=(100+110)/2=105
        let v = sig.update(&bar("130", "110")).unwrap(); // mid=120, avg=(110+120)/2=115
        assert_eq!(v, SignalValue::Scalar(dec!(115)));
    }
}
'''

files = [
    ("return_dispersion.rs", return_dispersion),
    ("open_close_ratio.rs", open_close_ratio),
    ("wick_rejection_score.rs", wick_rejection_score),
    ("high_low_midpoint.rs", high_low_midpoint),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
