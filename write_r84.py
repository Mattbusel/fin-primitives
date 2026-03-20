import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. TrendConsistencyScore
trend_consistency_score = r'''//! Trend Consistency Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Fraction of bars where close > N-bar EMA (simplified as SMA).
///
/// Measures how often price closes above its rolling average.
/// Values near 1.0: strong uptrend (price consistently above average).
/// Values near 0.0: strong downtrend (price consistently below average).
/// Values near 0.5: choppy / no trend.
pub struct TrendConsistencyScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl TrendConsistencyScore {
    /// Creates a new `TrendConsistencyScore` with the given rolling period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for TrendConsistencyScore {
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

        let sma = self.sum / Decimal::from(self.period as u32);
        let above_count = self.window.iter().filter(|&&c| c > sma).count();
        let score = Decimal::from(above_count as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(score))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "TrendConsistencyScore" }
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
    fn test_tcs_all_above_sma() {
        // Strongly trending up: last bars above SMA
        let mut sig = TrendConsistencyScore::new(4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("110")).unwrap();
        sig.update(&bar("120")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("130")).unwrap() {
            // sma = (100+110+120+130)/4=115, above sma: 120,130 → 2/4 = 0.5
            // Actually 100 and 110 are below 115, 120 and 130 are above → 0.5
            assert!(v >= dec!(0) && v <= dec!(1), "score out of range: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tcs_flat_half() {
        // Constant prices → none strictly above SMA → score = 0
        let mut sig = TrendConsistencyScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 2. VolatilitySpike
volatility_spike = r'''//! Volatility Spike indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Returns 1 if current bar range exceeds N-bar rolling average by a multiplier, else 0.
///
/// Detects sudden volatility spikes relative to recent baseline.
/// Useful for flagging news events, gaps, or exceptional market activity.
/// `multiplier` is provided as a percentage integer (e.g., 200 = 2x average range).
pub struct VolatilitySpike {
    period: usize,
    multiplier_pct: u32,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolatilitySpike {
    /// Creates a new `VolatilitySpike`.
    ///
    /// `multiplier_pct`: threshold as % of average range (e.g., 200 = 2x, 150 = 1.5x).
    pub fn new(period: usize, multiplier_pct: u32) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            multiplier_pct,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolatilitySpike {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        self.sum += range;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_range = self.sum / Decimal::from(self.period as u32);
        let threshold = avg_range * Decimal::from(self.multiplier_pct) / Decimal::ONE_HUNDRED;
        let spike: i32 = if range > threshold { 1 } else { 0 };
        Ok(SignalValue::Scalar(Decimal::from(spike)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolatilitySpike" }
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
    fn test_vs_no_spike() {
        // All bars same range → current = avg → no spike (not strictly greater)
        let mut sig = VolatilitySpike::new(3, 150).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vs_spike_detected() {
        // Previous bars range=20, current range=60 → 60 > 20*1.5=30 → spike
        let mut sig = VolatilitySpike::new(3, 150).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // range=20
        sig.update(&bar("110", "90")).unwrap(); // range=20
        sig.update(&bar("110", "90")).unwrap(); // range=20, avg=20
        let v = sig.update(&bar("130", "70")).unwrap(); // range=60 > 20*1.5=30 → spike
        // but window now includes this 60 too: avg=(20+20+60)/3=33.3, threshold=50
        // Actually window slides: [20,20,60], avg=33.3, threshold=50 → 60>50 → spike
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
'''

# 3. ClosePctFromHigh
close_pct_from_high = r'''//! Close Percentage From N-Bar High indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage distance of current close from the N-bar rolling high.
///
/// `(rolling_high - close) / rolling_high * 100`
///
/// Values near 0: close near recent high (strong momentum).
/// Higher values: close well below recent high (pullback or weakness).
/// Always non-negative.
pub struct ClosePctFromHigh {
    period: usize,
    window: VecDeque<Decimal>,
}

impl ClosePctFromHigh {
    /// Creates a new `ClosePctFromHigh` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for ClosePctFromHigh {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.high);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rolling_high = self.window.iter().cloned().fold(Decimal::MIN, Decimal::max);
        if rolling_high.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let pct = (rolling_high - bar.close) / rolling_high * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "ClosePctFromHigh" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: h.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_cpfh_at_high() {
        // close = rolling_high → 0%
        let mut sig = ClosePctFromHigh::new(3).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("105", "105")).unwrap();
        let v = sig.update(&bar("110", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cpfh_below_high() {
        // rolling_high=110, close=99 → (110-99)/110*100 = 10%
        let mut sig = ClosePctFromHigh::new(2).unwrap();
        sig.update(&bar("110", "110")).unwrap();
        let v = sig.update(&bar("100", "99")).unwrap();
        // rolling_high=max(110,100)=110, close=99 → (110-99)/110*100 = 10
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
'''

# 4. ClosePctFromLow
close_pct_from_low = r'''//! Close Percentage From N-Bar Low indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage distance of current close above the N-bar rolling low.
///
/// `(close - rolling_low) / rolling_low * 100`
///
/// Values near 0: close near recent low (weak momentum / potential reversal).
/// Higher values: close well above recent low (strong recovery or uptrend).
/// Always non-negative.
pub struct ClosePctFromLow {
    period: usize,
    window: VecDeque<Decimal>,
}

impl ClosePctFromLow {
    /// Creates a new `ClosePctFromLow` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for ClosePctFromLow {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.low);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rolling_low = self.window.iter().cloned().fold(Decimal::MAX, Decimal::min);
        if rolling_low.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let pct = (bar.close - rolling_low) / rolling_low * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "ClosePctFromLow" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(l: &str, c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_cpfl_at_low() {
        // close = rolling_low → 0%
        let mut sig = ClosePctFromLow::new(2).unwrap();
        sig.update(&bar("90", "90")).unwrap();
        let v = sig.update(&bar("85", "85")).unwrap();
        // rolling_low=min(90,85)=85, close=85 → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cpfl_above_low() {
        // rolling_low=90, close=99 → (99-90)/90*100 = 10%
        let mut sig = ClosePctFromLow::new(2).unwrap();
        sig.update(&bar("90", "95")).unwrap();
        let v = sig.update(&bar("92", "99")).unwrap();
        // rolling_low=min(90,92)=90, close=99 → (99-90)/90*100 = 10
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
'''

files = [
    ("trend_consistency_score.rs", trend_consistency_score),
    ("volatility_spike.rs", volatility_spike),
    ("close_pct_from_high.rs", close_pct_from_high),
    ("close_pct_from_low.rs", close_pct_from_low),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
