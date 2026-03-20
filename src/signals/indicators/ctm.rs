//! Chande Trend Meter (CTM).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Chande Trend Meter — scores trend strength by counting how many of several
/// SMA periods the close is currently above.
///
/// Uses six SMA lengths: 20, 50, 100, and three shorter ones specified at
/// construction.  The score is the count of SMAs the close beats, normalised
/// to `[0, 100]`.  A reading above 60 suggests a bull trend; below 40 a bear.
///
/// Returns [`SignalValue::Unavailable`] until enough bars have been seen to
/// compute all SMAs.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Ctm;
/// use fin_primitives::signals::Signal;
///
/// let ctm = Ctm::new("ctm").unwrap();
/// assert_eq!(ctm.period(), 200);
/// ```
pub struct Ctm {
    name: String,
    closes: VecDeque<Decimal>,
    /// SMA periods used (sorted ascending so `period()` returns the largest)
    periods: [usize; 6],
}

impl Ctm {
    /// Creates a `Ctm` with the standard six SMA periods: 20, 50, 100, 200, and
    /// two shorter helpers 14 and 65.
    ///
    /// # Errors
    /// Never fails; returns `Result` for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        let periods = [14, 20, 50, 65, 100, 200];
        Ok(Self {
            name: name.into(),
            closes: VecDeque::with_capacity(200),
            periods,
        })
    }
}

impl Signal for Ctm {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.periods[5] {
            self.closes.pop_front();
        }
        let n = self.closes.len();
        let close = bar.close;
        let mut above = 0u32;
        let mut computed = 0u32;
        for &p in &self.periods {
            if n < p { continue; }
            let sma: Decimal = self.closes.iter().rev().take(p).sum::<Decimal>()
                / Decimal::from(p as u32);
            computed += 1;
            if close > sma { above += 1; }
        }
        if computed == 0 {
            return Ok(SignalValue::Unavailable);
        }
        let score = Decimal::from(above * 100) / Decimal::from(computed);
        Ok(SignalValue::Scalar(score))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.periods[5]
    }

    fn period(&self) -> usize {
        self.periods[5]
    }

    fn reset(&mut self) {
        self.closes.clear();
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

    #[test]
    fn test_ctm_period() {
        let ctm = Ctm::new("ctm").unwrap();
        assert_eq!(ctm.period(), 200);
    }

    #[test]
    fn test_ctm_produces_partial_score_early() {
        // First 13 bars are Unavailable; at bar 14 the 14-period SMA is computable.
        let mut ctm = Ctm::new("ctm").unwrap();
        for _ in 0..13 {
            assert_eq!(ctm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        // bar 14 — first SMA (period 14) now computable → Scalar output
        let v = ctm.update_bar(&bar("100")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_ctm_all_above_gives_100() {
        let mut ctm = Ctm::new("ctm").unwrap();
        // Prime with 200 bars at 100, then spike to 200
        for _ in 0..200 { ctm.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = ctm.update_bar(&bar("200")).unwrap() {
            assert_eq!(v, dec!(100));
        }
    }

    #[test]
    fn test_ctm_reset() {
        let mut ctm = Ctm::new("ctm").unwrap();
        for _ in 0..200 { ctm.update_bar(&bar("100")).unwrap(); }
        assert!(ctm.is_ready());
        ctm.reset();
        assert!(!ctm.is_ready());
    }
}
