//! Bar Range Expansion Percentage — rolling fraction of bars where range expanded vs prior bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Range Expansion Percentage — rolling fraction of bars with `range > prev_range`.
///
/// Measures how often the intrabar range is increasing relative to the prior bar:
/// - **High values** (near 1.0): range is consistently expanding — increasing volatility.
/// - **Low values** (near 0.0): range is consistently contracting — compression regime.
/// - **~0.5**: random alternation between expansion and contraction.
///
/// Returns the fraction over the last `period` bar-pairs.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarRangeExpansionPct;
/// use fin_primitives::signals::Signal;
/// let brep = BarRangeExpansionPct::new("brep_10", 10).unwrap();
/// assert_eq!(brep.period(), 10);
/// ```
pub struct BarRangeExpansionPct {
    name: String,
    period: usize,
    window: VecDeque<bool>,
    prev_range: Option<Decimal>,
    count: usize,
}

impl BarRangeExpansionPct {
    /// Constructs a new `BarRangeExpansionPct`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            prev_range: None,
            count: 0,
        })
    }
}

impl Signal for BarRangeExpansionPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let current_range = bar.range();

        if let Some(pr) = self.prev_range {
            let expanded = current_range > pr;
            self.window.push_back(expanded);
            if expanded { self.count += 1; }

            if self.window.len() > self.period {
                let removed = self.window.pop_front().unwrap();
                if removed { self.count -= 1; }
            }
        }

        self.prev_range = Some(current_range);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let frac = Decimal::from(self.count as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(frac))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.prev_range = None;
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
    fn test_brep_invalid_period() {
        assert!(BarRangeExpansionPct::new("brep", 0).is_err());
    }

    #[test]
    fn test_brep_unavailable_before_period() {
        let mut s = BarRangeExpansionPct::new("brep", 3).unwrap();
        // Need period+1 bars total before ready
        assert_eq!(s.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("112", "88")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_brep_always_expanding_gives_one() {
        // Ranges: 10, 20, 30, 40 — always expanding
        let mut s = BarRangeExpansionPct::new("brep", 3).unwrap();
        s.update_bar(&bar("105", "95")).unwrap(); // range=10, prev=None
        s.update_bar(&bar("110", "90")).unwrap(); // range=20, expanded=true
        s.update_bar(&bar("115", "85")).unwrap(); // range=30, expanded=true
        if let SignalValue::Scalar(v) = s.update_bar(&bar("120", "80")).unwrap() {
            // range=40, expanded=true → 3/3=1.0
            assert!((v - dec!(1)).abs() < dec!(0.001), "all expanding → 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_brep_always_contracting_gives_zero() {
        // Ranges: 40, 30, 20, 10 — always contracting
        let mut s = BarRangeExpansionPct::new("brep", 3).unwrap();
        s.update_bar(&bar("120", "80")).unwrap(); // range=40
        s.update_bar(&bar("115", "85")).unwrap(); // range=30, expanded=false
        s.update_bar(&bar("110", "90")).unwrap(); // range=20, expanded=false
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105", "95")).unwrap() {
            // range=10, expanded=false → 0/3=0.0
            assert!(v.abs() < dec!(0.001), "all contracting → 0.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_brep_reset() {
        let mut s = BarRangeExpansionPct::new("brep", 2).unwrap();
        for (h, l) in &[("110","90"),("115","85"),("120","80")] {
            s.update_bar(&bar(h, l)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
