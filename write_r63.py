import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. BarEfficiency
bar_efficiency = r'''use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::{BarInput, FinError, Signal, SignalValue};

/// Rolling average of |close - open| / (high - low) * 100.
/// Measures candle directionality: 0 = pure doji, 100 = full-body candle.
pub struct BarEfficiency {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BarEfficiency {
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod);
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BarEfficiency {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let body = if bar.close >= bar.open {
            bar.close - bar.open
        } else {
            bar.open - bar.close
        };
        let eff = if range.is_zero() {
            Decimal::ZERO
        } else {
            body / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(eff);
        self.sum += eff;
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
    fn name(&self) -> &str { "BarEfficiency" }
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
    fn test_bar_efficiency_full_body() {
        let mut sig = BarEfficiency::new(3).unwrap();
        // Full-body up candles: body = range, efficiency = 100
        assert_eq!(sig.update(&bar("100", "110", "100", "110")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100", "110", "100", "110")).unwrap(), SignalValue::Unavailable);
        let v = sig.update(&bar("100", "110", "100", "110")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert_eq!(x, dec!(100));
        }
    }

    #[test]
    fn test_bar_efficiency_doji() {
        let mut sig = BarEfficiency::new(2).unwrap();
        // Doji: open == close
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert_eq!(x, dec!(0));
        }
    }
}
'''

# 2. ChaikinVolatility
chaikin_volatility = r'''use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::{BarInput, FinError, Signal, SignalValue};

/// Chaikin Volatility: EMA of (high - low) with rate-of-change.
/// Returns (current_ema - ema_N_bars_ago) / ema_N_bars_ago * 100.
pub struct ChaikinVolatility {
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
    history: VecDeque<Decimal>,
    bars_seen: usize,
}

impl ChaikinVolatility {
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod);
        }
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { period, ema: None, k, history: VecDeque::with_capacity(period + 1), bars_seen: 0 })
    }
}

impl Signal for ChaikinVolatility {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let hl = bar.high - bar.low;
        let ema = match self.ema {
            None => hl,
            Some(prev) => hl * self.k + prev * (Decimal::ONE - self.k),
        };
        self.ema = Some(ema);
        self.history.push_back(ema);
        self.bars_seen += 1;
        // Keep period+1 values so we can compare current vs N bars ago
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }
        if self.bars_seen <= self.period {
            return Ok(SignalValue::Unavailable);
        }
        let past = *self.history.front().unwrap();
        if past.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((ema - past) / past * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.bars_seen > self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.ema = None;
        self.history.clear();
        self.bars_seen = 0;
    }
    fn name(&self) -> &str { "ChaikinVolatility" }
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
    fn test_chaikin_volatility_not_ready_initially() {
        let mut sig = ChaikinVolatility::new(3).unwrap();
        for _ in 0..3 {
            assert_eq!(sig.update(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!sig.is_ready());
    }

    #[test]
    fn test_chaikin_volatility_ready_after_period_plus_one() {
        let mut sig = ChaikinVolatility::new(3).unwrap();
        for i in 0..4 {
            let v = sig.update(&bar("110", "90")).unwrap();
            if i < 3 {
                assert_eq!(v, SignalValue::Unavailable);
            } else {
                assert!(matches!(v, SignalValue::Scalar(_)));
            }
        }
    }
}
'''

# 3. CloseToOpenReturn
close_to_open_return = r'''use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::{BarInput, FinError, Signal, SignalValue};

/// Rolling average of (open - prev_close) / prev_close * 100.
/// Measures the average overnight / gap-open return over the period.
pub struct CloseToOpenReturn {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToOpenReturn {
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod);
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToOpenReturn {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.open - pc) / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(ret);
                self.sum += ret;
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
    fn name(&self) -> &str { "CloseToOpenReturn" }
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
    fn test_close_to_open_return_not_ready() {
        let mut sig = CloseToOpenReturn::new(3).unwrap();
        // First bar just seeds prev_close
        assert_eq!(sig.update(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_to_open_return_zero_gap() {
        let mut sig = CloseToOpenReturn::new(2).unwrap();
        // close 100, open 100 = 0% gap
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert_eq!(x, dec!(0));
        } else {
            panic!("expected scalar");
        }
    }
}
'''

# 4. VolumeWeightedRange
volume_weighted_range = r'''use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::{BarInput, FinError, Signal, SignalValue};

/// Rolling volume-weighted average range: Σ(volume * (high-low)) / Σ(volume).
/// Like VWAP but for price range — gives more weight to high-activity bars.
pub struct VolumeWeightedRange {
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (volume, range)
    vol_sum: Decimal,
    range_vol_sum: Decimal,
}

impl VolumeWeightedRange {
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod);
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            vol_sum: Decimal::ZERO,
            range_vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeWeightedRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let vol = bar.volume;
        self.window.push_back((vol, range));
        self.vol_sum += vol;
        self.range_vol_sum += vol * range;
        if self.window.len() > self.period {
            if let Some((ov, or_)) = self.window.pop_front() {
                self.vol_sum -= ov;
                self.range_vol_sum -= ov * or_;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.vol_sum.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(self.range_vol_sum / self.vol_sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.window.clear();
        self.vol_sum = Decimal::ZERO;
        self.range_vol_sum = Decimal::ZERO;
    }
    fn name(&self) -> &str { "VolumeWeightedRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_volume_weighted_range_basic() {
        let mut sig = VolumeWeightedRange::new(2).unwrap();
        sig.update(&bar("110", "90", "1000")).unwrap(); // range=20, vol=1000
        let v = sig.update(&bar("115", "85", "2000")).unwrap(); // range=30, vol=2000
        // VWR = (1000*20 + 2000*30) / (1000+2000) = (20000+60000)/3000 = 80000/3000 ≈ 26.666...
        if let SignalValue::Scalar(x) = v {
            let expected = dec!(80000) / dec!(3000);
            assert!((x - expected).abs() < dec!(0.001));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_volume_weighted_range_not_ready() {
        let mut sig = VolumeWeightedRange::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "1000")).unwrap(), SignalValue::Unavailable);
    }
}
'''

files = [
    ("bar_efficiency.rs", bar_efficiency),
    ("chaikin_volatility.rs", chaikin_volatility),
    ("close_to_open_return.rs", close_to_open_return),
    ("volume_weighted_range.rs", volume_weighted_range),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
