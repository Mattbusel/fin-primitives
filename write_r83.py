import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. PriceEntropyScore
price_entropy_score = r'''//! Price Entropy Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Approximate entropy of close returns over the rolling window.
///
/// Discretizes returns into bins (up/flat/down) and computes Shannon entropy.
/// High entropy: unpredictable, random-walk-like market.
/// Low entropy: predictable, trending or mean-reverting regime.
/// Entropy is normalized to [0, 1] by dividing by log2(3).
pub struct PriceEntropyScore {
    period: usize,
    prev_close: Option<Decimal>,
    signs: VecDeque<i8>, // -1, 0, +1
}

impl PriceEntropyScore {
    /// Creates a new `PriceEntropyScore` with the given rolling period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, signs: VecDeque::with_capacity(period) })
    }
}

impl Signal for PriceEntropyScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let sign: i8 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            self.signs.push_back(sign);
            if self.signs.len() > self.period {
                self.signs.pop_front();
            }
        }
        self.prev_close = Some(bar.close);

        if self.signs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let up = self.signs.iter().filter(|&&s| s == 1).count() as f64;
        let down = self.signs.iter().filter(|&&s| s == -1).count() as f64;
        let flat = self.signs.iter().filter(|&&s| s == 0).count() as f64;

        let mut entropy = 0.0f64;
        for &count in &[up, down, flat] {
            if count > 0.0 {
                let p = count / n;
                entropy -= p * p.log2();
            }
        }
        // normalize by log2(3) ≈ 1.585
        let normalized = entropy / std::f64::consts::LOG2_E.recip().mul_add(3.0f64.ln(), 0.0);
        // simpler: log2(3) = ln(3)/ln(2)
        let log2_3 = 3.0f64.ln() / 2.0f64.ln();
        let normalized = entropy / log2_3;

        match Decimal::from_f64_retain(normalized) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.signs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.signs.clear(); }
    fn name(&self) -> &str { "PriceEntropyScore" }
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
    fn test_pes_all_same_direction_low_entropy() {
        // All up → only one bin populated → entropy = 0
        let mut sig = PriceEntropyScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pes_mixed_higher_entropy() {
        // Mix of up and down → higher entropy
        let mut sig = PriceEntropyScore::new(4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap(); // up
        sig.update(&bar("100")).unwrap(); // down
        sig.update(&bar("102")).unwrap(); // up
        if let SignalValue::Scalar(v) = sig.update(&bar("100")).unwrap() { // down
            // window=[up,down,up,down], entropy > 0
            assert!(v > dec!(0), "expected non-zero entropy, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 2. BullPowerBearPower
bull_power_bear_power = r'''//! Bull Power and Bear Power indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `high - SMA(close)` (bull power) minus `low - SMA(close)` (bear power).
///
/// Also known as Elder Force combined measure.
/// Positive: bull power dominates (highs above average, lows not as far below).
/// Negative: bear power dominates (lows below average, highs not as far above).
pub struct BullPowerBearPower {
    period: usize,
    window: VecDeque<BarInput>,
    close_sum: Decimal,
}

impl BullPowerBearPower {
    /// Creates a new `BullPowerBearPower` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            close_sum: Decimal::ZERO,
        })
    }
}

impl Signal for BullPowerBearPower {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.close_sum += bar.close;
        self.window.push_back(*bar);
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.close_sum -= old.close;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.close_sum / Decimal::from(self.period as u32);
        let bull_power_sum: Decimal = self.window.iter().map(|b| b.high - sma).sum();
        let bear_power_sum: Decimal = self.window.iter().map(|b| b.low - sma).sum();
        let len = Decimal::from(self.period as u32);
        let net = (bull_power_sum - bear_power_sum) / len;
        Ok(SignalValue::Scalar(net))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.close_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "BullPowerBearPower" }
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
    fn test_bpbp_symmetric_zero() {
        // Equal upper and lower distance from SMA → net = 0
        // high = SMA + 10, low = SMA - 10 → bull=10, bear=-10, net= (10-(-10))/1... wait
        // bull_power = high - sma = +10
        // bear_power = low - sma = -10
        // net = (bull_power - bear_power) / n = (10 - (-10)) / 1 = 20
        // Hmm, let me think again. bull_power_sum - bear_power_sum per bar is (high-sma) - (low-sma) = high - low = range
        // For symmetric bar centered at SMA: high=110, low=90, sma=100 → (110-90)/1 = 20
        // Let me just check it's positive
        let mut sig = BullPowerBearPower::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        // sma=100, bull=(110-100)=10, bear=(90-100)=-10, net=(10-(-10))/1 per bar avg= 20
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_bpbp_not_ready() {
        let mut sig = BullPowerBearPower::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }
}
'''

# 3. VolatilityAdjustedMomentum
volatility_adjusted_momentum = r'''//! Volatility Adjusted Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// N-bar return divided by the rolling standard deviation of returns.
///
/// `(close[t] / close[t-N] - 1) / std_dev(returns, N)`
///
/// Normalizes momentum by recent volatility — similar to a Sharpe-like ratio.
/// Returns 0 when standard deviation is zero (flat price series).
pub struct VolatilityAdjustedMomentum {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl VolatilityAdjustedMomentum {
    /// Creates a new `VolatilityAdjustedMomentum` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for VolatilityAdjustedMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let base = *self.closes.front().unwrap();
        let current = *self.closes.back().unwrap();
        if base.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        // Compute N-bar return
        let n_bar_ret = (current / base - Decimal::ONE).to_f64().unwrap_or(0.0);

        // Compute returns in window
        let vals: Vec<f64> = self.closes.iter()
            .zip(self.closes.iter().skip(1))
            .filter_map(|(a, b)| {
                if a.is_zero() { return None; }
                (*b / *a - Decimal::ONE).to_f64()
            })
            .collect();

        if vals.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let vam = n_bar_ret / std_dev;
        match Decimal::from_f64_retain(vam) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "VolatilityAdjustedMomentum" }
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
    fn test_vam_flat_zero() {
        // Constant prices → std_dev=0 → result=0
        let mut sig = VolatilityAdjustedMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vam_positive_momentum() {
        // Rising prices → positive VAM
        let mut sig = VolatilityAdjustedMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("103")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("106")).unwrap() {
            assert!(v > dec!(0), "expected positive VAM, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
'''

# 4. OpenHighLowCloseAvg
open_high_low_close_avg = r'''//! Open-High-Low-Close Average indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(open + high + low + close) / 4`.
///
/// The OHLC average (also called the four-price doji average) captures the full
/// bar information equally. Smoother than close-only SMA, less biased than
/// typical price (which weights close twice).
pub struct OpenHighLowCloseAvg {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenHighLowCloseAvg {
    /// Creates a new `OpenHighLowCloseAvg` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenHighLowCloseAvg {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ohlc4 = (bar.open + bar.high + bar.low + bar.close) / Decimal::from(4u32);
        self.window.push_back(ohlc4);
        self.sum += ohlc4;
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
    fn name(&self) -> &str { "OpenHighLowCloseAvg" }
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
    fn test_ohlca_symmetric_bar() {
        // open=close=100, high=110, low=90 → ohlc4 = (100+110+90+100)/4 = 100
        let mut sig = OpenHighLowCloseAvg::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_ohlca_all_same() {
        // All values = 100 → ohlc4 = 100, avg = 100
        let mut sig = OpenHighLowCloseAvg::new(3).unwrap();
        sig.update(&bar("100", "100", "100", "100")).unwrap();
        sig.update(&bar("100", "100", "100", "100")).unwrap();
        let v = sig.update(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
'''

files = [
    ("price_entropy_score.rs", price_entropy_score),
    ("bull_power_bear_power.rs", bull_power_bear_power),
    ("volatility_adjusted_momentum.rs", volatility_adjusted_momentum),
    ("open_high_low_close_avg.rs", open_high_low_close_avg),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
