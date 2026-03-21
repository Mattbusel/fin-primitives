//! Body Width Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Width Ratio — rolling average of `body / range` (body-to-range), which
/// measures what fraction of the typical bar's movement is directional (body)
/// versus total range.
///
/// ```text
/// body_ratio[i] = |close[i] - open[i]| / (high[i] - low[i])
/// output[t]     = mean(body_ratio[t-period+1 .. t])
/// ```
///
/// - **Near 1.0 (100%)**: bars are closing very close to one extreme — strong directional moves.
/// - **Near 0.0**: bars are mostly wick — indecision, reversal candles, or doji patterns.
/// - **0.5**: body accounts for half the range on average.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
/// Bars with zero range (doji) contribute a ratio of 0.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyWidthRatio;
/// use fin_primitives::signals::Signal;
/// let bwr = BodyWidthRatio::new("bwr_14", 14).unwrap();
/// assert_eq!(bwr.period(), 14);
/// ```
pub struct BodyWidthRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyWidthRatio {
    /// Constructs a new `BodyWidthRatio`.
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
            ratios: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for BodyWidthRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ratios.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            bar.body_size()
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.sum += ratio;
        self.ratios.push_back(ratio);
        if self.ratios.len() > self.period {
            let removed = self.ratios.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.ratios.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.ratios.clear();
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
    fn test_bwr_invalid_period() {
        assert!(BodyWidthRatio::new("bwr", 0).is_err());
    }

    #[test]
    fn test_bwr_unavailable_during_warmup() {
        let mut bwr = BodyWidthRatio::new("bwr", 3).unwrap();
        assert_eq!(bwr.update_bar(&bar("100","110","90","105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(bwr.update_bar(&bar("100","110","90","105")).unwrap(), SignalValue::Unavailable);
        assert!(!bwr.is_ready());
    }

    #[test]
    fn test_bwr_full_body_near_one() {
        // bar open at low, close at high → body = range → ratio = 1
        let mut bwr = BodyWidthRatio::new("bwr", 3).unwrap();
        for _ in 0..4 {
            bwr.update_bar(&bar("100","110","100","110")).unwrap(); // body=10, range=10 → 1.0
        }
        if let SignalValue::Scalar(v) = bwr.update_bar(&bar("100","110","100","110")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bwr_doji_near_zero() {
        // All doji → ratio = 0
        let mut bwr = BodyWidthRatio::new("bwr", 3).unwrap();
        for _ in 0..4 {
            bwr.update_bar(&bar("100","110","90","100")).unwrap(); // open=close=100, range=20 → ratio=0
        }
        if let SignalValue::Scalar(v) = bwr.update_bar(&bar("100","110","90","100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bwr_reset() {
        let mut bwr = BodyWidthRatio::new("bwr", 3).unwrap();
        for _ in 0..3 { bwr.update_bar(&bar("100","110","90","105")).unwrap(); }
        assert!(bwr.is_ready());
        bwr.reset();
        assert!(!bwr.is_ready());
    }
}
