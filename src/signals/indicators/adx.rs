//! Average Directional Index (ADX) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Average Directional Index over `period` bars.
///
/// ADX measures trend strength (not direction) on a scale of 0–100:
/// - ADX < 20: weak or no trend
/// - ADX 20–40: moderate trend
/// - ADX > 40: strong trend
///
/// Uses Wilder's smoothing (`α = 1/period`) on the Directional Movement components.
/// Requires `2 * period` bars to fully warm up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Adx;
/// use fin_primitives::signals::Signal;
///
/// let adx = Adx::new("adx14", 14).unwrap();
/// assert_eq!(adx.period(), 14);
/// ```
pub struct Adx {
    name: String,
    period: usize,
    multiplier: Decimal,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    prev_close: Option<Decimal>,
    // Smoothed +DM, -DM, TR
    sdm_plus: Decimal,
    sdm_minus: Decimal,
    str_: Decimal,
    // Smoothed DX for ADX
    adx: Option<Decimal>,
    bar_count: usize,
    dx_sum: Decimal,
}

impl Adx {
    /// Constructs a new `Adx` with the given period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
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
            adx: None,
            bar_count: 0,
            dx_sum: Decimal::ZERO,
        })
    }
}

impl Signal for Adx {
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

        // Directional movement
        let up_move = h - ph;
        let down_move = pl - l;
        let dm_plus = if up_move > down_move && up_move > Decimal::ZERO { up_move } else { Decimal::ZERO };
        let dm_minus = if down_move > up_move && down_move > Decimal::ZERO { down_move } else { Decimal::ZERO };

        // True range
        let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());

        self.bar_count += 1;

        if self.bar_count <= self.period {
            // Accumulate first-period sums
            self.sdm_plus += dm_plus;
            self.sdm_minus += dm_minus;
            self.str_ += tr;

            self.prev_high = Some(h);
            self.prev_low = Some(l);
            self.prev_close = Some(c);

            if self.bar_count < self.period {
                return Ok(SignalValue::Unavailable);
            }
            // First period complete: compute first DX
        } else {
            // Wilder smoothing
            let one_minus_k = Decimal::ONE - self.multiplier;
            self.sdm_plus = self.sdm_plus * one_minus_k + dm_plus * self.multiplier;
            self.sdm_minus = self.sdm_minus * one_minus_k + dm_minus * self.multiplier;
            self.str_ = self.str_ * one_minus_k + tr * self.multiplier;

            self.prev_high = Some(h);
            self.prev_low = Some(l);
            self.prev_close = Some(c);
        }

        // DI+ and DI-
        if self.str_.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let di_plus = self.sdm_plus / self.str_ * Decimal::ONE_HUNDRED;
        let di_minus = self.sdm_minus / self.str_ * Decimal::ONE_HUNDRED;
        let di_sum = di_plus + di_minus;
        let dx = if di_sum.is_zero() {
            Decimal::ZERO
        } else {
            (di_plus - di_minus).abs() / di_sum * Decimal::ONE_HUNDRED
        };

        // ADX: Wilder smoothing of DX, seeded by simple average over second period
        let adx_period = self.bar_count - self.period; // how many DX values seen
        if adx_period < self.period {
            self.dx_sum += dx;
            if adx_period + 1 == self.period {
                // Seed the ADX
                self.adx = Some(self.dx_sum / Decimal::from(self.period as u32));
            }
            return Ok(SignalValue::Unavailable);
        }

        let prev_adx = self.adx.unwrap_or(dx);
        let one_minus_k = Decimal::ONE - self.multiplier;
        let new_adx = prev_adx * one_minus_k + dx * self.multiplier;
        self.adx = Some(new_adx);

        Ok(SignalValue::Scalar(new_adx))
    }

    fn is_ready(&self) -> bool {
        self.adx.is_some() && self.bar_count >= 2 * self.period
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
        self.adx = None;
        self.bar_count = 0;
        self.dx_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_adx_period_0_fails() {
        assert!(Adx::new("adx", 0).is_err());
    }

    #[test]
    fn test_adx_unavailable_before_warmup() {
        let mut adx = Adx::new("adx3", 3).unwrap();
        // needs 2*3 = 6 bars minimum (1 prev + period sums + period DX)
        for _ in 0..5 {
            assert_eq!(adx.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!adx.is_ready());
    }

    #[test]
    fn test_adx_reset() {
        let mut adx = Adx::new("adx3", 3).unwrap();
        for _ in 0..20 {
            adx.update_bar(&bar("110", "90", "100")).unwrap();
        }
        adx.reset();
        assert!(!adx.is_ready());
        assert_eq!(adx.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_adx_strong_trend_above_25() {
        // Feed a consistent uptrend: each bar's high and low increase monotonically
        let mut adx = Adx::new("adx5", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 1..=30u32 {
            let h = 100.0 + i as f64;
            let l = 98.0 + i as f64;
            let c = 99.0 + i as f64;
            let b = bar(&format!("{h:.1}"), &format!("{l:.1}"), &format!("{c:.1}"));
            last = adx.update_bar(&b).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v >= rust_decimal_macros::dec!(20), "ADX should be > 20 in strong trend, got {v}");
        } else {
            panic!("expected Scalar after 30 bars");
        }
    }
}
