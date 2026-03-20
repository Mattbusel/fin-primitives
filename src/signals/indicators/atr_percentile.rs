//! ATR Percentile indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ATR Percentile — percentile rank of the current ATR within the last `period` ATR values.
///
/// ```text
/// ATR_i = SMA(TR, 1) = TR_i  (single-bar TR, no smoothing)
/// percentile = count(ATR_j < current_ATR) / (period - 1) * 100
/// ```
///
/// - **Near 100**: current volatility is high relative to recent history (volatility expansion).
/// - **Near 0**: current volatility is low (volatility compression).
/// - Uses single-bar TR (not Wilder-smoothed) for simplicity.
/// - Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AtrPercentile;
/// use fin_primitives::signals::Signal;
///
/// let ap = AtrPercentile::new("atr_pct", 14).unwrap();
/// assert_eq!(ap.period(), 14);
/// ```
pub struct AtrPercentile {
    name: String,
    period: usize,
    trs: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl AtrPercentile {
    /// Constructs a new `AtrPercentile`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            trs: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for AtrPercentile {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.trs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        if self.trs.len() > self.period {
            self.trs.pop_front();
        }

        if self.trs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let current_tr = tr;
        let count_below = self.trs.iter()
            .filter(|&&t| t < current_tr)
            .count();

        // percentile rank: count below / (n-1) * 100, clamped to [0, 100]
        #[allow(clippy::cast_possible_truncation)]
        let percentile = Decimal::from(count_below as u32)
            / Decimal::from((self.period - 1) as u32)
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(percentile))
    }

    fn reset(&mut self) {
        self.trs.clear();
        self.prev_close = None;
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
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_atr_pct_invalid_period() {
        assert!(AtrPercentile::new("ap", 0).is_err());
        assert!(AtrPercentile::new("ap", 1).is_err());
    }

    #[test]
    fn test_atr_pct_unavailable_during_warmup() {
        let mut ap = AtrPercentile::new("ap", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(ap.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ap.is_ready());
    }

    #[test]
    fn test_atr_pct_highest_tr_is_100() {
        // Feed increasing TR bars, then a very wide bar last
        let mut ap = AtrPercentile::new("ap", 3).unwrap();
        ap.update_bar(&bar("102", "98", "100")).unwrap(); // TR=4
        ap.update_bar(&bar("103", "97", "100")).unwrap(); // TR=6
        // Last bar: very wide TR=20 — should be percentile=100
        let result = ap.update_bar(&bar("120", "80", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_atr_pct_lowest_tr_is_0() {
        let mut ap = AtrPercentile::new("ap", 3).unwrap();
        ap.update_bar(&bar("120", "80", "100")).unwrap(); // TR=40
        ap.update_bar(&bar("115", "85", "100")).unwrap(); // TR=30
        // Last bar: very narrow TR=2 — should be percentile=0
        let result = ap.update_bar(&bar("101", "99", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_atr_pct_range_0_to_100() {
        let mut ap = AtrPercentile::new("ap", 5).unwrap();
        let bars = [("110","90","100"),("108","92","100"),("112","88","100"),
                    ("106","94","100"),("115","85","100")];
        for &(h, l, c) in &bars {
            if let SignalValue::Scalar(v) = ap.update_bar(&bar(h, l, c)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(100), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_atr_pct_reset() {
        let mut ap = AtrPercentile::new("ap", 3).unwrap();
        for &(h, l, c) in &[("110","90","100"),("108","92","100"),("112","88","100")] {
            ap.update_bar(&bar(h, l, c)).unwrap();
        }
        assert!(ap.is_ready());
        ap.reset();
        assert!(!ap.is_ready());
    }
}
