//! Dual ATR Ratio — fast ATR divided by slow ATR for volatility regime detection.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Dual ATR Ratio — `ATR(fast_period) / ATR(slow_period)`.
///
/// Compares short-term volatility to long-term volatility:
/// - **> 1.0**: short-term volatility elevated — potentially entering a volatile regime.
/// - **= 1.0**: consistent volatility across both timeframes.
/// - **< 1.0**: short-term volatility lower than long-term — compression / calm regime.
///
/// Both ATR values use Wilder's smoothing. Returns [`SignalValue::Unavailable`] until
/// `slow_period + 1` bars have been seen, or when slow ATR is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `fast_period == 0`, `slow_period == 0`,
/// or `fast_period >= slow_period`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DualATRRatio;
/// use fin_primitives::signals::Signal;
/// let dar = DualATRRatio::new("dar", 5, 20).unwrap();
/// assert_eq!(dar.period(), 20);
/// ```
pub struct DualATRRatio {
    name: String,
    fast_period: usize,
    slow_period: usize,
    prev_close: Option<Decimal>,
    fast_atr: Option<Decimal>,
    slow_atr: Option<Decimal>,
    fast_init: VecDeque<Decimal>,
    slow_init: VecDeque<Decimal>,
    bars_seen: usize,
}

impl DualATRRatio {
    /// Constructs a new `DualATRRatio`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
    ) -> Result<Self, FinError> {
        if fast_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        if slow_period == 0 || fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(slow_period));
        }
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            prev_close: None,
            fast_atr: None,
            slow_atr: None,
            fast_init: VecDeque::with_capacity(fast_period),
            slow_init: VecDeque::with_capacity(slow_period),
            bars_seen: 0,
        })
    }

    fn true_range(bar: &BarInput, prev_close: Option<Decimal>) -> Decimal {
        let hl = bar.range();
        if let Some(pc) = prev_close {
            hl.max((bar.high - pc).abs()).max((bar.low - pc).abs())
        } else {
            hl
        }
    }

    fn update_atr(
        atr: &mut Option<Decimal>,
        init: &mut VecDeque<Decimal>,
        tr: Decimal,
        period: usize,
    ) -> Result<(), FinError> {
        if atr.is_none() {
            init.push_back(tr);
            if init.len() == period {
                let sum: Decimal = init.iter().sum();
                *atr = Some(
                    sum.checked_div(Decimal::from(period as u32))
                        .ok_or(FinError::ArithmeticOverflow)?,
                );
            }
        } else {
            let prev = atr.unwrap();
            let period_d = Decimal::from(period as u32);
            *atr = Some(
                (prev * (period_d - Decimal::ONE) + tr)
                    .checked_div(period_d)
                    .ok_or(FinError::ArithmeticOverflow)?,
            );
        }
        Ok(())
    }
}

impl Signal for DualATRRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow_period }
    fn is_ready(&self) -> bool { self.bars_seen > self.slow_period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = Self::true_range(bar, self.prev_close);
        self.bars_seen += 1;

        Self::update_atr(&mut self.fast_atr, &mut self.fast_init, tr, self.fast_period)?;
        Self::update_atr(&mut self.slow_atr, &mut self.slow_init, tr, self.slow_period)?;

        self.prev_close = Some(bar.close);

        let (fast, slow) = match (self.fast_atr, self.slow_atr) {
            (Some(f), Some(s)) => (f, s),
            _ => return Ok(SignalValue::Unavailable),
        };

        if self.bars_seen <= self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        if slow.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = fast
            .checked_div(slow)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.fast_atr = None;
        self.slow_atr = None;
        self.fast_init.clear();
        self.slow_init.clear();
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
    fn test_dar_invalid_period() {
        assert!(DualATRRatio::new("dar", 0, 20).is_err());
        assert!(DualATRRatio::new("dar", 20, 5).is_err()); // fast >= slow
        assert!(DualATRRatio::new("dar", 5, 5).is_err()); // fast == slow
    }

    #[test]
    fn test_dar_unavailable_before_warm_up() {
        let mut s = DualATRRatio::new("dar", 2, 4).unwrap();
        for _ in 0..4 {
            assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_dar_constant_tr_gives_one() {
        let mut s = DualATRRatio::new("dar", 2, 4).unwrap();
        // Constant TR: both ATRs converge to same value → ratio = 1
        for _ in 0..5 { s.update_bar(&bar("110","90","100")).unwrap(); }
        let v = s.update_bar(&bar("110","90","100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(1)).abs() < dec!(0.001), "constant TR should give ratio~1: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_dar_non_negative() {
        let mut s = DualATRRatio::new("dar", 3, 10).unwrap();
        for _ in 0..12 {
            if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","100")).unwrap() {
                assert!(v >= dec!(0), "ratio must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_dar_reset() {
        let mut s = DualATRRatio::new("dar", 2, 4).unwrap();
        for _ in 0..6 { s.update_bar(&bar("110","90","100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
