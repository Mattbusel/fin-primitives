//! Directional Movement Index (DMI) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Directional Movement Index — exposes `+DI` and `-DI` as a `SignalValue::Pair`.
///
/// Uses Wilder smoothing (`α = 1/period`) identical to the ADX calculation.
/// Returns `SignalValue::Scalar(di_plus - di_minus)` once ready; use the
/// `di_plus()` and `di_minus()` accessors for individual values.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Dmi;
/// use fin_primitives::signals::Signal;
///
/// let mut dmi = Dmi::new("dmi14", 14).unwrap();
/// assert_eq!(dmi.period(), 14);
/// ```
pub struct Dmi {
    name: String,
    period: usize,
    multiplier: Decimal,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    prev_close: Option<Decimal>,
    sdm_plus: Decimal,
    sdm_minus: Decimal,
    str_: Decimal,
    bar_count: usize,
    last_di_plus: Option<Decimal>,
    last_di_minus: Option<Decimal>,
}

impl Dmi {
    /// Constructs a new `Dmi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let multiplier = Decimal::ONE / Decimal::from(period as u32);
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            prev_high: None,
            prev_low: None,
            prev_close: None,
            sdm_plus: Decimal::ZERO,
            sdm_minus: Decimal::ZERO,
            str_: Decimal::ZERO,
            bar_count: 0,
            last_di_plus: None,
            last_di_minus: None,
        })
    }

    /// Returns the last computed `+DI` value, or `None` if not ready.
    pub fn di_plus(&self) -> Option<Decimal> {
        self.last_di_plus
    }

    /// Returns the last computed `-DI` value, or `None` if not ready.
    pub fn di_minus(&self) -> Option<Decimal> {
        self.last_di_minus
    }
}

impl Signal for Dmi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let h = bar.high;
        let l = bar.low;
        let c = bar.close;

        let Some(ph) = self.prev_high else {
            self.prev_high = Some(h);
            self.prev_low = Some(l);
            self.prev_close = Some(c);
            return Ok(SignalValue::Unavailable);
        };
        let pl = self.prev_low.unwrap();
        let pc = self.prev_close.unwrap();

        let up_move = h - ph;
        let down_move = pl - l;
        let dm_plus = if up_move > down_move && up_move > Decimal::ZERO { up_move } else { Decimal::ZERO };
        let dm_minus = if down_move > up_move && down_move > Decimal::ZERO { down_move } else { Decimal::ZERO };
        let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());

        self.prev_high = Some(h);
        self.prev_low = Some(l);
        self.prev_close = Some(c);
        self.bar_count += 1;

        let one_minus_k = Decimal::ONE - self.multiplier;
        if self.bar_count <= self.period {
            self.sdm_plus += dm_plus;
            self.sdm_minus += dm_minus;
            self.str_ += tr;
            if self.bar_count < self.period {
                return Ok(SignalValue::Unavailable);
            }
        } else {
            self.sdm_plus = self.sdm_plus * one_minus_k + dm_plus * self.multiplier;
            self.sdm_minus = self.sdm_minus * one_minus_k + dm_minus * self.multiplier;
            self.str_ = self.str_ * one_minus_k + tr * self.multiplier;
        }

        if self.str_.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let di_plus = self.sdm_plus / self.str_ * Decimal::ONE_HUNDRED;
        let di_minus = self.sdm_minus / self.str_ * Decimal::ONE_HUNDRED;
        self.last_di_plus = Some(di_plus);
        self.last_di_minus = Some(di_minus);
        Ok(SignalValue::Scalar(di_plus - di_minus))
    }

    fn is_ready(&self) -> bool {
        self.last_di_plus.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
        self.prev_close = None;
        self.sdm_plus = Decimal::ZERO;
        self.sdm_minus = Decimal::ZERO;
        self.str_ = Decimal::ZERO;
        self.bar_count = 0;
        self.last_di_plus = None;
        self.last_di_minus = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_dmi_period_0_error() {
        assert!(Dmi::new("d", 0).is_err());
    }

    #[test]
    fn test_dmi_unavailable_before_period() {
        let mut dmi = Dmi::new("d3", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(dmi.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(dmi.update_bar(&bar("110","90","100")).unwrap().is_scalar());
    }

    #[test]
    fn test_dmi_uptrend_positive() {
        let mut dmi = Dmi::new("d5", 5).unwrap();
        for i in 0..20u32 {
            let h = format!("{}", 110 + i);
            let l = format!("{}", 90 + i);
            let c = format!("{}", 100 + i);
            dmi.update_bar(&bar(&h, &l, &c)).unwrap();
        }
        if let Some(dip) = dmi.di_plus() {
            if let Some(dim) = dmi.di_minus() {
                assert!(dip > dim, "+DI should exceed -DI in uptrend");
            }
        }
    }

    #[test]
    fn test_dmi_reset() {
        let mut dmi = Dmi::new("d3", 3).unwrap();
        for _ in 0..10 { dmi.update_bar(&bar("110","90","100")).unwrap(); }
        assert!(dmi.is_ready());
        dmi.reset();
        assert!(!dmi.is_ready());
        assert!(dmi.di_plus().is_none());
        assert!(dmi.di_minus().is_none());
    }

    #[test]
    fn test_dmi_flat_market_near_zero() {
        let mut dmi = Dmi::new("d3", 3).unwrap();
        for _ in 0..10 { dmi.update_bar(&bar("100","100","100")).unwrap(); }
        // flat → str=0 → Unavailable or near-zero
        assert!(!dmi.is_ready() || dmi.di_plus().unwrap_or(dec!(0)) < dec!(1));
    }
}
