import os

base = "src/signals/indicators"

conditional_var5 = """\
//! Conditional Value at Risk 5% (CVaR / Expected Shortfall) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Conditional VaR 5% (CVaR95 / Expected Shortfall) -- the average close-to-close
/// return over the worst 5% of observations in the rolling window.
///
/// More conservative than VaR5: while VaR tells you the threshold, CVaR tells
/// you the expected loss *given* that the worst 5% scenario has occurred.
///
/// ```text
/// return[t]  = (close[t] - close[t-1]) / close[t-1] * 100
/// cvar5[t]   = mean of the bottom ceil(period * 0.05) returns
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConditionalVar5;
/// use fin_primitives::signals::Signal;
/// let cv = ConditionalVar5::new("cvar5", 20).unwrap();
/// assert_eq!(cv.period(), 20);
/// ```
pub struct ConditionalVar5 {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl ConditionalVar5 {
    /// Constructs a new `ConditionalVar5`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ConditionalVar5 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(ret);
                if self.window.len() > self.period {
                    self.window.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let mut sorted: Vec<Decimal> = self.window.iter().copied().collect();
        sorted.sort();
        // Take bottom ceil(5%) of observations
        let tail_count = ((self.period as f64 * 0.05).ceil() as usize).max(1);
        let tail = &sorted[..tail_count.min(sorted.len())];
        let sum: Decimal = tail.iter().sum();
        #[allow(clippy::cast_possible_truncation)]
        let mean = sum / Decimal::from(tail.len() as u32);
        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cvar5_period_0_error() { assert!(ConditionalVar5::new("cv", 0).is_err()); }

    #[test]
    fn test_cvar5_unavailable_before_period() {
        let mut cv = ConditionalVar5::new("cv", 5).unwrap();
        assert_eq!(cv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cvar5_all_negative_returns() {
        // Falling prices -> all returns negative -> CVaR is the worst
        let mut cv = ConditionalVar5::new("cv", 4).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("99")).unwrap();  // -1%
        cv.update_bar(&bar("98")).unwrap();  // ~-1.01%
        cv.update_bar(&bar("97")).unwrap();  // ~-1.02%
        let r = cv.update_bar(&bar("96")).unwrap(); // ~-1.03%
        if let SignalValue::Scalar(cvar) = r {
            assert!(cvar < dec!(0), "all negative returns, CVaR should be negative, got {cvar}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cvar5_big_loss_dominates() {
        // One huge loss should pull down CVaR
        let mut cv = ConditionalVar5::new("cv", 4).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("50")).unwrap(); // -50%
        cv.update_bar(&bar("51")).unwrap();
        cv.update_bar(&bar("52")).unwrap();
        let r = cv.update_bar(&bar("53")).unwrap();
        if let SignalValue::Scalar(cvar) = r {
            // The -50% loss should be in the worst 5% tail, pulling CVaR negative
            assert!(cvar < dec!(0), "big loss present, CVaR should be negative, got {cvar}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cvar5_reset() {
        let mut cv = ConditionalVar5::new("cv", 3).unwrap();
        for p in ["100", "101", "102", "103"] { cv.update_bar(&bar(p)).unwrap(); }
        assert!(cv.is_ready());
        cv.reset();
        assert!(!cv.is_ready());
    }
}
"""

abs_return_mean = """\
//! Absolute Return Mean indicator -- rolling mean of absolute close-to-close returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Absolute Return Mean -- rolling average of |close[t] - close[t-1]|.
///
/// A volatility proxy that is scale-invariant and intuitive: it represents
/// the average absolute price move per bar over the period.
///
/// ```text
/// abs_ret[t] = |close[t] - close[t-1]|
/// mean[t]    = SMA(abs_ret, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AbsReturnMean;
/// use fin_primitives::signals::Signal;
/// let arm = AbsReturnMean::new("arm", 14).unwrap();
/// assert_eq!(arm.period(), 14);
/// ```
pub struct AbsReturnMean {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl AbsReturnMean {
    /// Constructs a new `AbsReturnMean`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for AbsReturnMean {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let abs_ret = (bar.close - pc).abs();
            self.window.push_back(abs_ret);
            self.sum += abs_ret;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.sum -= old; }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_arm_period_0_error() { assert!(AbsReturnMean::new("arm", 0).is_err()); }

    #[test]
    fn test_arm_unavailable_before_period() {
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        assert_eq!(arm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(arm.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_arm_flat_price_is_zero() {
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        let v = arm.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_arm_constant_moves() {
        // Alternating +5/-5 -> abs_ret = 5 each -> mean = 5
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("105")).unwrap(); // |+5|
        arm.update_bar(&bar("100")).unwrap(); // |-5|
        let v = arm.update_bar(&bar("105")).unwrap(); // |+5| -> mean([5,5,5]) = 5
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_arm_window_slides() {
        // period=2: [5,10] -> mean=7.5 then [10,3] -> mean=6.5
        let mut arm = AbsReturnMean::new("arm", 2).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("105")).unwrap(); // |5|
        arm.update_bar(&bar("95")).unwrap();  // |10| -> mean([5,10]) = 7.5
        let v = arm.update_bar(&bar("98")).unwrap(); // |3| -> mean([10,3]) = 6.5
        assert_eq!(v, SignalValue::Scalar(dec!(6.5)));
    }

    #[test]
    fn test_arm_reset() {
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        for p in ["100", "101", "102", "103"] { arm.update_bar(&bar(p)).unwrap(); }
        assert!(arm.is_ready());
        arm.reset();
        assert!(!arm.is_ready());
    }
}
"""

flat_bar_pct = """\
//! Flat Bar Percent indicator -- rolling % of bars where close equals previous close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Flat Bar Percent -- rolling percentage of bars where `close == prev_close`.
///
/// High values indicate a stagnant, illiquid, or range-bound market where the price
/// repeatedly fails to move. Useful for filtering signals in low-activity periods.
///
/// ```text
/// flat[t]  = 1 if close[t] == close[t-1], else 0
/// pct[t]   = sum(flat, period) / period * 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` comparisons have been made
/// (requires `period + 1` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::FlatBarPct;
/// use fin_primitives::signals::Signal;
/// let fb = FlatBarPct::new("fb", 10).unwrap();
/// assert_eq!(fb.period(), 10);
/// ```
pub struct FlatBarPct {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl FlatBarPct {
    /// Constructs a new `FlatBarPct`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for FlatBarPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let flat: u8 = if bar.close == pc { 1 } else { 0 };
            self.window.push_back(flat);
            self.count += flat as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(self.count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_fb_period_0_error() { assert!(FlatBarPct::new("fb", 0).is_err()); }

    #[test]
    fn test_fb_unavailable_before_period() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        assert_eq!(fb.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_fb_all_flat_is_100() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        fb.update_bar(&bar("100")).unwrap(); // no comparison
        fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1]
        fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1,1]
        let v = fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1,1,1] -> 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_fb_no_flat_is_0() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        fb.update_bar(&bar("100")).unwrap();
        fb.update_bar(&bar("101")).unwrap();
        fb.update_bar(&bar("102")).unwrap();
        let v = fb.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_fb_half_flat() {
        let mut fb = FlatBarPct::new("fb", 2).unwrap();
        fb.update_bar(&bar("100")).unwrap();
        fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1]
        let v = fb.update_bar(&bar("101")).unwrap(); // not flat -> window=[1,0] -> 50%
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_fb_reset() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        for p in ["100", "100", "100", "100"] { fb.update_bar(&bar(p)).unwrap(); }
        assert!(fb.is_ready());
        fb.reset();
        assert!(!fb.is_ready());
    }
}
"""

upper_to_lower_wick = """\
//! Upper to Lower Wick Ratio indicator -- rolling mean of upper/lower wick ratio.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Upper to Lower Wick Ratio -- rolling average of `upper_wick / lower_wick` per bar.
///
/// Upper wick = `high - max(open, close)`.
/// Lower wick = `min(open, close) - low`.
///
/// Values > 1 indicate upper wicks dominate (bearish rejection), suggesting selling
/// pressure at highs. Values < 1 indicate lower wicks dominate (bullish rejection).
///
/// Bars with a zero lower wick are excluded from the rolling average.
///
/// Returns [`SignalValue::Unavailable`] until `period` valid bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpperToLowerWick;
/// use fin_primitives::signals::Signal;
/// let utlw = UpperToLowerWick::new("utlw", 14).unwrap();
/// assert_eq!(utlw.period(), 14);
/// ```
pub struct UpperToLowerWick {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl UpperToLowerWick {
    /// Constructs a new `UpperToLowerWick`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for UpperToLowerWick {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body_top = bar.open.max(bar.close);
        let body_bot = bar.open.min(bar.close);
        let upper_wick = bar.high - body_top;
        let lower_wick = body_bot - bar.low;
        if lower_wick.is_zero() { return Ok(SignalValue::Unavailable); }
        let ratio = upper_wick / lower_wick;
        self.window.push_back(ratio);
        self.sum += ratio;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_utlw_period_0_error() { assert!(UpperToLowerWick::new("utlw", 0).is_err()); }

    #[test]
    fn test_utlw_zero_lower_wick_unavailable() {
        // open=100, high=110, low=100, close=105: lower_wick = min(100,105) - 100 = 0
        let mut utlw = UpperToLowerWick::new("utlw", 1).unwrap();
        let v = utlw.update_bar(&bar("100", "110", "100", "105")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_utlw_equal_wicks() {
        // open=100, high=110, low=90, close=100: upper=(110-100)=10, lower=(100-90)=10, ratio=1
        let mut utlw = UpperToLowerWick::new("utlw", 1).unwrap();
        let v = utlw.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_utlw_double_upper_wick() {
        // open=100, high=120, low=90, close=100: upper=20, lower=10, ratio=2
        let mut utlw = UpperToLowerWick::new("utlw", 1).unwrap();
        let v = utlw.update_bar(&bar("100", "120", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_utlw_rolling_average() {
        // Two bars with equal wicks (ratio=1 each) -> avg=1
        let mut utlw = UpperToLowerWick::new("utlw", 2).unwrap();
        utlw.update_bar(&bar("100", "110", "90", "100")).unwrap(); // ratio=1
        let v = utlw.update_bar(&bar("100", "110", "90", "100")).unwrap(); // ratio=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_utlw_reset() {
        let mut utlw = UpperToLowerWick::new("utlw", 2).unwrap();
        utlw.update_bar(&bar("100", "110", "90", "100")).unwrap();
        utlw.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(utlw.is_ready());
        utlw.reset();
        assert!(!utlw.is_ready());
    }
}
"""

files = {
    "conditional_var5": conditional_var5,
    "abs_return_mean": abs_return_mean,
    "flat_bar_pct": flat_bar_pct,
    "upper_to_lower_wick": upper_to_lower_wick,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
