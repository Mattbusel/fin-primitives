//! Average True Range Percent of Close indicator.
//!
//! Normalizes ATR by the closing price to produce a volatility measure that is
//! comparable across instruments with different price levels.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Average True Range as a percentage of close: `ATR(N) / close * 100`.
///
/// Also known as ATRP. Normalizes the raw ATR by the current close price,
/// making it useful for comparing volatility across instruments or across time
/// when price levels have changed significantly.
///
/// True Range for each bar: `max(high - low, |high - prev_close|, |low - prev_close|)`.
/// The Wilder-smoothed ATR is then divided by the current close.
///
/// Returns [`SignalValue::Unavailable`] until the warm-up period is complete or
/// when the close is zero. Period = `period` bars.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AvgTrueRangePct;
/// use fin_primitives::signals::Signal;
///
/// let atrp = AvgTrueRangePct::new("atrp14", 14).unwrap();
/// assert_eq!(atrp.period(), 14);
/// assert!(!atrp.is_ready());
/// ```
pub struct AvgTrueRangePct {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    atr: Option<Decimal>,
    seed_trs: VecDeque<Decimal>,
}

impl AvgTrueRangePct {
    /// Constructs a new `AvgTrueRangePct`.
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
            prev_close: None,
            atr: None,
            seed_trs: VecDeque::with_capacity(period),
        })
    }

    fn true_range(bar: &BarInput, prev_close: Option<Decimal>) -> Decimal {
        let hl = bar.high - bar.low;
        match prev_close {
            None => hl,
            Some(pc) => {
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        }
    }
}

impl Signal for AvgTrueRangePct {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.atr.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = Self::true_range(bar, self.prev_close);
        self.prev_close = Some(bar.close);

        if self.atr.is_none() {
            self.seed_trs.push_back(tr);
            if self.seed_trs.len() < self.period {
                return Ok(SignalValue::Unavailable);
            }
            #[allow(clippy::cast_possible_truncation)]
            let period_d = Decimal::from(self.period as u32);
            let seed_sum = self.seed_trs.iter().copied().sum::<Decimal>();
            let atr = seed_sum
                .checked_div(period_d)
                .ok_or(FinError::ArithmeticOverflow)?;
            self.atr = Some(atr);

            if bar.close.is_zero() {
                return Ok(SignalValue::Unavailable);
            }
            let pct = atr
                .checked_div(bar.close)
                .ok_or(FinError::ArithmeticOverflow)?
                * Decimal::ONE_HUNDRED;
            return Ok(SignalValue::Scalar(pct));
        }

        #[allow(clippy::cast_possible_truncation)]
        let period_d = Decimal::from(self.period as u32);
        let prev_atr = self.atr.unwrap_or(Decimal::ZERO);
        let atr = (prev_atr * (period_d - Decimal::ONE) + tr)
            .checked_div(period_d)
            .ok_or(FinError::ArithmeticOverflow)?;
        self.atr = Some(atr);

        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let pct = atr
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.atr = None;
        self.seed_trs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(open.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_atrpct_invalid_period() {
        assert!(AvgTrueRangePct::new("atrp", 0).is_err());
    }

    #[test]
    fn test_atrpct_unavailable_during_warmup() {
        let mut atrp = AvgTrueRangePct::new("atrp", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(
                atrp.update_bar(&bar("100", "110", "90", "105")).unwrap(),
                SignalValue::Unavailable
            );
        }
    }

    #[test]
    fn test_atrpct_ready_after_period() {
        let mut atrp = AvgTrueRangePct::new("atrp", 3).unwrap();
        for _ in 0..3 {
            atrp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        }
        assert!(atrp.is_ready());
    }

    #[test]
    fn test_atrpct_positive() {
        let mut atrp = AvgTrueRangePct::new("atrp", 3).unwrap();
        for _ in 0..3 {
            atrp.update_bar(&bar("100", "110", "90", "100")).unwrap();
        }
        let v = atrp.update_bar(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(pct) = v {
            assert!(pct > dec!(0), "ATR% should be positive: {pct}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_atrpct_flat_bar_zero() {
        let mut atrp = AvgTrueRangePct::new("atrp", 3).unwrap();
        for _ in 0..3 {
            atrp.update_bar(&bar("100", "100", "100", "100")).unwrap();
        }
        let v = atrp.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_atrpct_reset() {
        let mut atrp = AvgTrueRangePct::new("atrp", 3).unwrap();
        for _ in 0..3 {
            atrp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        }
        assert!(atrp.is_ready());
        atrp.reset();
        assert!(!atrp.is_ready());
    }
}
