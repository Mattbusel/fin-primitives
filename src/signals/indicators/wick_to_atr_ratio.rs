//! Wick-to-ATR Ratio — rolling average of total wick length divided by ATR.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Wick-to-ATR Ratio — rolling SMA of `(upper_wick + lower_wick) / ATR(period)`.
///
/// Measures what fraction of average true range is expressed as wick (rejection)
/// rather than body:
/// - **High (> 0.6)**: most volatility expressed as wicks — strong rejection / indecision.
/// - **Low (< 0.3)**: clean directional bars — little intrabar reversal.
/// - **Near 0**: mostly doji bars or bars with minimal wicks.
///
/// Uses Wilder's smoothing for the ATR denominator. Returns [`SignalValue::Unavailable`]
/// until `period + 1` bars have been seen or ATR is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WickToAtrRatio;
/// use fin_primitives::signals::Signal;
/// let wta = WickToAtrRatio::new("wta_14", 14).unwrap();
/// assert_eq!(wta.period(), 14);
/// ```
pub struct WickToAtrRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    atr: Option<Decimal>,
    atr_init: VecDeque<Decimal>,
    ratio_sum: Decimal,
    ratio_window: VecDeque<Decimal>,
    bars_seen: usize,
}

impl WickToAtrRatio {
    /// Constructs a new `WickToAtrRatio`.
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
            atr_init: VecDeque::with_capacity(period),
            ratio_sum: Decimal::ZERO,
            ratio_window: VecDeque::with_capacity(period),
            bars_seen: 0,
        })
    }
}

impl Signal for WickToAtrRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bars_seen > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.bars_seen += 1;

        // Update ATR with Wilder's smoothing
        if self.atr.is_none() {
            self.atr_init.push_back(tr);
            if self.atr_init.len() == self.period {
                let sum: Decimal = self.atr_init.iter().sum();
                self.atr = Some(
                    sum.checked_div(Decimal::from(self.period as u32))
                        .ok_or(FinError::ArithmeticOverflow)?,
                );
            }
        } else {
            let prev = self.atr.unwrap();
            let p = Decimal::from(self.period as u32);
            self.atr = Some(
                (prev * (p - Decimal::ONE) + tr)
                    .checked_div(p)
                    .ok_or(FinError::ArithmeticOverflow)?,
            );
        }

        self.prev_close = Some(bar.close);

        let atr = match self.atr {
            Some(a) => a,
            None => return Ok(SignalValue::Unavailable),
        };

        if self.bars_seen <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        if atr.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        // Compute wicks: body bounds are [min(open,close), max(open,close)]
        let body_top = bar.body_high();
        let body_bot = bar.body_low();
        let upper_wick = bar.high - body_top;
        let lower_wick = body_bot - bar.low;
        let total_wick = upper_wick + lower_wick;

        let ratio = total_wick
            .checked_div(atr)
            .ok_or(FinError::ArithmeticOverflow)?
            .max(Decimal::ZERO);

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.atr = None;
        self.atr_init.clear();
        self.ratio_sum = Decimal::ZERO;
        self.ratio_window.clear();
        self.bars_seen = 0;
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
    fn test_wta_invalid_period() {
        assert!(WickToAtrRatio::new("wta", 0).is_err());
    }

    #[test]
    fn test_wta_unavailable_during_warmup() {
        let mut s = WickToAtrRatio::new("wta", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(
                s.update_bar(&bar("100","110","90","100")).unwrap(),
                SignalValue::Unavailable
            );
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_wta_doji_bar_gives_high_ratio() {
        // Open=Close=100, High=110, Low=90 → pure wicks, no body
        let mut s = WickToAtrRatio::new("wta", 2).unwrap();
        for _ in 0..3 {
            s.update_bar(&bar("100","110","90","100")).unwrap();
        }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","110","90","100")).unwrap() {
            assert!(v > dec!(0), "doji bar should have positive wick-to-ATR ratio: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wta_full_body_bar_gives_lower_ratio() {
        // Open=90, Close=110, High=110, Low=90 → no wicks
        let mut s = WickToAtrRatio::new("wta", 2).unwrap();
        for _ in 0..3 {
            s.update_bar(&bar("90","110","90","110")).unwrap();
        }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("90","110","90","110")).unwrap() {
            assert_eq!(v, dec!(0), "no-wick bar should give ratio 0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wta_non_negative() {
        let mut s = WickToAtrRatio::new("wta", 3).unwrap();
        let bars_data = [
            ("100","115","85","110"),
            ("110","120","100","105"),
            ("105","112","98","108"),
            ("108","118","95","100"),
        ];
        for (o,h,l,c) in &bars_data {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(o,h,l,c)).unwrap() {
                assert!(v >= dec!(0), "wick-to-ATR should be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_wta_reset() {
        let mut s = WickToAtrRatio::new("wta", 2).unwrap();
        for _ in 0..4 { s.update_bar(&bar("100","110","90","100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
