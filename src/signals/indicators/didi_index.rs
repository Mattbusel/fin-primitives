//! Didi Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Didi Index (Índice Didi Aguiar).
///
/// A Brazilian momentum indicator invented by Odir "Didi" Aguiar. It uses three
/// simple moving averages (short, medium, long) to identify trend direction and
/// momentum quality. The output is the *needle spread*: the distance between the
/// short-SMA and the long-SMA, normalised by the medium-SMA.
///
/// Formula: `needle = (short_sma − long_sma) / medium_sma × 100`
///
/// - Positive values indicate bullish momentum (short SMA above long SMA).
/// - Negative values indicate bearish momentum.
/// - Zero crossings act as signal entries.
///
/// Default periods: short = 3, medium = 8, long = 20.
/// Custom periods may be supplied but must satisfy `short < medium < long`.
///
/// Returns `SignalValue::Unavailable` until `long_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DidiIndex;
/// use fin_primitives::signals::Signal;
/// let didi = DidiIndex::new("didi", 3, 8, 20).unwrap();
/// assert_eq!(didi.period(), 20);
/// ```
pub struct DidiIndex {
    name: String,
    short_period: usize,
    medium_period: usize,
    long_period: usize,
    short_win: VecDeque<Decimal>,
    medium_win: VecDeque<Decimal>,
    long_win: VecDeque<Decimal>,
}

impl DidiIndex {
    /// Constructs a new `DidiIndex` with explicit short, medium and long periods.
    ///
    /// # Errors
    /// - [`FinError::InvalidPeriod`] if any period is 0 or if `short >= medium` or `medium >= long`.
    pub fn new(
        name: impl Into<String>,
        short_period: usize,
        medium_period: usize,
        long_period: usize,
    ) -> Result<Self, FinError> {
        if short_period == 0 || medium_period == 0 || long_period == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        if short_period >= medium_period || medium_period >= long_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            name: name.into(),
            short_period,
            medium_period,
            long_period,
            short_win: VecDeque::with_capacity(short_period),
            medium_win: VecDeque::with_capacity(medium_period),
            long_win: VecDeque::with_capacity(long_period),
        })
    }

    fn sma(window: &VecDeque<Decimal>, period: usize) -> Result<Decimal, FinError> {
        let sum: Decimal = window.iter().copied().sum();
        sum.checked_div(Decimal::from(period as u32))
            .ok_or(FinError::ArithmeticOverflow)
    }
}

impl Signal for DidiIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.short_win.push_back(bar.close);
        if self.short_win.len() > self.short_period {
            self.short_win.pop_front();
        }
        self.medium_win.push_back(bar.close);
        if self.medium_win.len() > self.medium_period {
            self.medium_win.pop_front();
        }
        self.long_win.push_back(bar.close);
        if self.long_win.len() > self.long_period {
            self.long_win.pop_front();
        }

        if self.long_win.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let short_sma = Self::sma(&self.short_win, self.short_period)?;
        let medium_sma = Self::sma(&self.medium_win, self.medium_period)?;
        let long_sma = Self::sma(&self.long_win, self.long_period)?;

        if medium_sma.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let spread = short_sma
            .checked_sub(long_sma)
            .ok_or(FinError::ArithmeticOverflow)?;
        let needle = spread
            .checked_div(medium_sma)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(needle))
    }

    fn is_ready(&self) -> bool {
        self.long_win.len() >= self.long_period
    }

    fn period(&self) -> usize {
        self.long_period
    }

    fn reset(&mut self) {
        self.short_win.clear();
        self.medium_win.clear();
        self.long_win.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_invalid_period_fails() {
        assert!(DidiIndex::new("d", 0, 8, 20).is_err());
        assert!(DidiIndex::new("d", 8, 3, 20).is_err()); // short >= medium
        assert!(DidiIndex::new("d", 3, 20, 8).is_err()); // medium >= long
    }

    #[test]
    fn test_unavailable_before_long_period() {
        let mut d = DidiIndex::new("d", 3, 8, 20).unwrap();
        for _ in 0..19 {
            let v = d.update_bar(&bar("100")).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ready_after_long_period() {
        let mut d = DidiIndex::new("d", 3, 8, 20).unwrap();
        for _ in 0..20 {
            d.update_bar(&bar("100")).unwrap();
        }
        assert!(d.is_ready());
    }

    #[test]
    fn test_constant_price_needle_zero() {
        // When all prices are equal, short == medium == long → needle = 0
        let mut d = DidiIndex::new("d", 3, 8, 20).unwrap();
        for _ in 0..20 {
            d.update_bar(&bar("100")).unwrap();
        }
        let v = d.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bullish_needle_positive() {
        // Rising prices: short SMA > long SMA → positive needle
        let mut d = DidiIndex::new("d", 3, 8, 20).unwrap();
        for i in 0..21 {
            d.update_bar(&bar(&(i * 10).to_string())).unwrap();
        }
        let v = d.update_bar(&bar("300")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "expected positive needle for rising prices, got {}", s);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset_clears_state() {
        let mut d = DidiIndex::new("d", 3, 8, 20).unwrap();
        for _ in 0..20 {
            d.update_bar(&bar("100")).unwrap();
        }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
    }

    #[test]
    fn test_period_returns_long() {
        let d = DidiIndex::new("d", 3, 8, 20).unwrap();
        assert_eq!(d.period(), 20);
    }
}
