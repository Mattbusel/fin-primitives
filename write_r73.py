import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. DownsideDeviation
downside_deviation = r'''//! Downside Deviation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Rolling standard deviation of only negative close returns (downside risk).
///
/// Used as a component of the Sortino ratio. Ignores positive returns,
/// focusing purely on the volatility of losses.
/// Returns 0 when there are no negative returns in the window.
pub struct DownsideDeviation {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl DownsideDeviation {
    /// Creates a new `DownsideDeviation` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for DownsideDeviation {
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

        let neg_vals: Vec<f64> = self.returns.iter()
            .filter_map(|r| {
                let fv = r.to_f64()?;
                if fv < 0.0 { Some(fv) } else { None }
            })
            .collect();

        if neg_vals.len() < 2 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let n = neg_vals.len() as f64;
        let mean = neg_vals.iter().sum::<f64>() / n;
        let var = neg_vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);

        match Decimal::from_f64_retain(var.sqrt()) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "DownsideDeviation" }
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
    fn test_downside_deviation_no_losses() {
        // Only up returns → no downside → returns 0
        let mut sig = DownsideDeviation::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_downside_deviation_not_ready() {
        let mut sig = DownsideDeviation::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }
}
'''

# 2. VolumePriceImpact
volume_price_impact = r'''//! Volume Price Impact indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `|close - prev_close| / volume`.
///
/// Measures price change per unit of volume (market impact / price efficiency).
/// Low values indicate high liquidity; high values indicate thin liquidity.
/// Bars with zero volume are skipped.
pub struct VolumePriceImpact {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumePriceImpact {
    /// Creates a new `VolumePriceImpact` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumePriceImpact {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !bar.volume.is_zero() {
                let impact = (bar.close - pc).abs() / bar.volume;
                self.window.push_back(impact);
                self.sum += impact;
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
    fn name(&self) -> &str { "VolumePriceImpact" }
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
    fn test_vpi_no_price_change() {
        let mut sig = VolumePriceImpact::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vpi_basic_impact() {
        // Price change = 1, volume = 1000 → impact = 0.001
        let mut sig = VolumePriceImpact::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap(); // seeds prev_close=100
        sig.update(&bar("101", "1000")).unwrap(); // impact=1/1000=0.001
        let v = sig.update(&bar("102", "1000")).unwrap(); // impact=0.001, avg=0.001
        assert_eq!(v, SignalValue::Scalar(dec!(0.001)));
    }
}
'''

# 3. CloseAcceleration
close_acceleration = r'''//! Close Acceleration indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rate of change of N-bar momentum: `momentum[t] - momentum[t-1]`.
///
/// Where momentum = `close[t] - close[t-N]`.
/// Positive values indicate accelerating upward momentum.
/// Negative values indicate decelerating upward or accelerating downward momentum.
/// Requires `2*period + 1` bars to first produce a value.
pub struct CloseAcceleration {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl CloseAcceleration {
    /// Creates a new `CloseAcceleration` with the given N-bar momentum period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(2 * period + 1) })
    }
}

impl Signal for CloseAcceleration {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > 2 * self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < 2 * self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let n = self.period;
        let len = self.closes.len();
        // momentum[t] = close[last] - close[last-n]
        // momentum[t-1] = close[last-1] - close[last-1-n]
        let mom_t = self.closes[len - 1] - self.closes[len - 1 - n];
        let mom_t1 = self.closes[len - 2] - self.closes[len - 2 - n];
        Ok(SignalValue::Scalar(mom_t - mom_t1))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= 2 * self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "CloseAcceleration" }
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
    fn test_close_acceleration_constant_momentum() {
        // Constant +1 per bar: momentum always = period, acceleration = 0
        let mut sig = CloseAcceleration::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("103")).unwrap();
        let v = sig.update(&bar("104")).unwrap(); // mom_t=2, mom_t1=2, accel=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_close_acceleration_not_ready() {
        let mut sig = CloseAcceleration::new(3).unwrap();
        for _ in 0..6 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        // needs 2*3+1=7 bars
    }
}
'''

# 4. HighLowDivergence
high_low_divergence = r'''//! High-Low Divergence indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(high - prev_high) - (prev_low - low)`.
///
/// Positive values: highs expanding faster than lows are contracting (bullish expansion).
/// Negative values: lows dropping faster than highs are rising (bearish expansion).
/// Near zero: symmetric range expansion or contraction.
pub struct HighLowDivergence {
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighLowDivergence {
    /// Creates a new `HighLowDivergence` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for HighLowDivergence {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let div = (bar.high - ph) - (pl - bar.low);
            self.window.push_back(div);
            self.sum += div;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.prev_low = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "HighLowDivergence" }
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
    fn test_hl_divergence_symmetric_expansion() {
        // Both high and low expand equally → divergence = 0
        let mut sig = HighLowDivergence::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // seeds
        sig.update(&bar("115", "85")).unwrap(); // +5, -5 → div=5-5=0
        let v = sig.update(&bar("120", "80")).unwrap(); // +5, -5 → div=5-5=0, avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hl_divergence_bullish() {
        // High expanding, low stable → positive divergence
        let mut sig = HighLowDivergence::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // seeds
        sig.update(&bar("115", "90")).unwrap(); // high+5, low+0 → div=5-0=5
        let v = sig.update(&bar("120", "90")).unwrap(); // div=5, avg=5
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }
}
'''

files = [
    ("downside_deviation.rs", downside_deviation),
    ("volume_price_impact.rs", volume_price_impact),
    ("close_acceleration.rs", close_acceleration),
    ("high_low_divergence.rs", high_low_divergence),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
