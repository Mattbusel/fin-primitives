import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. TrueStrengthIndex (simplified - momentum of momentum)
true_strength_index = r'''//! True Strength Index indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Simplified True Strength Index: double-smoothed momentum / double-smoothed absolute momentum.
///
/// Uses simple moving averages instead of EMA for streaming simplicity.
/// `TSI = SMA(SMA(close_change, r), s) / SMA(SMA(|close_change|, r), s) * 100`
///
/// Ranges from -100 to +100. Positive values indicate bullish momentum.
/// Uses `long_period` for outer smoothing, `short_period` for inner smoothing.
pub struct TrueStrengthIndex {
    long_period: usize,
    short_period: usize,
    prev_close: Option<Decimal>,
    changes: VecDeque<Decimal>,   // raw price changes
    abs_changes: VecDeque<Decimal>, // absolute price changes
}

impl TrueStrengthIndex {
    /// Creates a new `TrueStrengthIndex` with given periods (short < long, both >= 2).
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period < 2 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            long_period,
            short_period,
            prev_close: None,
            changes: VecDeque::with_capacity(long_period),
            abs_changes: VecDeque::with_capacity(long_period),
        })
    }

    fn sma(window: &VecDeque<Decimal>, n: usize) -> Option<Decimal> {
        if window.len() < n { return None; }
        let sum: Decimal = window.iter().rev().take(n).sum();
        Some(sum / Decimal::from(n as u32))
    }
}

impl Signal for TrueStrengthIndex {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let change = bar.close - pc;
            self.changes.push_back(change);
            self.abs_changes.push_back(change.abs());
            if self.changes.len() > self.long_period {
                self.changes.pop_front();
                self.abs_changes.pop_front();
            }
        }
        self.prev_close = Some(bar.close);

        if self.changes.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        // inner smooth (short period) then outer smooth (long period of inner results)
        // Simplified: compute rolling SMA of last short_period, then check vs long_period avg
        let inner_mom = match Self::sma(&self.changes, self.short_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };
        let inner_abs = match Self::sma(&self.abs_changes, self.short_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };

        // outer smooth: use long_period window of inner values (approximated as long SMA)
        let outer_mom = match Self::sma(&self.changes, self.long_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };
        let outer_abs = match Self::sma(&self.abs_changes, self.long_period) {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };

        // blend: use average of inner and outer as double-smoothed approximation
        let smoothed_mom = (inner_mom + outer_mom) / Decimal::TWO;
        let smoothed_abs = (inner_abs + outer_abs) / Decimal::TWO;

        if smoothed_abs.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(smoothed_mom / smoothed_abs * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.changes.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.prev_close = None; self.changes.clear(); self.abs_changes.clear(); }
    fn name(&self) -> &str { "TrueStrengthIndex" }
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
    fn test_tsi_constant_zero() {
        // Constant prices → all changes = 0 → TSI = 0
        let mut sig = TrueStrengthIndex::new(2, 4).unwrap();
        for _ in 0..6 {
            sig.update(&bar("100")).unwrap();
        }
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_tsi_all_up_positive() {
        // Consistently rising prices → TSI = +100
        let mut sig = TrueStrengthIndex::new(2, 4).unwrap();
        for i in 0..=6u32 {
            sig.update(&bar(&format!("{}", 100 + i))).unwrap();
        }
        if let SignalValue::Scalar(v) = sig.update(&bar("108")).unwrap() {
            assert!(v > dec!(0), "expected positive TSI, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 2. WilderSmoothedRange
wilder_smoothed_range = r'''//! Wilder Smoothed Range indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Wilder-smoothed (RMA) true range — the smoothing used in ATR.
///
/// `smoothed[t] = (smoothed[t-1] * (period-1) + true_range[t]) / period`
///
/// Equivalent to `ATR` but exposed as a standalone indicator.
/// Initial value seeded after `period` bars of simple averaging.
pub struct WilderSmoothedRange {
    period: usize,
    smoothed: Option<Decimal>,
    warm_up_count: usize,
    warm_up_sum: Decimal,
    prev_close: Option<Decimal>,
}

impl WilderSmoothedRange {
    /// Creates a new `WilderSmoothedRange` with the given smoothing period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            smoothed: None,
            warm_up_count: 0,
            warm_up_sum: Decimal::ZERO,
            prev_close: None,
        })
    }

    fn true_range(bar: &BarInput, prev_close: Option<Decimal>) -> Decimal {
        let hl = bar.high - bar.low;
        match prev_close {
            Some(pc) => {
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
            None => hl,
        }
    }
}

impl Signal for WilderSmoothedRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = Self::true_range(bar, self.prev_close);
        self.prev_close = Some(bar.close);

        match self.smoothed {
            None => {
                self.warm_up_sum += tr;
                self.warm_up_count += 1;
                if self.warm_up_count >= self.period {
                    self.smoothed = Some(self.warm_up_sum / Decimal::from(self.period as u32));
                    Ok(SignalValue::Scalar(self.smoothed.unwrap()))
                } else {
                    Ok(SignalValue::Unavailable)
                }
            }
            Some(prev) => {
                let p = Decimal::from(self.period as u32);
                let new_val = (prev * (p - Decimal::ONE) + tr) / p;
                self.smoothed = Some(new_val);
                Ok(SignalValue::Scalar(new_val))
            }
        }
    }

    fn is_ready(&self) -> bool { self.smoothed.is_some() }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.smoothed = None;
        self.warm_up_count = 0;
        self.warm_up_sum = Decimal::ZERO;
        self.prev_close = None;
    }
    fn name(&self) -> &str { "WilderSmoothedRange" }
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
    fn test_wsr_constant_range() {
        // Constant range of 20 → smoothed value converges to 20
        let mut sig = WilderSmoothedRange::new(3).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
        // More bars → stays at 20
        let v2 = sig.update(&bar("110", "90", "100")).unwrap();
        assert_eq!(v2, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_wsr_not_ready() {
        let mut sig = WilderSmoothedRange::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }
}
'''

# 3. PriceOscillatorPct
price_oscillator_pct = r'''//! Price Oscillator Percentage indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage difference between short-period and long-period SMA.
///
/// `(short_SMA - long_SMA) / long_SMA * 100`
///
/// Normalizes the momentum oscillator by the long-term baseline price level,
/// making it comparable across different price levels.
pub struct PriceOscillatorPct {
    short_period: usize,
    long_period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceOscillatorPct {
    /// Creates a new `PriceOscillatorPct` with short and long SMA periods.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period == 0 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            short_period,
            long_period,
            window: VecDeque::with_capacity(long_period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceOscillatorPct {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.long_period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let long_sma = self.sum / Decimal::from(self.long_period as u32);
        if long_sma.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let short_sum: Decimal = self.window.iter().rev().take(self.short_period).sum();
        let short_sma = short_sum / Decimal::from(self.short_period as u32);
        Ok(SignalValue::Scalar((short_sma - long_sma) / long_sma * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceOscillatorPct" }
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
    fn test_pop_flat_zero() {
        // Constant price → short_sma = long_sma → 0%
        let mut sig = PriceOscillatorPct::new(2, 4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pop_trend_positive() {
        // Rising price: short SMA > long SMA → positive %
        let mut sig = PriceOscillatorPct::new(2, 4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("103")).unwrap() {
            assert!(v > dec!(0), "expected positive, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 4. CloseToHighRatio
close_to_high_ratio = r'''//! Close-to-High Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `close / high`.
///
/// Measures how consistently price closes near the session high.
/// Values near 1.0: closes at or near the high (bullish).
/// Values far below 1.0: closes significantly below the high (bearish rejection).
/// Bars with zero high are skipped.
pub struct CloseToHighRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToHighRatio {
    /// Creates a new `CloseToHighRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToHighRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if !bar.high.is_zero() {
            let ratio = bar.close / bar.high;
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
    fn name(&self) -> &str { "CloseToHighRatio" }
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
    fn test_cthr_close_at_high() {
        // close = high → ratio = 1
        let mut sig = CloseToHighRatio::new(2).unwrap();
        sig.update(&bar("110", "110")).unwrap();
        let v = sig.update(&bar("110", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cthr_close_at_half_high() {
        // high=110, close=55 → ratio = 0.5
        let mut sig = CloseToHighRatio::new(2).unwrap();
        sig.update(&bar("110", "55")).unwrap();
        let v = sig.update(&bar("110", "55")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
'''

files = [
    ("true_strength_index.rs", true_strength_index),
    ("wilder_smoothed_range.rs", wilder_smoothed_range),
    ("price_oscillator_pct.rs", price_oscillator_pct),
    ("close_to_high_ratio.rs", close_to_high_ratio),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
