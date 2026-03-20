import os

base = "src/signals/indicators"

momentum_quality = """\
//! Momentum Quality indicator -- fraction of period gains that are bars-above-SMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Momentum Quality -- ratio of positive momentum bars (close above rolling SMA)
/// to total bars in the period, expressed as a percentage.
///
/// A high value (near 100%) indicates consistent bullish momentum with the price
/// rarely dipping below its average. A low value indicates choppy or bearish action.
///
/// ```text
/// sma[t]   = SMA(close, period)
/// above[t] = 1 if close[t] > sma[t], else 0
/// mq[t]    = sum(above, period) / period * 100
/// ```
///
/// Note: uses the concurrent SMA for each bar, so the first `period - 1` bars are
/// all considered "not above SMA" until the SMA is ready.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MomentumQuality;
/// use fin_primitives::signals::Signal;
/// let mq = MomentumQuality::new("mq", 14).unwrap();
/// assert_eq!(mq.period(), 14);
/// ```
pub struct MomentumQuality {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
    above_window: VecDeque<u8>,
    above_count: usize,
}

impl MomentumQuality {
    /// Constructs a new `MomentumQuality`.
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
            above_window: VecDeque::with_capacity(period),
            above_count: 0,
        })
    }
}

impl Signal for MomentumQuality {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Update SMA window
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        // Compute current SMA and check if close is above
        #[allow(clippy::cast_possible_truncation)]
        let above: u8 = if self.window.len() == self.period {
            let sma = self.sum / Decimal::from(self.period as u32);
            if bar.close > sma { 1 } else { 0 }
        } else {
            0 // Not yet enough data for SMA -> count as not above
        };
        self.above_window.push_back(above);
        self.above_count += above as usize;
        if self.above_window.len() > self.period {
            if let Some(old) = self.above_window.pop_front() { self.above_count -= old as usize; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let mq = Decimal::from(self.above_count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(mq))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
        self.above_window.clear();
        self.above_count = 0;
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
    fn test_mq_period_0_error() { assert!(MomentumQuality::new("mq", 0).is_err()); }

    #[test]
    fn test_mq_unavailable_before_period() {
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        assert_eq!(mq.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_mq_flat_price_is_0_or_low() {
        // Flat price: close == SMA always, so never strictly above -> 0%
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        mq.update_bar(&bar("100")).unwrap();
        mq.update_bar(&bar("100")).unwrap();
        let v = mq.update_bar(&bar("100")).unwrap();
        // SMA = 100, close = 100, not strictly above -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mq_strongly_rising_is_positive() {
        // Rising prices: last bars are above SMA -> positive MQ
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        mq.update_bar(&bar("100")).unwrap();
        mq.update_bar(&bar("110")).unwrap();
        let v = mq.update_bar(&bar("120")).unwrap();
        // SMA = (100+110+120)/3 = 110, close=120 > 110 -> above=1 in last bar
        if let SignalValue::Scalar(q) = v {
            assert!(q > dec!(0), "rising prices, some above SMA, got {q}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mq_reset() {
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        for p in ["100", "110", "120"] { mq.update_bar(&bar(p)).unwrap(); }
        assert!(mq.is_ready());
        mq.reset();
        assert!(!mq.is_ready());
    }
}
"""

rolling_max_dd = """\
//! Rolling Maximum Drawdown indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Maximum Drawdown -- the largest peak-to-trough decline within the
/// rolling `period`-bar window.
///
/// ```text
/// peak_so_far[t]    = max(close, period)
/// dd[t]             = (close[t] - peak[t]) / peak[t] * 100   (always <= 0)
/// max_dd[t]         = min of dd values over the period
/// ```
///
/// Returns a negative percentage representing the worst drawdown.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingMaxDd;
/// use fin_primitives::signals::Signal;
/// let rmdd = RollingMaxDd::new("rmdd", 20).unwrap();
/// assert_eq!(rmdd.period(), 20);
/// ```
pub struct RollingMaxDd {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl RollingMaxDd {
    /// Constructs a new `RollingMaxDd`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingMaxDd {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        // Compute peak-to-trough drawdown over the window
        let mut max_dd = Decimal::ZERO;
        let mut peak = Decimal::MIN;
        for &price in &self.window {
            if price > peak { peak = price; }
            if peak.is_zero() { continue; }
            let dd = (price - peak) / peak * Decimal::ONE_HUNDRED;
            if dd < max_dd { max_dd = dd; }
        }
        Ok(SignalValue::Scalar(max_dd))
    }

    fn reset(&mut self) {
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
    fn test_rmdd_period_0_error() { assert!(RollingMaxDd::new("rmdd", 0).is_err()); }

    #[test]
    fn test_rmdd_unavailable_before_period() {
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        assert_eq!(rmdd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rmdd_monotone_rising_no_drawdown() {
        // Rising prices -> no drawdown (peak always == close) -> max_dd = 0
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        rmdd.update_bar(&bar("100")).unwrap();
        rmdd.update_bar(&bar("110")).unwrap();
        let v = rmdd.update_bar(&bar("120")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rmdd_peak_then_trough() {
        // 100 -> 200 -> 100: dd = (100-200)/200*100 = -50%
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        rmdd.update_bar(&bar("100")).unwrap();
        rmdd.update_bar(&bar("200")).unwrap();
        let v = rmdd.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-50)));
    }

    #[test]
    fn test_rmdd_window_slides_out_peak() {
        // Once the peak bar slides out of the window, drawdown recovers
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        rmdd.update_bar(&bar("200")).unwrap(); // peak
        rmdd.update_bar(&bar("100")).unwrap(); // -50%
        rmdd.update_bar(&bar("110")).unwrap(); // -45%
        // Now slides: window=[100, 110, 120], peak=120, no drawdown
        let v = rmdd.update_bar(&bar("120")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rmdd_reset() {
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        for p in ["100", "200", "100"] { rmdd.update_bar(&bar(p)).unwrap(); }
        assert!(rmdd.is_ready());
        rmdd.reset();
        assert!(!rmdd.is_ready());
    }
}
"""

high_low_crossover = """\
//! High-Low Crossover indicator -- when close crosses the period high or low.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Crossover -- detects when close breaks to a new `period`-bar high or low.
///
/// Returns:
/// - `+1` if `close[t] == max(high, period)` (new period high)
/// - `-1` if `close[t] == min(low, period)`  (new period low)
/// - `0`  otherwise
///
/// Useful as a breakout signal: a new high or low often precedes continued movement.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowCrossover;
/// use fin_primitives::signals::Signal;
/// let hlx = HighLowCrossover::new("hlx", 20).unwrap();
/// assert_eq!(hlx.period(), 20);
/// ```
pub struct HighLowCrossover {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl HighLowCrossover {
    /// Constructs a new `HighLowCrossover`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for HighLowCrossover {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period { return Ok(SignalValue::Unavailable); }
        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low  = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let signal = if bar.close >= period_high {
            Decimal::ONE
        } else if bar.close <= period_low {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hlx_period_0_error() { assert!(HighLowCrossover::new("hlx", 0).is_err()); }

    #[test]
    fn test_hlx_unavailable_before_period() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        assert_eq!(hlx.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hlx_new_high_returns_plus1() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        hlx.update_bar(&bar("105", "95", "100")).unwrap();
        hlx.update_bar(&bar("107", "93", "105")).unwrap();
        // period_high = 110, close = 110 >= 110 -> +1
        let v = hlx.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hlx_new_low_returns_minus1() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        hlx.update_bar(&bar("105", "95", "100")).unwrap();
        hlx.update_bar(&bar("107", "93", "95")).unwrap();
        // period_low = 90, close = 90 <= 90 -> -1
        let v = hlx.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_hlx_middle_returns_zero() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        // period_high=110, period_low=90, close=100 -> 0
        let v = hlx.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hlx_reset() {
        let mut hlx = HighLowCrossover::new("hlx", 2).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(hlx.is_ready());
        hlx.reset();
        assert!(!hlx.is_ready());
    }
}
"""

body_direction_ratio = """\
//! Body Direction Ratio indicator -- rolling ratio of bullish to bearish candle bodies.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Direction Ratio -- rolling ratio of total bullish body size to total bearish
/// body size over `period` bars.
///
/// ```text
/// body[t]      = |close - open|
/// bull_body[t] = body[t] if close > open, else 0
/// bear_body[t] = body[t] if close < open, else 0
/// ratio[t]     = sum(bull_body, period) / sum(bear_body, period)
/// ```
///
/// Values > 1 indicate bulls have more total body size (buying momentum dominates).
/// Values < 1 indicate bears dominate. Returns 0 if no bearish bodies exist in window.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// total bearish body size is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyDirectionRatio;
/// use fin_primitives::signals::Signal;
/// let bdr = BodyDirectionRatio::new("bdr", 10).unwrap();
/// assert_eq!(bdr.period(), 10);
/// ```
pub struct BodyDirectionRatio {
    name: String,
    period: usize,
    bull_window: VecDeque<Decimal>,
    bear_window: VecDeque<Decimal>,
    bull_sum: Decimal,
    bear_sum: Decimal,
}

impl BodyDirectionRatio {
    /// Constructs a new `BodyDirectionRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            bull_window: VecDeque::with_capacity(period),
            bear_window: VecDeque::with_capacity(period),
            bull_sum: Decimal::ZERO,
            bear_sum: Decimal::ZERO,
        })
    }
}

impl Signal for BodyDirectionRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bull_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = (bar.close - bar.open).abs();
        let bull = if bar.close > bar.open { body } else { Decimal::ZERO };
        let bear = if bar.close < bar.open { body } else { Decimal::ZERO };
        self.bull_window.push_back(bull);
        self.bear_window.push_back(bear);
        self.bull_sum += bull;
        self.bear_sum += bear;
        if self.bull_window.len() > self.period {
            if let Some(old_b) = self.bull_window.pop_front() { self.bull_sum -= old_b; }
            if let Some(old_r) = self.bear_window.pop_front() { self.bear_sum -= old_r; }
        }
        if self.bull_window.len() < self.period { return Ok(SignalValue::Unavailable); }
        if self.bear_sum.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.bull_sum / self.bear_sum))
    }

    fn reset(&mut self) {
        self.bull_window.clear();
        self.bear_window.clear();
        self.bull_sum = Decimal::ZERO;
        self.bear_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bdr_period_0_error() { assert!(BodyDirectionRatio::new("bdr", 0).is_err()); }

    #[test]
    fn test_bdr_unavailable_before_period() {
        let mut bdr = BodyDirectionRatio::new("bdr", 3).unwrap();
        assert_eq!(bdr.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bdr_all_bullish_unavailable() {
        // No bearish bodies -> bear_sum=0 -> Unavailable
        let mut bdr = BodyDirectionRatio::new("bdr", 3).unwrap();
        bdr.update_bar(&bar("100", "105")).unwrap();
        bdr.update_bar(&bar("105", "110")).unwrap();
        let v = bdr.update_bar(&bar("110", "115")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_bdr_equal_bodies_is_1() {
        // Equal bull and bear bodies -> ratio = 1
        let mut bdr = BodyDirectionRatio::new("bdr", 2).unwrap();
        bdr.update_bar(&bar("100", "110")).unwrap(); // bull body = 10
        let v = bdr.update_bar(&bar("110", "100")).unwrap(); // bear body = 10, ratio=10/10=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bdr_bull_dominates() {
        // Bull body 20, bear body 10 -> ratio = 2
        let mut bdr = BodyDirectionRatio::new("bdr", 2).unwrap();
        bdr.update_bar(&bar("100", "120")).unwrap(); // bull=20
        let v = bdr.update_bar(&bar("120", "110")).unwrap(); // bear=10, ratio=20/10=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_bdr_reset() {
        let mut bdr = BodyDirectionRatio::new("bdr", 2).unwrap();
        bdr.update_bar(&bar("100", "110")).unwrap();
        bdr.update_bar(&bar("110", "100")).unwrap();
        assert!(bdr.is_ready());
        bdr.reset();
        assert!(!bdr.is_ready());
    }
}
"""

files = {
    "momentum_quality": momentum_quality,
    "rolling_max_dd": rolling_max_dd,
    "high_low_crossover": high_low_crossover,
    "body_direction_ratio": body_direction_ratio,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
