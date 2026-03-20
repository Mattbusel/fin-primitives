//! Price Velocity Ratio — intrabar body size relative to ATR.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Velocity Ratio — `(close - open) / ATR(period)`.
///
/// Measures the current bar's intrabar move in units of average true range:
/// - **+1**: bullish bar equal to one ATR in size.
/// - **−1**: bearish bar equal to one ATR.
/// - **Near 0**: small body relative to recent volatility.
///
/// Uses Wilder's ATR smoothing. Returns [`SignalValue::Unavailable`] until `period + 1`
/// bars have been seen, or when ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceVelocityRatio;
/// use fin_primitives::signals::Signal;
/// let pvr = PriceVelocityRatio::new("pvr_14", 14).unwrap();
/// assert_eq!(pvr.period(), 14);
/// ```
pub struct PriceVelocityRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    atr: Option<Decimal>,
    bars_seen: usize,
    tr_sum: Decimal,
    initial_trs: VecDeque<Decimal>,
}

impl PriceVelocityRatio {
    /// Constructs a new `PriceVelocityRatio`.
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
            bars_seen: 0,
            tr_sum: Decimal::ZERO,
            initial_trs: VecDeque::with_capacity(period),
        })
    }

    fn true_range(bar: &BarInput, prev_close: Option<Decimal>) -> Decimal {
        let hl = bar.range();
        if let Some(pc) = prev_close {
            let hc = (bar.high - pc).abs();
            let lc = (bar.low - pc).abs();
            hl.max(hc).max(lc)
        } else {
            hl
        }
    }
}

impl Signal for PriceVelocityRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bars_seen > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = Self::true_range(bar, self.prev_close);
        self.bars_seen += 1;

        let atr = if self.atr.is_none() {
            self.initial_trs.push_back(tr);
            if self.initial_trs.len() == self.period {
                let seed: Decimal = self.initial_trs.iter().sum();
                let a = seed
                    .checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.atr = Some(a);
            }
            None
        } else {
            let prev_atr = self.atr.unwrap();
            let period_d = Decimal::from(self.period as u32);
            let new_atr = (prev_atr * (period_d - Decimal::ONE) + tr)
                .checked_div(period_d)
                .ok_or(FinError::ArithmeticOverflow)?;
            self.atr = Some(new_atr);
            Some(new_atr)
        };

        self.prev_close = Some(bar.close);

        let atr_val = match atr {
            Some(a) => a,
            None => return Ok(SignalValue::Unavailable),
        };

        if atr_val.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let body = bar.net_move();
        let ratio = body
            .checked_div(atr_val)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.atr = None;
        self.bars_seen = 0;
        self.tr_sum = Decimal::ZERO;
        self.initial_trs.clear();
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
    fn test_pvr_invalid_period() {
        assert!(PriceVelocityRatio::new("pvr", 0).is_err());
    }

    #[test]
    fn test_pvr_unavailable_before_warm_up() {
        let mut s = PriceVelocityRatio::new("pvr", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100","110","90","105")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_pvr_doji_gives_zero() {
        let mut s = PriceVelocityRatio::new("pvr", 2).unwrap();
        s.update_bar(&bar("100","110","90","105")).unwrap();
        s.update_bar(&bar("105","115","95","110")).unwrap();
        // After warm-up, a doji bar
        let v = s.update_bar(&bar("108","118","98","108")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert_eq!(r, dec!(0), "doji should give zero velocity ratio");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pvr_bullish_positive() {
        let mut s = PriceVelocityRatio::new("pvr", 2).unwrap();
        s.update_bar(&bar("100","110","90","100")).unwrap();
        s.update_bar(&bar("100","110","90","100")).unwrap();
        let v = s.update_bar(&bar("100","110","90","108")).unwrap(); // body=8, ATR~10
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "bullish bar should give positive PVR: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pvr_reset() {
        let mut s = PriceVelocityRatio::new("pvr", 2).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100","110","90","105")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
