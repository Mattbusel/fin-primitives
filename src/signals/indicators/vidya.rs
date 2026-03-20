//! VIDYA (Variable Index Dynamic Average) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Variable Index Dynamic Average — CMO-adaptive EMA.
///
/// `VIDYA = close × k × |CMO| + prev_VIDYA × (1 - k × |CMO|)`
///
/// where `k = 2/(period+1)` and `|CMO|` is the absolute value of the
/// Chande Momentum Oscillator normalized to [0,1].
///
/// High momentum → faster adaptation; low momentum → slower (like SMA).
///
/// Returns [`SignalValue::Unavailable`] until `cmo_period + 1` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vidya;
/// use fin_primitives::signals::Signal;
///
/// let mut v = Vidya::new("vidya14", 14).unwrap();
/// assert_eq!(v.period(), 14);
/// ```
pub struct Vidya {
    name: String,
    period: usize,
    k: Decimal,
    prev_close: Option<Decimal>,
    changes: VecDeque<Decimal>,
    vidya: Option<Decimal>,
}

impl Vidya {
    /// Constructs a new `Vidya`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            k,
            prev_close: None,
            changes: VecDeque::with_capacity(period),
            vidya: None,
        })
    }
}

impl Signal for Vidya {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let Some(pc) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };
        let change = bar.close - pc;
        self.prev_close = Some(bar.close);

        self.changes.push_back(change);
        if self.changes.len() > self.period {
            self.changes.pop_front();
        }
        if self.changes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let up_sum: Decimal = self.changes.iter().filter(|&&d| d > Decimal::ZERO).sum();
        let down_sum: Decimal = self.changes.iter().filter(|&&d| d < Decimal::ZERO).map(|d| d.abs()).sum();
        let denom = up_sum + down_sum;
        let cmo_abs = if denom == Decimal::ZERO {
            Decimal::ZERO
        } else {
            ((up_sum - down_sum) / denom).abs()
        };

        let alpha = self.k * cmo_abs;
        let prev_vidya = self.vidya.unwrap_or(bar.close);
        let new_vidya = bar.close * alpha + prev_vidya * (Decimal::ONE - alpha);
        self.vidya = Some(new_vidya);
        Ok(SignalValue::Scalar(new_vidya))
    }

    fn is_ready(&self) -> bool {
        self.vidya.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.changes.clear();
        self.vidya = None;
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
    fn test_vidya_period_0_error() {
        assert!(Vidya::new("v", 0).is_err());
    }

    #[test]
    fn test_vidya_unavailable_before_period() {
        let mut v = Vidya::new("v3", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(v.update_bar(&bar("100")).unwrap().is_scalar());
    }

    #[test]
    fn test_vidya_flat_market_tracks_price() {
        let mut v = Vidya::new("v3", 3).unwrap();
        for _ in 0..10 { v.update_bar(&bar("100")).unwrap(); }
        // flat → cmo=0 → alpha=0 → vidya stays at seed
        match v.update_bar(&bar("100")).unwrap() {
            SignalValue::Scalar(val) => assert!(val >= dec!(99) && val <= dec!(101)),
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_vidya_reset() {
        let mut v = Vidya::new("v3", 3).unwrap();
        for _ in 0..10 { v.update_bar(&bar("100")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
