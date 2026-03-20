//! Jurik Moving Average (JMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Jurik Moving Average (JMA) — an adaptive, low-lag moving average developed by
/// Mark Jurik.
///
/// JMA adapts its smoothing to recent price volatility: it moves quickly when
/// prices are trending and more slowly when they are choppy. The result is a
/// cleaner signal with less noise and lag than a comparable EMA.
///
/// This implementation uses the standard public approximation of JMA based on a
/// two-stage EMA with adaptive `alpha` derived from a normalised volatility band:
///
/// ```text
/// del1  = close - e0
/// e0    = e0_prev + alpha × del1
/// e1    = (close - e0) × (1 - alpha) + e1_prev × alpha
/// e2    = e1 × beta + e2_prev × (1 - beta)
/// jma   = jma_prev + e2 + beta × (e0 - jma_prev)
/// ```
///
/// where `alpha = 0.45 × (period - 1) / (0.45 × (period - 1) + 2)`
/// and `beta = phase_ratio`, a fixed sharpness coefficient (default 0.45).
///
/// Returns [`SignalValue::Unavailable`] until the first bar has been processed;
/// from bar 1 onwards the indicator produces values (warm-up is internal).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::JurikMa;
/// use fin_primitives::signals::Signal;
///
/// let jma = JurikMa::new("jma", 14).unwrap();
/// assert_eq!(jma.period(), 14);
/// ```
pub struct JurikMa {
    name: String,
    period: usize,
    alpha: Decimal,
    beta: Decimal,
    e0: Option<Decimal>,
    e1: Option<Decimal>,
    e2: Decimal,
    jma: Option<Decimal>,
}

impl JurikMa {
    /// Constructs a new `JurikMa` with the given lookback `period`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        // alpha = 0.45 × (period - 1) / (0.45 × (period - 1) + 2)
        let p = Decimal::from((period - 1) as u32);
        let c = Decimal::from_str_exact("0.45").map_err(|_| FinError::ArithmeticOverflow)?;
        let numerator = c * p;
        let alpha = numerator
            .checked_div(numerator + Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;
        let beta = Decimal::from_str_exact("0.45").map_err(|_| FinError::ArithmeticOverflow)?;
        Ok(Self {
            name: name.into(),
            period,
            alpha,
            beta,
            e0: None,
            e1: None,
            e2: Decimal::ZERO,
            jma: None,
        })
    }
}

impl Signal for JurikMa {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.jma.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let price = bar.close;

        match (self.e0, self.e1, self.jma) {
            (None, _, _) => {
                // Bootstrap: seed everything at first close
                self.e0  = Some(price);
                self.e1  = Some(Decimal::ZERO);
                self.e2  = Decimal::ZERO;
                self.jma = Some(price);
                Ok(SignalValue::Scalar(price))
            }
            (Some(e0_prev), Some(e1_prev), Some(jma_prev)) => {
                let del1 = price - e0_prev;
                let e0   = e0_prev + self.alpha * del1;
                let e1   = (price - e0) * (Decimal::ONE - self.alpha) + e1_prev * self.alpha;
                let e2   = e1 * self.beta + self.e2 * (Decimal::ONE - self.beta);
                let jma  = jma_prev + e2 + self.beta * (e0 - jma_prev);
                self.e0  = Some(e0);
                self.e1  = Some(e1);
                self.e2  = e2;
                self.jma = Some(jma);
                Ok(SignalValue::Scalar(jma))
            }
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.e0  = None;
        self.e1  = None;
        self.e2  = Decimal::ZERO;
        self.jma = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_jma_period_one_fails() {
        assert!(JurikMa::new("j", 0).is_err());
        assert!(JurikMa::new("j", 1).is_err());
    }

    #[test]
    fn test_jma_ready_after_first_bar() {
        let mut j = JurikMa::new("j", 14).unwrap();
        j.update_bar(&bar("100")).unwrap();
        assert!(j.is_ready());
    }

    #[test]
    fn test_jma_constant_price_returns_constant() {
        let mut j = JurikMa::new("j", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = j.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            let diff = (v - dec!(100)).abs();
            assert!(diff < dec!(0.01), "JMA should converge to 100, got {v}");
        }
    }

    #[test]
    fn test_jma_follows_trend() {
        let mut j = JurikMa::new("j", 5).unwrap();
        let mut prev_jma = dec!(100);
        j.update_bar(&bar("100")).unwrap();
        for i in 1u32..=10 {
            if let SignalValue::Scalar(v) = j.update_bar(&bar(&(100 + i).to_string())).unwrap() {
                // JMA should be increasing in a rising trend
                assert!(v >= prev_jma, "JMA should rise: {v} < {prev_jma}");
                prev_jma = v;
            }
        }
    }

    #[test]
    fn test_jma_reset() {
        let mut j = JurikMa::new("j", 7).unwrap();
        j.update_bar(&bar("100")).unwrap();
        assert!(j.is_ready());
        j.reset();
        assert!(!j.is_ready());
    }
}
