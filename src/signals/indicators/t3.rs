//! Tim Tillson's T3 moving average.
//!
//! A triple-smoothed EMA that is smoother and has less lag than a standard triple EMA.
//! Uses a volume factor `v_factor` (default 0.7) to control the smoothness/lag trade-off.
//! Lower `v_factor` increases smoothness at the cost of more lag; higher reduces lag.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// T3 Moving Average (Tim Tillson).
///
/// Computes six nested EMAs and combines them:
/// `T3 = c1*EMA6 + c2*EMA5 + c3*EMA4 + c4*EMA3`
/// where `c1..c4` depend on the volume factor `v`.
///
/// Returns [`crate::signals::SignalValue::Unavailable`] until `6*(period-1)+1` bars are seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::T3;
/// use fin_primitives::signals::Signal;
/// let t = T3::new("t3_5", 5, None).unwrap();
/// assert_eq!(t.period(), 5);
/// assert!(!t.is_ready());
/// ```
pub struct T3 {
    name: String,
    period: usize,
    k: Decimal,
    c1: Decimal,
    c2: Decimal,
    c3: Decimal,
    c4: Decimal,
    emas: [Option<Decimal>; 6],
    bar_count: usize,
}

impl T3 {
    /// Constructs a new `T3` indicator.
    ///
    /// `v_factor` defaults to 0.7 if `None`. Must be in (0.0, 1.0].
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `v_factor` is outside (0.0, 1.0].
    pub fn new(name: impl Into<String>, period: usize, v_factor: Option<f64>) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let v = v_factor.unwrap_or(0.7);
        if v <= 0.0 || v > 1.0 {
            return Err(FinError::InvalidInput(format!("v_factor must be in (0,1], got {}", v)));
        }
        let v_dec = Decimal::try_from(v).map_err(|_| FinError::ArithmeticOverflow)?;
        let v2 = v_dec * v_dec;
        let v3 = v2 * v_dec;
        let c1 = -v3;
        let c2 = Decimal::from(3) * v2 + Decimal::from(3) * v3;
        let c3 = -Decimal::from(6) * v2 - Decimal::from(3) * v_dec - Decimal::from(3) * v3;
        let c4 = Decimal::ONE + Decimal::from(3) * v_dec + v3 + Decimal::from(3) * v2;
        let k = Decimal::TWO / Decimal::from(period as u32 + 1);
        Ok(Self {
            name: name.into(),
            period,
            k,
            c1,
            c2,
            c3,
            c4,
            emas: [None; 6],
            bar_count: 0,
        })
    }
}

impl Signal for T3 {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bar_count += 1;
        let mut val = bar.close;
        for i in 0..6 {
            let ema = match self.emas[i] {
                None => {
                    self.emas[i] = Some(val);
                    val
                }
                Some(prev) => {
                    let v = prev + self.k * (val - prev);
                    self.emas[i] = Some(v);
                    v
                }
            };
            val = ema;
        }
        // Need 6*(period-1)+1 bars minimum
        let min_bars = 6 * (self.period.saturating_sub(1)) + 1;
        if self.bar_count < min_bars {
            return Ok(SignalValue::Unavailable);
        }
        let e3 = self.emas[2].unwrap();
        let e4 = self.emas[3].unwrap();
        let e5 = self.emas[4].unwrap();
        let e6 = self.emas[5].unwrap();
        let t3 = self.c1 * e6 + self.c2 * e5 + self.c3 * e4 + self.c4 * e3;
        Ok(SignalValue::Scalar(t3))
    }

    fn is_ready(&self) -> bool {
        let min_bars = 6 * (self.period.saturating_sub(1)) + 1;
        self.bar_count >= min_bars
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.emas = [None; 6];
        self.bar_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> BarInput {
        BarInput::from_close(close.parse().unwrap())
    }

    #[test]
    fn test_t3_invalid_period() {
        assert!(T3::new("t", 0, None).is_err());
    }

    #[test]
    fn test_t3_invalid_v_factor() {
        assert!(T3::new("t", 5, Some(0.0)).is_err());
        assert!(T3::new("t", 5, Some(1.5)).is_err());
    }

    #[test]
    fn test_t3_unavailable_before_warmup() {
        let mut t = T3::new("t", 5, None).unwrap();
        for _ in 0..5 {
            assert_eq!(t.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_t3_ready_after_warmup() {
        let mut t = T3::new("t", 2, None).unwrap();
        // min_bars = 6*(2-1)+1 = 7
        let mut last = SignalValue::Unavailable;
        for i in 0..7 {
            let p = format!("{}", 100 + i);
            last = t.update(&bar(&p)).unwrap();
        }
        assert!(t.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_t3_flat_prices_near_flat() {
        let mut t = T3::new("t", 2, None).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = t.update(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            let diff = (v - dec!(100)).abs();
            assert!(diff < dec!(1), "T3 far from price on flat series: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_t3_reset_clears_state() {
        let mut t = T3::new("t", 2, None).unwrap();
        for i in 0..7 {
            let p = format!("{}", 100 + i);
            t.update(&bar(&p)).unwrap();
        }
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
    }

    #[test]
    fn test_t3_period_and_name() {
        let t = T3::new("my_t3", 5, None).unwrap();
        assert_eq!(t.period(), 5);
        assert_eq!(t.name(), "my_t3");
    }
}
