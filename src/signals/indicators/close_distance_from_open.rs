//! Close Distance from Open indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Distance from Open — rolling average of `(close - open) / ATR`.
///
/// For each bar:
/// ```text
/// single_bar_score = (close - open) / ATR(period)
/// output           = SMA(single_bar_score, period)
/// ```
///
/// - **Positive**: average bar closes above its open relative to volatility — bullish.
/// - **Negative**: average bar closes below its open — bearish.
/// - **Near zero**: balanced or indecisive.
/// - ATR uses simple moving average of TR (not Wilder smoothing).
/// - Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
///   (ATR needs `period` TRs, which requires `period + 1` bars for complete prev_close).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseDistanceFromOpen;
/// use fin_primitives::signals::Signal;
///
/// let cdfo = CloseDistanceFromOpen::new("cdfo", 10).unwrap();
/// assert_eq!(cdfo.period(), 10);
/// ```
pub struct CloseDistanceFromOpen {
    name: String,
    period: usize,
    trs: VecDeque<Decimal>,
    tr_sum: Decimal,
    scores: VecDeque<Decimal>,
    score_sum: Decimal,
    prev_close: Option<Decimal>,
}

impl CloseDistanceFromOpen {
    /// Constructs a new `CloseDistanceFromOpen`.
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
            trs: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
            scores: VecDeque::with_capacity(period),
            score_sum: Decimal::ZERO,
            prev_close: None,
        })
    }
}

impl Signal for CloseDistanceFromOpen {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.scores.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.tr_sum += tr;
        self.trs.push_back(tr);
        if self.trs.len() > self.period {
            let removed = self.trs.pop_front().unwrap();
            self.tr_sum -= removed;
        }

        if self.trs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.tr_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let score = if atr.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open)
                .checked_div(atr)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.score_sum += score;
        self.scores.push_back(score);
        if self.scores.len() > self.period {
            let removed = self.scores.pop_front().unwrap();
            self.score_sum -= removed;
        }

        if self.scores.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.score_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(avg))
    }

    fn reset(&mut self) {
        self.trs.clear();
        self.tr_sum = Decimal::ZERO;
        self.scores.clear();
        self.score_sum = Decimal::ZERO;
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
    fn test_cdfo_invalid_period() {
        assert!(CloseDistanceFromOpen::new("cdfo", 0).is_err());
    }

    #[test]
    fn test_cdfo_unavailable_during_warmup() {
        let mut cdfo = CloseDistanceFromOpen::new("cdfo", 3).unwrap();
        for _ in 0..5 {
            // warmup needs period (3 TRs) + period (3 scores) = ~6 bars total
            let r = cdfo.update_bar(&bar("100", "110", "90", "105")).unwrap();
            if !cdfo.is_ready() {
                assert_eq!(r, SignalValue::Unavailable);
            }
        }
    }

    #[test]
    fn test_cdfo_bullish_bars_positive() {
        // All bars close above open → positive avg score
        let mut cdfo = CloseDistanceFromOpen::new("cdfo", 2).unwrap();
        let bars = [
            ("95","110","90","108"),("96","112","92","110"),("97","113","93","111"),
            ("98","114","94","112"),("99","115","95","113"),
        ];
        let mut last = SignalValue::Unavailable;
        for &(o, h, l, c) in &bars {
            last = cdfo.update_bar(&bar(o, h, l, c)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "bullish bars → positive cdfo: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cdfo_reset() {
        let mut cdfo = CloseDistanceFromOpen::new("cdfo", 2).unwrap();
        for (o, h, l, c) in &[("95","110","90","108"),("96","112","92","110"),
                               ("97","113","93","111"),("98","114","94","112")] {
            cdfo.update_bar(&bar(o, h, l, c)).unwrap();
        }
        assert!(cdfo.is_ready());
        cdfo.reset();
        assert!(!cdfo.is_ready());
    }
}
