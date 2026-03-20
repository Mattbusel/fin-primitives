//! Three Bar Pattern indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Three Bar Pattern — detects Three White Soldiers (bullish) and Three Black Crows
/// (bearish) candlestick patterns.
///
/// **Three White Soldiers** (output `+1`):
/// - Three consecutive bullish bars (`close > open`)
/// - Each bar closes higher than the previous
/// - Each bar opens within the prior bar's body
///
/// **Three Black Crows** (output `-1`):
/// - Three consecutive bearish bars (`close < open`)
/// - Each bar closes lower than the previous
/// - Each bar opens within the prior bar's body
///
/// Outputs `0` if no pattern is detected.
///
/// Returns [`SignalValue::Unavailable`] until 3 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ThreeBarPattern;
/// use fin_primitives::signals::Signal;
///
/// let tbp = ThreeBarPattern::new("tbp").unwrap();
/// assert_eq!(tbp.period(), 3);
/// ```
pub struct ThreeBarPattern {
    name: String,
    bars: VecDeque<(Decimal, Decimal)>, // (open, close) pairs
}

impl ThreeBarPattern {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), bars: VecDeque::with_capacity(3) })
    }
}

impl Signal for ThreeBarPattern {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 3 }
    fn is_ready(&self) -> bool { self.bars.len() >= 3 }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars.push_back((bar.open, bar.close));
        if self.bars.len() > 3 { self.bars.pop_front(); }
        if self.bars.len() < 3 { return Ok(SignalValue::Unavailable); }

        let (o1, c1) = self.bars[0];
        let (o2, c2) = self.bars[1];
        let (o3, c3) = self.bars[2];

        let body1_lo = o1.min(c1);
        let body1_hi = o1.max(c1);
        let body2_lo = o2.min(c2);
        let body2_hi = o2.max(c2);

        // Three White Soldiers
        let three_white = c1 > o1 && c2 > o2 && c3 > o3  // all bullish
            && c2 > c1 && c3 > c2                          // each closes higher
            && o2 >= body1_lo && o2 <= body1_hi            // bar2 opens in bar1's body
            && o3 >= body2_lo && o3 <= body2_hi;           // bar3 opens in bar2's body

        // Three Black Crows
        let three_black = c1 < o1 && c2 < o2 && c3 < o3  // all bearish
            && c2 < c1 && c3 < c2                          // each closes lower
            && o2 >= body1_lo && o2 <= body1_hi
            && o3 >= body2_lo && o3 <= body2_hi;

        let signal = if three_white { Decimal::ONE }
            else if three_black { Decimal::NEGATIVE_ONE }
            else { Decimal::ZERO };
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) { self.bars.clear(); }
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
        let hp = Price::new(cp.value().max(op.value()).to_string().parse().unwrap()).unwrap();
        let lp = Price::new(cp.value().min(op.value()).to_string().parse().unwrap()).unwrap();
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
    fn test_tbp_unavailable() {
        let mut tbp = ThreeBarPattern::new("t").unwrap();
        for _ in 0..2 {
            assert_eq!(tbp.update_bar(&bar("100","105")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_tbp_three_white_soldiers() {
        let mut tbp = ThreeBarPattern::new("t").unwrap();
        // Bar1: 100→108, Bar2: 105→113 (opens in bar1 body 100-108), Bar3: 110→118
        tbp.update_bar(&bar("100","108")).unwrap();
        tbp.update_bar(&bar("105","113")).unwrap();
        let r = tbp.update_bar(&bar("110","118")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_tbp_three_black_crows() {
        let mut tbp = ThreeBarPattern::new("t").unwrap();
        // Bar1: 108→100, Bar2: 103→95, Bar3: 98→90
        tbp.update_bar(&bar("108","100")).unwrap();
        tbp.update_bar(&bar("103","95")).unwrap();
        let r = tbp.update_bar(&bar("98","90")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_tbp_no_pattern() {
        let mut tbp = ThreeBarPattern::new("t").unwrap();
        tbp.update_bar(&bar("100","105")).unwrap();
        tbp.update_bar(&bar("103","108")).unwrap();
        let r = tbp.update_bar(&bar("106","104")).unwrap(); // last bar bearish → no three whites
        assert_eq!(r, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_tbp_reset() {
        let mut tbp = ThreeBarPattern::new("t").unwrap();
        for _ in 0..3 { tbp.update_bar(&bar("100","105")).unwrap(); }
        tbp.reset();
        assert!(!tbp.is_ready());
    }
}
