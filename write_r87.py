import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. ShadowRatio
shadow_ratio = r'''//! Shadow Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of total wick (shadow) length relative to body length.
///
/// `(upper_wick + lower_wick) / |close - open|`
///
/// High values: large wicks relative to body (indecision, reversal signals).
/// Low values: small wicks, price moves cleanly from open to close.
/// Bars with zero body (doji) contribute a fixed value of 1.0.
/// Bars with zero total wick contribute 0.0.
pub struct ShadowRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl ShadowRatio {
    /// Creates a new `ShadowRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for ShadowRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = (bar.close - bar.open).abs();
        let body_high = bar.open.max(bar.close);
        let body_low = bar.open.min(bar.close);
        let total_wick = (bar.high - body_high) + (body_low - bar.low);

        let ratio = if body.is_zero() {
            Decimal::ONE
        } else {
            total_wick / body
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
    fn name(&self) -> &str { "ShadowRatio" }
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
    fn test_sr_no_wicks() {
        // open=low, close=high → body=range, no wicks → ratio = 0
        let mut sig = ShadowRatio::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap(); // no wicks, ratio=0
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_sr_equal_body_and_wicks() {
        // body=10, upper_wick=5, lower_wick=5 → total_wick=10 → ratio=1
        let mut sig = ShadowRatio::new(2).unwrap();
        // open=95, close=105 (body=10), high=110, low=90
        sig.update(&bar("95", "110", "90", "105")).unwrap();
        let v = sig.update(&bar("95", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
'''

# 2. PriceMeanDeviation
price_mean_deviation = r'''//! Price Mean Deviation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling mean absolute deviation (MAD) of close prices from their rolling mean.
///
/// `mean(|close - SMA(close)|)` over the rolling period.
///
/// A robust volatility measure, less sensitive to outliers than standard deviation.
/// Useful as a substitute for std dev in noisy or fat-tailed markets.
pub struct PriceMeanDeviation {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceMeanDeviation {
    /// Creates a new `PriceMeanDeviation` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for PriceMeanDeviation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = self.sum / Decimal::from(self.period as u32);
        let mad: Decimal = self.window.iter()
            .map(|&c| (c - mean).abs())
            .sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(mad))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceMeanDeviation" }
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
    fn test_pmd_flat_zero() {
        // Constant prices → MAD = 0
        let mut sig = PriceMeanDeviation::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pmd_symmetric() {
        // [90, 100, 110]: mean=100, deviations=[10, 0, 10] → MAD=20/3 ≈ 6.666...
        let mut sig = PriceMeanDeviation::new(3).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("110")).unwrap() {
            assert!(v > dec!(0), "expected non-zero MAD, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 3. AbsReturnSum
abs_return_sum = r'''//! Absolute Return Sum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of absolute close-to-close returns.
///
/// `Σ |close[t] - close[t-1]|` over the rolling period.
///
/// Measures total price path length (activity) over the window.
/// High values: very active, choppy or trending market with large moves.
/// Low values: quiet market with small price movements.
pub struct AbsReturnSum {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl AbsReturnSum {
    /// Creates a new `AbsReturnSum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for AbsReturnSum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let abs_ret = (bar.close - pc).abs();
            self.window.push_back(abs_ret);
            self.sum += abs_ret;
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
        Ok(SignalValue::Scalar(self.sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "AbsReturnSum" }
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
    fn test_ars_flat_zero() {
        // Constant prices → abs_ret = 0 each bar → sum = 0
        let mut sig = AbsReturnSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ars_alternating() {
        // Up 5, down 5, up 5 → sum = 15
        let mut sig = AbsReturnSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("105")).unwrap(); // +5
        sig.update(&bar("100")).unwrap(); // +5
        let v = sig.update(&bar("105")).unwrap(); // +5, sum=15
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }
}
'''

# 4. RollingMaxDrawdown
rolling_max_drawdown = r'''//! Rolling Maximum Drawdown indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Maximum peak-to-trough drawdown of close prices within the rolling window.
///
/// `max_drawdown = max(peak - trough) / peak` for all peak-trough pairs in window.
///
/// Returns the worst percentage drawdown experienced in the last N bars.
/// High values: significant pullbacks occurred in the window.
/// Low values: price moved without major reversals (strong trend).
pub struct RollingMaxDrawdown {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl RollingMaxDrawdown {
    /// Creates a new `RollingMaxDrawdown` with the given rolling period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period) })
    }
}

impl Signal for RollingMaxDrawdown {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Find max drawdown: for each peak, find minimum trough after it
        let vals: Vec<Decimal> = self.closes.iter().cloned().collect();
        let mut max_dd = Decimal::ZERO;
        for i in 0..vals.len() {
            if vals[i].is_zero() { continue; }
            for j in (i+1)..vals.len() {
                if vals[j] < vals[i] {
                    let dd = (vals[i] - vals[j]) / vals[i];
                    if dd > max_dd { max_dd = dd; }
                }
            }
        }
        Ok(SignalValue::Scalar(max_dd * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "RollingMaxDrawdown" }
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
    fn test_rmd_no_drawdown() {
        // Strictly rising → no trough after peak → drawdown = 0
        let mut sig = RollingMaxDrawdown::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("105")).unwrap();
        let v = sig.update(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rmd_ten_percent_drawdown() {
        // Peak=110, trough=99 → dd = 11/110 * 100 = 10%
        let mut sig = RollingMaxDrawdown::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("99")).unwrap() {
            // (110-99)/110*100 = 10%
            assert_eq!(v, dec!(10));
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

files = [
    ("shadow_ratio.rs", shadow_ratio),
    ("price_mean_deviation.rs", price_mean_deviation),
    ("abs_return_sum.rs", abs_return_sum),
    ("rolling_max_drawdown.rs", rolling_max_drawdown),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
