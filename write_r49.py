import os

base = "src/signals/indicators"

atr_ratio = """\
//! ATR Ratio indicator -- current ATR as a multiple of its rolling average.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ATR Ratio -- current True Range (1-bar ATR) divided by the SMA of True Range
/// over `period` bars.
///
/// ```text
/// tr[t]        = max(high-low, |high-prev_close|, |low-prev_close|)
/// atr_sma[t]   = SMA(tr, period)
/// atr_ratio[t] = tr[t] / atr_sma[t]
/// ```
///
/// A ratio > 1 means the current bar's range is above average (elevated volatility).
/// A ratio < 1 means below-average volatility (compression).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs prev_close and a full ATR window).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AtrRatio;
/// use fin_primitives::signals::Signal;
/// let ar = AtrRatio::new("ar", 14).unwrap();
/// assert_eq!(ar.period(), 14);
/// ```
pub struct AtrRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    tr_window: VecDeque<Decimal>,
    tr_sum: Decimal,
}

impl AtrRatio {
    /// Constructs a new `AtrRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            tr_window: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
        })
    }
}

impl Signal for AtrRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.tr_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let hl = bar.high - bar.low;
        let tr = match self.prev_close {
            None => hl,
            Some(pc) => {
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);

        self.tr_window.push_back(tr);
        self.tr_sum += tr;
        if self.tr_window.len() > self.period {
            if let Some(old) = self.tr_window.pop_front() { self.tr_sum -= old; }
        }
        if self.tr_window.len() < self.period { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let atr_sma = self.tr_sum / Decimal::from(self.period as u32);
        if atr_sma.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(tr / atr_sma))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.tr_window.clear();
        self.tr_sum = Decimal::ZERO;
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
    fn test_ar_period_0_error() { assert!(AtrRatio::new("ar", 0).is_err()); }

    #[test]
    fn test_ar_unavailable_before_period() {
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        assert_eq!(ar.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ar_constant_range_is_one() {
        // All bars have same range -> current TR == avg TR -> ratio = 1
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        for _ in 0..5 {
            ar.update_bar(&bar("110", "90", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = ar.update_bar(&bar("110", "90", "100")).unwrap() {
            let diff = (v - dec!(1)).abs();
            assert!(diff < dec!(0.001), "expected ~1, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ar_high_volatility_above_one() {
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        // seed with small ranges
        for _ in 0..3 { ar.update_bar(&bar("101", "99", "100")).unwrap(); }
        // then spike
        let v = ar.update_bar(&bar("120", "80", "100")).unwrap();
        if let SignalValue::Scalar(ratio) = v {
            assert!(ratio > dec!(1), "expected ratio > 1 for spike bar, got {ratio}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ar_reset() {
        let mut ar = AtrRatio::new("ar", 3).unwrap();
        for _ in 0..5 { ar.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(ar.is_ready());
        ar.reset();
        assert!(!ar.is_ready());
    }
}
"""

price_channel_position = """\
//! Price Channel Position indicator -- where close sits in the N-period Donchian channel.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Channel Position -- close's percentile position in the N-period Donchian channel.
///
/// ```text
/// channel_high[t] = max(high, period)
/// channel_low[t]  = min(low,  period)
/// position[t]     = (close - channel_low) / (channel_high - channel_low) x 100
/// ```
///
/// 100 means close is at the top of the channel; 0 means at the bottom.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or when
/// the channel width is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceChannelPosition;
/// use fin_primitives::signals::Signal;
/// let pcp = PriceChannelPosition::new("pcp", 20).unwrap();
/// assert_eq!(pcp.period(), 20);
/// ```
pub struct PriceChannelPosition {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceChannelPosition {
    /// Constructs a new `PriceChannelPosition`.
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

impl Signal for PriceChannelPosition {
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

        let ch = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let cl = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let width = ch - cl;
        if width.is_zero() { return Ok(SignalValue::Unavailable); }
        let pos = (bar.close - cl) / width * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pos))
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
    fn test_pcp_period_0_error() { assert!(PriceChannelPosition::new("p", 0).is_err()); }

    #[test]
    fn test_pcp_unavailable_before_period() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        assert_eq!(p.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pcp_close_at_channel_high() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        // close=110 = channel_high -> position=100
        let v = p.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pcp_close_at_channel_low() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        // close=90 = channel_low -> position=0
        let v = p.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcp_close_at_midpoint() {
        let mut p = PriceChannelPosition::new("p", 3).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        // close=100, channel=[90,110], width=20 -> position=50
        let v = p.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_pcp_reset() {
        let mut p = PriceChannelPosition::new("p", 2).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        p.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
"""

close_minus_open_ma = """\
//! Close Minus Open MA indicator -- SMA of bar body (close - open) over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Minus Open MA -- rolling SMA of the signed bar body `(close - open)`.
///
/// Positive values indicate net bullish pressure over the window;
/// negative values indicate net bearish pressure.
///
/// ```text
/// body[t]    = close[t] - open[t]
/// cmo_ma[t]  = SMA(body, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseMinusOpenMa;
/// use fin_primitives::signals::Signal;
/// let c = CloseMinusOpenMa::new("cmo_ma", 10).unwrap();
/// assert_eq!(c.period(), 10);
/// ```
pub struct CloseMinusOpenMa {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseMinusOpenMa {
    /// Constructs a new `CloseMinusOpenMa`.
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

impl Signal for CloseMinusOpenMa {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.close - bar.open;
        self.window.push_back(body);
        self.sum += body;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let avg = self.sum / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
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
    fn test_cmo_ma_period_0_error() { assert!(CloseMinusOpenMa::new("c", 0).is_err()); }

    #[test]
    fn test_cmo_ma_unavailable_before_period() {
        let mut c = CloseMinusOpenMa::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cmo_ma_all_bullish() {
        // body = 5 each bar
        let mut c = CloseMinusOpenMa::new("c", 3).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        let v = c.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_cmo_ma_mixed_near_zero() {
        let mut c = CloseMinusOpenMa::new("c", 2).unwrap();
        c.update_bar(&bar("100", "110")).unwrap(); // body=+10
        let v = c.update_bar(&bar("110", "100")).unwrap(); // body=-10, avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cmo_ma_reset() {
        let mut c = CloseMinusOpenMa::new("c", 2).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        c.update_bar(&bar("100", "105")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
"""

files = {
    "atr_ratio": atr_ratio,
    "price_channel_position": price_channel_position,
    "close_minus_open_ma": close_minus_open_ma,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
