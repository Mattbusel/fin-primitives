import os

base = "src/signals/indicators"

rolling_max_return = """\
//! Rolling Max Return indicator -- highest close-to-close return in a window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Max Return -- the largest single-bar close-to-close return observed
/// within the last `period` bars.
///
/// ```text
/// return[t]     = close[t] - close[t-1]
/// max_return[t] = max(return[t-period+1..t])
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen
/// (need `period + 1` closes to compute `period` returns).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingMaxReturn;
/// use fin_primitives::signals::Signal;
/// let rmr = RollingMaxReturn::new("rmr", 10).unwrap();
/// assert_eq!(rmr.period(), 10);
/// ```
pub struct RollingMaxReturn {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl RollingMaxReturn {
    /// Constructs a new `RollingMaxReturn`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingMaxReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => { self.prev_close = Some(bar.close); return Ok(SignalValue::Unavailable); }
            Some(pc) => bar.close - pc,
        };
        self.prev_close = Some(bar.close);
        self.returns.push_back(result);
        if self.returns.len() > self.period { self.returns.pop_front(); }
        if self.returns.len() < self.period { return Ok(SignalValue::Unavailable); }
        let max = self.returns.iter().copied().fold(Decimal::MIN, Decimal::max);
        Ok(SignalValue::Scalar(max))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.returns.clear();
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
    fn test_rmr_period_0_error() { assert!(RollingMaxReturn::new("r", 0).is_err()); }

    #[test]
    fn test_rmr_unavailable_before_warmup() {
        let mut r = RollingMaxReturn::new("r", 3).unwrap();
        // first bar: no prev -> Unavailable
        assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        // 2nd and 3rd bars: have 1 and 2 returns, < period=3 -> Unavailable
        assert_eq!(r.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert_eq!(r.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rmr_max_return_correct() {
        let mut r = RollingMaxReturn::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap(); // seed
        r.update_bar(&bar("110")).unwrap(); // return=+10
        r.update_bar(&bar("105")).unwrap(); // return=-5
        let v = r.update_bar(&bar("108")).unwrap(); // return=+3, window=[10,-5,3], max=10
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_rmr_rolling_drops_old_max() {
        let mut r = RollingMaxReturn::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("110")).unwrap(); // +10
        r.update_bar(&bar("111")).unwrap(); // +1
        r.update_bar(&bar("113")).unwrap(); // +2, window=[10,1,2], max=10
        // Now drop +10 from window
        let v = r.update_bar(&bar("114")).unwrap(); // +1, window=[1,2,1], max=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_rmr_reset() {
        let mut r = RollingMaxReturn::new("r", 2).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("110")).unwrap();
        r.update_bar(&bar("120")).unwrap();
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
"""

rolling_min_return = """\
//! Rolling Min Return indicator -- lowest close-to-close return in a window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Min Return -- the most negative single-bar return (worst bar) within the
/// last `period` bars.
///
/// ```text
/// return[t]     = close[t] - close[t-1]
/// min_return[t] = min(return[t-period+1..t])
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` returns have been accumulated
/// (needs `period + 1` closes).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingMinReturn;
/// use fin_primitives::signals::Signal;
/// let rmr = RollingMinReturn::new("rmr", 10).unwrap();
/// assert_eq!(rmr.period(), 10);
/// ```
pub struct RollingMinReturn {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl RollingMinReturn {
    /// Constructs a new `RollingMinReturn`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingMinReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ret = match self.prev_close {
            None => { self.prev_close = Some(bar.close); return Ok(SignalValue::Unavailable); }
            Some(pc) => bar.close - pc,
        };
        self.prev_close = Some(bar.close);
        self.returns.push_back(ret);
        if self.returns.len() > self.period { self.returns.pop_front(); }
        if self.returns.len() < self.period { return Ok(SignalValue::Unavailable); }
        let min = self.returns.iter().copied().fold(Decimal::MAX, Decimal::min);
        Ok(SignalValue::Scalar(min))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.returns.clear();
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
    fn test_rminr_period_0_error() { assert!(RollingMinReturn::new("r", 0).is_err()); }

    #[test]
    fn test_rminr_unavailable_before_warmup() {
        let mut r = RollingMinReturn::new("r", 3).unwrap();
        assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(r.update_bar(&bar("99")).unwrap(), SignalValue::Unavailable);
        assert_eq!(r.update_bar(&bar("98")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rminr_min_return_correct() {
        let mut r = RollingMinReturn::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("90")).unwrap();  // -10
        r.update_bar(&bar("95")).unwrap();  // +5
        let v = r.update_bar(&bar("93")).unwrap(); // -2, window=[-10,5,-2], min=-10
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_rminr_reset() {
        let mut r = RollingMinReturn::new("r", 2).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("90")).unwrap();
        r.update_bar(&bar("80")).unwrap();
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
"""

close_vs_prior_high = """\
//! Close vs Prior High indicator -- ratio of current close to the N-period prior high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close vs Prior High -- how the current close compares to the highest high seen
/// over the prior `period` bars (excluding the current bar).
///
/// ```text
/// prior_high[t] = max(high[t-period..t-1])
/// ratio[t]      = close[t] / prior_high[t]
/// ```
///
/// A ratio above 1 means close broke above the prior-period high (bullish breakout).
/// Below 1 means close remains under the prior high.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (need to fill the prior-high window before comparing).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseVsPriorHigh;
/// use fin_primitives::signals::Signal;
/// let cvph = CloseVsPriorHigh::new("cvph", 20).unwrap();
/// assert_eq!(cvph.period(), 20);
/// ```
pub struct CloseVsPriorHigh {
    name: String,
    period: usize,
    prior_highs: VecDeque<Decimal>,
}

impl CloseVsPriorHigh {
    /// Constructs a new `CloseVsPriorHigh`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prior_highs: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseVsPriorHigh {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.prior_highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Compare close against the current prior-high window BEFORE updating
        let result = if self.prior_highs.len() >= self.period {
            let prior_max = self.prior_highs.iter().copied().fold(Decimal::MIN, Decimal::max);
            if prior_max.is_zero() {
                SignalValue::Unavailable
            } else {
                SignalValue::Scalar(bar.close / prior_max)
            }
        } else {
            SignalValue::Unavailable
        };

        // Then slide the window
        self.prior_highs.push_back(bar.high);
        if self.prior_highs.len() > self.period { self.prior_highs.pop_front(); }

        Ok(result)
    }

    fn reset(&mut self) {
        self.prior_highs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let low = cp.min(hp);
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cvph_period_0_error() { assert!(CloseVsPriorHigh::new("c", 0).is_err()); }

    #[test]
    fn test_cvph_unavailable_during_warmup() {
        let mut c = CloseVsPriorHigh::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("110", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("110", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("110", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cvph_breakout_above_one() {
        let mut c = CloseVsPriorHigh::new("c", 3).unwrap();
        // seed 3 bars with high=100
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        // 4th bar: prior high=100, close=110 -> ratio=1.1
        let v = c.update_bar(&bar("115", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(110) / dec!(100)));
    }

    #[test]
    fn test_cvph_below_prior_high() {
        let mut c = CloseVsPriorHigh::new("c", 3).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        // close=90, prior high=100 -> ratio=0.9
        let v = c.update_bar(&bar("95", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(90) / dec!(100)));
    }

    #[test]
    fn test_cvph_reset() {
        let mut c = CloseVsPriorHigh::new("c", 2).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        c.update_bar(&bar("100", "95")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
"""

intraday_spread_pct = """\
//! Intraday Spread Percent indicator -- bar spread as a percentage of the midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Intraday Spread Percent -- models the bar's bid-ask spread proxy as the range
/// expressed as a percentage of the midpoint.
///
/// ```text
/// spread_pct[t] = (high - low) / ((high + low) / 2) x 100
/// ```
///
/// High values indicate wide spreads (illiquid or volatile); low values indicate
/// tight spreads (liquid or calm market).
///
/// Returns [`SignalValue::Unavailable`] if `high + low == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::IntradaySpreadPct;
/// use fin_primitives::signals::Signal;
/// let isp = IntradaySpreadPct::new("isp");
/// assert_eq!(isp.period(), 1);
/// ```
pub struct IntradaySpreadPct {
    name: String,
}

impl IntradaySpreadPct {
    /// Constructs a new `IntradaySpreadPct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for IntradaySpreadPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = bar.high + bar.low;
        if mid.is_zero() { return Ok(SignalValue::Unavailable); }
        let spread_pct = (bar.high - bar.low) / mid * Decimal::from(200u32);
        Ok(SignalValue::Scalar(spread_pct))
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_isp_zero_range_is_zero() {
        let mut isp = IntradaySpreadPct::new("isp");
        let v = isp.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_isp_spread_correct() {
        // high=110, low=90, mid=100, spread=20 -> 20/100*100 = 20%
        let mut isp = IntradaySpreadPct::new("isp");
        let v = isp.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_isp_always_ready() {
        let isp = IntradaySpreadPct::new("isp");
        assert!(isp.is_ready());
    }

    #[test]
    fn test_isp_period_is_1() {
        let isp = IntradaySpreadPct::new("isp");
        assert_eq!(isp.period(), 1);
    }
}
"""

files = {
    "rolling_max_return": rolling_max_return,
    "rolling_min_return": rolling_min_return,
    "close_vs_prior_high": close_vs_prior_high,
    "intraday_spread_pct": intraday_spread_pct,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
