import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. DonchianWidth
donchian_width = r'''//! Donchian Channel Width indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Width of the Donchian channel: `rolling_high - rolling_low`.
///
/// Measures the price range over the rolling period.
/// Wide channels: high volatility / trending market.
/// Narrow channels: low volatility / consolidation / breakout setup.
pub struct DonchianWidth {
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl DonchianWidth {
    /// Creates a new `DonchianWidth` with the given rolling period.
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

impl Signal for DonchianWidth {
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
        Ok(SignalValue::Scalar(rolling_high - rolling_low))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.highs.clear(); self.lows.clear(); }
    fn name(&self) -> &str { "DonchianWidth" }
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
    fn test_dw_basic_width() {
        // high=120, low=80 over 2 bars → width=40
        let mut sig = DonchianWidth::new(2).unwrap();
        sig.update(&bar("120", "90")).unwrap();
        let v = sig.update(&bar("110", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(40)));
    }

    #[test]
    fn test_dw_single_bar() {
        // Period 1 → width = bar's own range
        let mut sig = DonchianWidth::new(1).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }
}
'''

# 2. RollingSkewness
rolling_skewness = r'''//! Rolling Skewness indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Skewness of close returns over the rolling period.
///
/// Positive skew: long right tail (more frequent large positive returns).
/// Negative skew: long left tail (more frequent large negative returns).
/// Normal distribution has skewness ≈ 0.
/// Requires at least 3 bars (period >= 3).
pub struct RollingSkewness {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl RollingSkewness {
    /// Creates a new `RollingSkewness` with the given period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for RollingSkewness {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                if let (Some(c), Some(p)) = (bar.close.to_f64(), pc.to_f64()) {
                    self.returns.push_back((c - p) / p);
                    if self.returns.len() > self.period {
                        self.returns.pop_front();
                    }
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let mean = self.returns.iter().sum::<f64>() / n;
        let var = self.returns.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std_dev = var.sqrt();
        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let m3 = self.returns.iter().map(|v| (v - mean).powi(3)).sum::<f64>() / n;
        let skewness = m3 / (std_dev * std_dev * std_dev);

        match Decimal::from_f64_retain(skewness) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "RollingSkewness" }
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
    fn test_rs_flat_zero() {
        // Constant prices → std_dev=0 → skewness=0
        let mut sig = RollingSkewness::new(3).unwrap();
        for _ in 0..5 { sig.update(&bar("100")).unwrap(); }
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rs_not_ready() {
        let mut sig = RollingSkewness::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }
}
'''

# 3. PriceChangeCount
price_change_count = r'''//! Price Change Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where the close changed (up or down) from previous bar.
///
/// Measures market activity / choppiness.
/// High values: price moving frequently (active market).
/// Low values: price staying flat (low-liquidity or range-bound market).
pub struct PriceChangeCount {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl PriceChangeCount {
    /// Creates a new `PriceChangeCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for PriceChangeCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let changed: u8 = if bar.close != pc { 1 } else { 0 };
            self.window.push_back(changed);
            self.count += changed as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "PriceChangeCount" }
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
    fn test_pcc_all_change() {
        // All closes different → count = period
        let mut sig = PriceChangeCount::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_pcc_no_change() {
        // Constant price → count = 0
        let mut sig = PriceChangeCount::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 4. OpenToHighRatio
open_to_high_ratio = r'''//! Open-to-High Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(high - open) / (high - low)`.
///
/// Measures how early in the bar the high tends to form:
/// Values near 1.0: high forms near end of bar (bullish, late surge).
/// Values near 0.0: high forms near start of bar (bearish, sells off from open).
/// Bars with zero range are skipped.
pub struct OpenToHighRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenToHighRatio {
    /// Creates a new `OpenToHighRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenToHighRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if !range.is_zero() {
            let ratio = (bar.high - bar.open) / range;
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
    fn name(&self) -> &str { "OpenToHighRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: o.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_othr_open_at_low() {
        // open=low → high-open = full range → ratio = 1
        let mut sig = OpenToHighRatio::new(2).unwrap();
        sig.update(&bar("90", "110", "90")).unwrap();
        let v = sig.update(&bar("90", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_othr_open_at_high() {
        // open=high → high-open = 0 → ratio = 0
        let mut sig = OpenToHighRatio::new(2).unwrap();
        sig.update(&bar("110", "110", "90")).unwrap();
        let v = sig.update(&bar("110", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("donchian_width.rs", donchian_width),
    ("rolling_skewness.rs", rolling_skewness),
    ("price_change_count.rs", price_change_count),
    ("open_to_high_ratio.rs", open_to_high_ratio),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
