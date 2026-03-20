import os

base = "src/signals/indicators"

price_reversal_strength = """\
//! Price Reversal Strength indicator -- magnitude of closing reversals from extreme.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Reversal Strength -- measures how strongly price reversed from the intrabar
/// extreme back toward the close.
///
/// For a bar that reached a high extreme:
/// ```text
/// bull_reversal[t] = (close - low) / (high - low) * 100  (from low extreme)
/// ```
///
/// This rolling average indicates how consistently bars recover from their intraday
/// lows. High values signal strong buying at dips; low values signal weak recoveries.
///
/// Returns [`SignalValue::Unavailable`] until `period` valid (non-zero-range) bars
/// have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceReversalStrength;
/// use fin_primitives::signals::Signal;
/// let prs = PriceReversalStrength::new("prs", 14).unwrap();
/// assert_eq!(prs.period(), 14);
/// ```
pub struct PriceReversalStrength {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceReversalStrength {
    /// Constructs a new `PriceReversalStrength`.
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

impl Signal for PriceReversalStrength {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let strength = (bar.close - bar.low) / range * Decimal::ONE_HUNDRED;
        self.window.push_back(strength);
        self.sum += strength;
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
    fn test_prs_period_0_error() { assert!(PriceReversalStrength::new("prs", 0).is_err()); }

    #[test]
    fn test_prs_zero_range_unavailable() {
        let mut prs = PriceReversalStrength::new("prs", 1).unwrap();
        let v = prs.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_prs_close_at_high_is_100() {
        // close == high -> (high-low)/(high-low)*100 = 100
        let mut prs = PriceReversalStrength::new("prs", 1).unwrap();
        let v = prs.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_prs_close_at_low_is_0() {
        let mut prs = PriceReversalStrength::new("prs", 1).unwrap();
        let v = prs.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_prs_rolling_average() {
        let mut prs = PriceReversalStrength::new("prs", 2).unwrap();
        prs.update_bar(&bar("110", "90", "110")).unwrap(); // 100%
        let v = prs.update_bar(&bar("110", "90", "90")).unwrap(); // 0% -> avg=50
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_prs_reset() {
        let mut prs = PriceReversalStrength::new("prs", 2).unwrap();
        prs.update_bar(&bar("110", "90", "100")).unwrap();
        prs.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(prs.is_ready());
        prs.reset();
        assert!(!prs.is_ready());
    }
}
"""

gap_fill_detector = """\
//! Gap Fill Detector indicator -- detects when a previous gap is filled.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap Fill Detector -- returns 1 when the current bar's range fills a previous
/// gap (i.e., trades through the prior gap's zone), and 0 otherwise.
///
/// A gap-up occurs when `open[t] > close[t-1]`. It's "filled" when a subsequent
/// bar's low falls at or below `close[t-1]` (the gap's lower boundary).
/// A gap-down occurs when `open[t] < close[t-1]`. It's "filled" when a bar's
/// high rises to or above `close[t-1]`.
///
/// This indicator tracks the most recent gap and signals when it is filled.
///
/// ```text
/// gap_up_target   = prev_close[gap_bar]   (lower edge of a gap-up)
/// gap_filled[t]   = 1 if gap exists AND low[t] <= gap_up_target
///                   1 if gap exists AND high[t] >= gap_down_target
///                   0 otherwise
/// ```
///
/// Returns 0 on the first bar (no prior close). Ready after the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapFillDetector;
/// use fin_primitives::signals::Signal;
/// let gfd = GapFillDetector::new("gfd");
/// assert_eq!(gfd.period(), 1);
/// ```
pub struct GapFillDetector {
    name: String,
    prev_close: Option<Decimal>,
    gap_target: Option<Decimal>,   // level that fills the gap
    gap_direction: i8,             // +1 = gap up (fill by going down), -1 = gap down
}

impl GapFillDetector {
    /// Constructs a new `GapFillDetector`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prev_close: None,
            gap_target: None,
            gap_direction: 0,
        }
    }
}

impl Signal for GapFillDetector {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pc) = self.prev_close {
            // Detect new gap
            if bar.open > pc {
                // Gap up: fill when low <= prev_close
                self.gap_target = Some(pc);
                self.gap_direction = 1;
            } else if bar.open < pc {
                // Gap down: fill when high >= prev_close
                self.gap_target = Some(pc);
                self.gap_direction = -1;
            }
            // Check if current gap is filled
            let filled = match (self.gap_target, self.gap_direction) {
                (Some(target), 1)  if bar.low  <= target => Decimal::ONE,
                (Some(target), -1) if bar.high >= target => Decimal::ONE,
                _ => Decimal::ZERO,
            };
            // If gap was filled, clear it
            if filled == Decimal::ONE {
                self.gap_target = None;
                self.gap_direction = 0;
            }
            filled
        } else {
            Decimal::ZERO
        };
        self.prev_close = Some(bar.close);
        Ok(SignalValue::Scalar(result))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gap_target = None;
        self.gap_direction = 0;
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
    fn test_gfd_first_bar_is_zero() {
        let mut gfd = GapFillDetector::new("gfd");
        let v = gfd.update_bar(&bar("100", "105", "95", "102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_gfd_gap_up_not_filled() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap(); // prev_close=100
        // Gap up: open=105 > prev_close=100, low=102 > 100 -> not filled
        let v = gfd.update_bar(&bar("105", "110", "102", "108")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_gfd_gap_up_filled() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap(); // prev_close=100
        // Gap up bar: open=105 > 100, gap target = 100
        gfd.update_bar(&bar("105", "110", "102", "108")).unwrap(); // gap set to 100
        // Fill bar: low=99 <= 100 -> filled!
        let v = gfd.update_bar(&bar("108", "110", "99", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_gfd_gap_down_filled() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap(); // prev_close=100
        // Gap down bar: open=95 < 100, gap target = 100
        gfd.update_bar(&bar("95", "98", "90", "92")).unwrap(); // gap set to 100
        // Fill bar: high=101 >= 100 -> filled!
        let v = gfd.update_bar(&bar("92", "101", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_gfd_reset() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap();
        assert!(gfd.is_ready());
        gfd.reset();
        assert!(!gfd.is_ready());
    }
}
"""

return_persistence = """\
//! Return Persistence indicator -- rolling fraction of returns with the same sign.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Persistence -- rolling fraction of consecutive return-sign pairs where
/// the current return has the same sign as the previous return.
///
/// Measures momentum persistence: high values mean returns tend to continue in the
/// same direction (trending); low values mean frequent reversals (mean-reverting).
///
/// ```text
/// ret[t]        = close[t] - close[t-1]
/// persist[t]    = 1 if sign(ret[t]) == sign(ret[t-1]), else 0
/// pct[t]        = sum(persist, period) / period * 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 2` bars have been seen
/// (need at least `period` sign-pair comparisons).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnPersistence;
/// use fin_primitives::signals::Signal;
/// let rp = ReturnPersistence::new("rp", 10).unwrap();
/// assert_eq!(rp.period(), 10);
/// ```
pub struct ReturnPersistence {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    prev_ret: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl ReturnPersistence {
    /// Constructs a new `ReturnPersistence`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            prev_ret: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for ReturnPersistence {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let ret = bar.close - pc;
            if let Some(pr) = self.prev_ret {
                // Same sign (both positive, both negative, or both zero)
                let persist: u8 = if (ret > Decimal::ZERO && pr > Decimal::ZERO)
                    || (ret < Decimal::ZERO && pr < Decimal::ZERO)
                    || (ret.is_zero() && pr.is_zero())
                { 1 } else { 0 };
                self.window.push_back(persist);
                self.count += persist as usize;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
                }
            }
            self.prev_ret = Some(ret);
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
        self.prev_ret = None;
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
    fn test_rp_period_0_error() { assert!(ReturnPersistence::new("rp", 0).is_err()); }

    #[test]
    fn test_rp_unavailable_before_period() {
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        assert_eq!(rp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rp.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rp_trending_series_is_100() {
        // Monotone rising -> all returns same sign -> 100% persistence
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        rp.update_bar(&bar("100")).unwrap();
        rp.update_bar(&bar("101")).unwrap(); // ret=+1, no prev_ret yet
        rp.update_bar(&bar("102")).unwrap(); // ret=+1, persist=1 -> window=[1]
        rp.update_bar(&bar("103")).unwrap(); // ret=+1, persist=1 -> window=[1,1]
        let v = rp.update_bar(&bar("104")).unwrap(); // ret=+1, persist=1 -> window=[1,1,1] -> 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rp_alternating_series_is_0() {
        // Alternating returns -> no consecutive same-sign pairs -> 0%
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        rp.update_bar(&bar("100")).unwrap();
        rp.update_bar(&bar("101")).unwrap(); // +1
        rp.update_bar(&bar("100")).unwrap(); // -1 -> persist=0, window=[0]
        rp.update_bar(&bar("101")).unwrap(); // +1 -> persist=0, window=[0,0]
        let v = rp.update_bar(&bar("100")).unwrap(); // -1 -> persist=0, window=[0,0,0] -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rp_reset() {
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        for p in ["100", "101", "102", "103", "104"] { rp.update_bar(&bar(p)).unwrap(); }
        assert!(rp.is_ready());
        rp.reset();
        assert!(!rp.is_ready());
    }
}
"""

files = {
    "price_reversal_strength": price_reversal_strength,
    "gap_fill_detector": gap_fill_detector,
    "return_persistence": return_persistence,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
