import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. RangeMomentum
range_momentum = r'''//! Range Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// N-bar change in rolling average bar range.
///
/// `avg_range(t) - avg_range(t-N)` where avg_range is a K-period simple average.
/// Positive: ranges expanding over N bars (volatility accelerating).
/// Negative: ranges contracting over N bars (volatility decelerating).
pub struct RangeMomentum {
    avg_period: usize,
    mom_period: usize,
    bar_window: VecDeque<Decimal>, // raw ranges
    range_sum: Decimal,
    avg_history: VecDeque<Decimal>, // history of avg_range values
}

impl RangeMomentum {
    /// Creates a new `RangeMomentum`.
    ///
    /// `avg_period`: period for computing rolling average range.
    /// `mom_period`: look-back for momentum of that average.
    pub fn new(avg_period: usize, mom_period: usize) -> Result<Self, FinError> {
        if avg_period == 0 || mom_period == 0 {
            return Err(FinError::InvalidPeriod(if avg_period == 0 { avg_period } else { mom_period }));
        }
        Ok(Self {
            avg_period,
            mom_period,
            bar_window: VecDeque::with_capacity(avg_period),
            range_sum: Decimal::ZERO,
            avg_history: VecDeque::with_capacity(mom_period + 1),
        })
    }
}

impl Signal for RangeMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.bar_window.push_back(range);
        self.range_sum += range;
        if self.bar_window.len() > self.avg_period {
            if let Some(old) = self.bar_window.pop_front() {
                self.range_sum -= old;
            }
        }
        if self.bar_window.len() < self.avg_period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_range = self.range_sum / Decimal::from(self.avg_period as u32);
        self.avg_history.push_back(avg_range);
        if self.avg_history.len() > self.mom_period + 1 {
            self.avg_history.pop_front();
        }
        if self.avg_history.len() < self.mom_period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.avg_history.back().unwrap();
        let base = *self.avg_history.front().unwrap();
        Ok(SignalValue::Scalar(current - base))
    }

    fn is_ready(&self) -> bool { self.avg_history.len() >= self.mom_period + 1 }
    fn period(&self) -> usize { self.avg_period + self.mom_period }
    fn reset(&mut self) {
        self.bar_window.clear();
        self.range_sum = Decimal::ZERO;
        self.avg_history.clear();
    }
    fn name(&self) -> &str { "RangeMomentum" }
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
    fn test_rm_constant_range_zero() {
        // Constant range → avg_range constant → momentum = 0
        let mut sig = RangeMomentum::new(2, 2).unwrap();
        for _ in 0..5 {
            sig.update(&bar("110", "90")).unwrap();
        }
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rm_expanding_positive() {
        // Range grows: 10, 20, 30... → momentum positive
        let mut sig = RangeMomentum::new(1, 1).unwrap();
        sig.update(&bar("105", "95")).unwrap(); // range=10, avg=10, history=[10]
        sig.update(&bar("110", "90")).unwrap(); // range=20, avg=20, history=[10,20]
        let v = sig.update(&bar("115", "85")).unwrap(); // range=30, avg=30, history=[20,30]
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
'''

# 2. PriceRelativeStrength
price_relative_strength = r'''//! Price Relative Strength indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Ratio of average gains to average losses over rolling period (RSI numerator component).
///
/// `avg_gain / avg_loss` — the raw RS value used in RSI.
/// High values: gains dominate (strong upward momentum).
/// Low values: losses dominate (strong downward momentum).
/// Returns MAX_VALUE when avg_loss is zero (pure bullish streak).
pub struct PriceRelativeStrength {
    period: usize,
    prev_close: Option<Decimal>,
    gain_window: VecDeque<Decimal>,
    loss_window: VecDeque<Decimal>,
    gain_sum: Decimal,
    loss_sum: Decimal,
}

impl PriceRelativeStrength {
    /// Creates a new `PriceRelativeStrength` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            gain_window: VecDeque::with_capacity(period),
            loss_window: VecDeque::with_capacity(period),
            gain_sum: Decimal::ZERO,
            loss_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceRelativeStrength {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let gain = if bar.close > pc { bar.close - pc } else { Decimal::ZERO };
            let loss = if bar.close < pc { pc - bar.close } else { Decimal::ZERO };

            self.gain_window.push_back(gain);
            self.loss_window.push_back(loss);
            self.gain_sum += gain;
            self.loss_sum += loss;

            if self.gain_window.len() > self.period {
                if let Some(og) = self.gain_window.pop_front() { self.gain_sum -= og; }
                if let Some(ol) = self.loss_window.pop_front() { self.loss_sum -= ol; }
            }
        }
        self.prev_close = Some(bar.close);

        if self.gain_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_gain = self.gain_sum / Decimal::from(self.period as u32);
        let avg_loss = self.loss_sum / Decimal::from(self.period as u32);

        if avg_loss.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from(u32::MAX)));
        }
        Ok(SignalValue::Scalar(avg_gain / avg_loss))
    }

    fn is_ready(&self) -> bool { self.gain_window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.gain_window.clear();
        self.loss_window.clear();
        self.gain_sum = Decimal::ZERO;
        self.loss_sum = Decimal::ZERO;
    }
    fn name(&self) -> &str { "PriceRelativeStrength" }
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
    fn test_prs_equal_gains_losses_one() {
        // Average gain = average loss → RS = 1
        let mut sig = PriceRelativeStrength::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap(); // gain=2
        let v = sig.update(&bar("100")).unwrap(); // loss=2, avg_gain=1, avg_loss=1 → RS=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_prs_all_up_max() {
        // All gains → avg_loss=0 → returns MAX
        let mut sig = PriceRelativeStrength::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("104")).unwrap();
        if let SignalValue::Scalar(rs) = v {
            assert!(rs > dec!(1000), "expected large RS for all-up, got {rs}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 3. OpenLowRange
open_low_range = r'''//! Open-Low Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `open - low` (lower wick from open perspective).
///
/// Measures how far price fell below the opening price on average.
/// High values: consistent selling pressure from the open, gaps down.
/// Low values: price tends to find support near or above the open.
pub struct OpenLowRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenLowRange {
    /// Creates a new `OpenLowRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenLowRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ol = bar.open - bar.low;
        self.window.push_back(ol);
        self.sum += ol;
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
    fn name(&self) -> &str { "OpenLowRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, l: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(110),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_olr_open_at_low() {
        // open=low → range=0
        let mut sig = OpenLowRange::new(2).unwrap();
        sig.update(&bar("90", "90")).unwrap();
        let v = sig.update(&bar("90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_olr_basic() {
        // open=100, low=90 → ol=10
        let mut sig = OpenLowRange::new(2).unwrap();
        sig.update(&bar("100", "90")).unwrap();
        let v = sig.update(&bar("100", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
'''

# 4. HighOpenRange
high_open_range = r'''//! High-Open Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `high - open` (upper spike from open perspective).
///
/// Measures how far price rallied above the opening price on average.
/// High values: consistent buying pressure from the open, intraday rallies.
/// Low values: price tends to fail near or below the open.
pub struct HighOpenRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighOpenRange {
    /// Creates a new `HighOpenRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for HighOpenRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ho = bar.high - bar.open;
        self.window.push_back(ho);
        self.sum += ho;
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
    fn name(&self) -> &str { "HighOpenRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, o: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: dec!(90),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_hor_open_at_high() {
        // open=high → range=0
        let mut sig = HighOpenRange::new(2).unwrap();
        sig.update(&bar("110", "110")).unwrap();
        let v = sig.update(&bar("110", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hor_basic() {
        // high=110, open=100 → ho=10
        let mut sig = HighOpenRange::new(2).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        let v = sig.update(&bar("110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
'''

files = [
    ("range_momentum.rs", range_momentum),
    ("price_relative_strength.rs", price_relative_strength),
    ("open_low_range.rs", open_low_range),
    ("high_open_range.rs", high_open_range),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
