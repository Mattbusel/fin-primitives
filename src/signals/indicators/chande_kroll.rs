//! Chande Kroll Stop indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Chande Kroll Stop — a trend-following stop indicator based on ATR.
///
/// ```text
/// ATR_p   = mean(TR, p)
/// first_high_stop = max(high, p) − factor × ATR_p
/// first_low_stop  = min(low,  p) + factor × ATR_p
///
/// stop_short = max(first_high_stop, q bars)
/// stop_long  = min(first_low_stop,  q bars)
///
/// output = (stop_short − stop_long) / close × 100
/// ```
///
/// Positive output means stop_short > stop_long (normal); sign of close relative
/// to stops indicates trend direction. Use `stop_short()` and `stop_long()` for
/// the raw stop levels.
///
/// Returns [`SignalValue::Unavailable`] until `p + q` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChandeKrollStop;
/// use fin_primitives::signals::Signal;
///
/// let ck = ChandeKrollStop::new("ck", 10, "1.5".parse().unwrap(), 9).unwrap();
/// assert_eq!(ck.period(), 10);
/// ```
pub struct ChandeKrollStop {
    name: String,
    p: usize,
    factor: Decimal,
    q: usize,

    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    trs: VecDeque<Decimal>,
    prev_close: Option<Decimal>,

    first_highs: VecDeque<Decimal>,
    first_lows: VecDeque<Decimal>,

    stop_short: Option<Decimal>,
    stop_long: Option<Decimal>,
}

impl ChandeKrollStop {
    /// Creates a new `ChandeKrollStop`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `p == 0` or `q == 0`.
    /// Returns [`FinError::InvalidInput`] if `factor` is not positive.
    pub fn new(
        name: impl Into<String>,
        p: usize,
        factor: Decimal,
        q: usize,
    ) -> Result<Self, FinError> {
        if p == 0 { return Err(FinError::InvalidPeriod(p)); }
        if q == 0 { return Err(FinError::InvalidPeriod(q)); }
        if factor <= Decimal::ZERO {
            return Err(FinError::InvalidInput("factor must be positive".into()));
        }
        Ok(Self {
            name: name.into(),
            p,
            factor,
            q,
            highs: VecDeque::with_capacity(p),
            lows: VecDeque::with_capacity(p),
            trs: VecDeque::with_capacity(p),
            prev_close: None,
            first_highs: VecDeque::with_capacity(q),
            first_lows: VecDeque::with_capacity(q),
            stop_short: None,
            stop_long: None,
        })
    }

    /// Returns the current short stop level.
    pub fn stop_short(&self) -> Option<Decimal> { self.stop_short }
    /// Returns the current long stop level.
    pub fn stop_long(&self) -> Option<Decimal> { self.stop_long }
}

impl Signal for ChandeKrollStop {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        self.trs.push_back(tr);
        if self.highs.len() > self.p { self.highs.pop_front(); }
        if self.lows.len() > self.p { self.lows.pop_front(); }
        if self.trs.len() > self.p { self.trs.pop_front(); }

        if self.trs.len() < self.p {
            return Ok(SignalValue::Unavailable);
        }

        let n = Decimal::from(self.p as u32);
        let atr = self.trs.iter().sum::<Decimal>() / n;
        let period_high = self.highs.iter().cloned().max().unwrap();
        let period_low = self.lows.iter().cloned().min().unwrap();

        let fhs = period_high - self.factor * atr;
        let fls = period_low + self.factor * atr;

        self.first_highs.push_back(fhs);
        self.first_lows.push_back(fls);
        if self.first_highs.len() > self.q { self.first_highs.pop_front(); }
        if self.first_lows.len() > self.q { self.first_lows.pop_front(); }

        if self.first_highs.len() < self.q {
            return Ok(SignalValue::Unavailable);
        }

        let ss = self.first_highs.iter().cloned().max().unwrap();
        let sl = self.first_lows.iter().cloned().min().unwrap();
        self.stop_short = Some(ss);
        self.stop_long = Some(sl);

        if bar.close.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let spread = (ss - sl) / bar.close * Decimal::from(100u32);
        Ok(SignalValue::Scalar(spread))
    }

    fn is_ready(&self) -> bool { self.stop_short.is_some() }
    fn period(&self) -> usize { self.p }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.trs.clear();
        self.prev_close = None;
        self.first_highs.clear();
        self.first_lows.clear();
        self.stop_short = None;
        self.stop_long = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
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
    fn test_ck_invalid() {
        assert!(ChandeKrollStop::new("c", 0, dec!(1.5), 9).is_err());
        assert!(ChandeKrollStop::new("c", 10, dec!(0), 9).is_err());
        assert!(ChandeKrollStop::new("c", 10, dec!(1.5), 0).is_err());
    }

    #[test]
    fn test_ck_unavailable_before_warmup() {
        let mut ck = ChandeKrollStop::new("c", 3, dec!(1.5), 3).unwrap();
        assert_eq!(ck.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!ck.is_ready());
    }

    #[test]
    fn test_ck_stop_levels_set() {
        // factor=0.3: 2*0.3*ATR < range → stops don't cross
        let mut ck = ChandeKrollStop::new("c", 3, dec!(0.3), 2).unwrap();
        for _ in 0..5 { ck.update_bar(&bar_hlc("105", "95", "100")).unwrap(); }
        assert!(ck.stop_short().is_some());
        assert!(ck.stop_long().is_some());
        // stop_short >= stop_long when factor is small enough
        assert!(ck.stop_short().unwrap() >= ck.stop_long().unwrap());
    }

    #[test]
    fn test_ck_flat_stops_symmetric() {
        // Flat bars: high=low=close → ATR=0 → fhs=period_high, fls=period_low
        let mut ck = ChandeKrollStop::new("c", 2, dec!(1.5), 2).unwrap();
        for _ in 0..4 { ck.update_bar(&bar("100")).unwrap(); }
        // Both stops equal close (100) since ATR=0 and high=low
        assert_eq!(ck.stop_short(), Some(dec!(100)));
        assert_eq!(ck.stop_long(), Some(dec!(100)));
    }

    #[test]
    fn test_ck_reset() {
        let mut ck = ChandeKrollStop::new("c", 2, dec!(1.5), 2).unwrap();
        for _ in 0..5 { ck.update_bar(&bar("100")).unwrap(); }
        assert!(ck.is_ready());
        ck.reset();
        assert!(!ck.is_ready());
        assert!(ck.stop_short().is_none());
        assert!(ck.stop_long().is_none());
    }
}
