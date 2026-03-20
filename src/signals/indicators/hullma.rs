//! Hull Moving Average (HMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use crate::signals::indicators::Wma;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Hull Moving Average over `period` bars.
///
/// `HMA(n) = WMA(2·WMA(n/2) − WMA(n), √n)`
///
/// The HMA dramatically reduces lag compared to a plain WMA while remaining smooth.
/// It requires `n + floor(sqrt(n)) - 1` bars to warm up fully.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HullMa;
/// use fin_primitives::signals::Signal;
///
/// let mut hma = HullMa::new("hma9", 9).unwrap();
/// assert_eq!(hma.period(), 9);
/// ```
pub struct HullMa {
    name: String,
    period: usize,
    /// WMA over the full `period`
    wma_n: Wma,
    /// WMA over `period / 2`
    wma_half: Wma,
    /// sqrt(period) as usize for the outer WMA period
    sqrt_period: usize,
    /// Outer WMA applied to `2*wma_half - wma_n`
    values: VecDeque<Decimal>,
    /// Denominator for the outer WMA (1+2+...+sqrt_period)
    outer_denom: Decimal,
}

impl HullMa {
    /// Constructs a new `HullMa` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let half = (period / 2).max(1);
        let sqrt_p = (period as f64).sqrt() as usize;
        let sqrt_p = sqrt_p.max(1);
        #[allow(clippy::cast_possible_truncation)]
        let outer_denom = Decimal::from((sqrt_p * (sqrt_p + 1) / 2) as u32);
        Ok(Self {
            name: name.into(),
            period,
            wma_n: Wma::new("_wma_n", period)?,
            wma_half: Wma::new("_wma_half", half)?,
            sqrt_period: sqrt_p,
            values: VecDeque::with_capacity(sqrt_p),
            outer_denom,
        })
    }
}

impl Signal for HullMa {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let wma_n_val = self.wma_n.update(bar)?;
        let wma_half_val = self.wma_half.update(bar)?;

        // Need both WMAs ready before proceeding
        let (wn, wh) = match (wma_n_val, wma_half_val) {
            (SignalValue::Scalar(n), SignalValue::Scalar(h)) => (n, h),
            _ => return Ok(SignalValue::Unavailable),
        };

        let diff = Decimal::TWO
            .checked_mul(wh)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_sub(wn)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.values.push_back(diff);
        if self.values.len() > self.sqrt_period {
            self.values.pop_front();
        }
        if self.values.len() < self.sqrt_period {
            return Ok(SignalValue::Unavailable);
        }

        // Outer WMA: weight i+1 for values[i] (oldest gets weight 1, newest gets sqrt_period)
        #[allow(clippy::cast_possible_truncation)]
        let weighted: Decimal = self
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| *v * Decimal::from((i + 1) as u32))
            .sum();

        Ok(SignalValue::Scalar(weighted / self.outer_denom))
    }

    fn is_ready(&self) -> bool {
        self.values.len() >= self.sqrt_period && self.wma_n.is_ready()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.wma_n.reset();
        self.wma_half.reset();
        self.values.clear();
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
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hullma_period_0_error() {
        assert!(HullMa::new("h", 0).is_err());
    }

    #[test]
    fn test_hullma_constant_price_equals_price() {
        let mut hma = HullMa::new("hma9", 9).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = hma.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_hullma_reset_clears_state() {
        let mut hma = HullMa::new("hma4", 4).unwrap();
        for _ in 0..15 {
            hma.update_bar(&bar("50")).unwrap();
        }
        assert!(hma.is_ready());
        hma.reset();
        assert!(!hma.is_ready());
        assert_eq!(hma.update_bar(&bar("50")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hullma_period_1() {
        // WMA(1) = price, WMA(1/2=1) = price, HMA = WMA(2*p - p, sqrt(1)=1) = WMA(p,1) = p
        let mut hma = HullMa::new("hma1", 1).unwrap();
        let v = hma.update_bar(&bar("42")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(42)));
    }
}
