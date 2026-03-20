//! Stochastic Momentum Index (SMI).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Stochastic Momentum Index — a double-smoothed stochastic oscillator.
///
/// Unlike the classic stochastic, SMI measures where the close falls relative
/// to the midpoint of the high-low range, double-smoothed for reduced noise.
///
/// ```text
/// midpoint[i]  = (HH(n) + LL(n)) / 2
/// diff[i]      = close[i] - midpoint[i]
/// range[i]     = (HH(n) - LL(n)) / 2
///
/// EMA1_diff    = EMA(smooth1, diff)
/// EMA2_diff    = EMA(smooth2, EMA1_diff)       ← numerator
/// EMA1_range   = EMA(smooth1, range)
/// EMA2_range   = EMA(smooth2, EMA1_range)      ← denominator
///
/// SMI = 100 × EMA2_diff / EMA2_range           (0 when range == 0)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Smi;
/// use fin_primitives::signals::Signal;
///
/// let smi = Smi::new("smi13", 13, 3, 3).unwrap();
/// assert_eq!(smi.period(), 13);
/// ```
pub struct Smi {
    name: String,
    period: usize,
    bars: VecDeque<BarInput>,
    // double-EMA state for diff and range
    smooth1_k: Decimal,
    smooth2_k: Decimal,
    ema1_diff: Option<Decimal>,
    ema2_diff: Option<Decimal>,
    ema1_range: Option<Decimal>,
    ema2_range: Option<Decimal>,
}

impl Smi {
    /// Creates a new `Smi`.
    ///
    /// * `period`  — lookback for highest-high / lowest-low
    /// * `smooth1` — first smoothing EMA period
    /// * `smooth2` — second smoothing EMA period
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        smooth1: usize,
        smooth2: usize,
    ) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if smooth1 == 0 { return Err(FinError::InvalidPeriod(smooth1)); }
        if smooth2 == 0 { return Err(FinError::InvalidPeriod(smooth2)); }
        #[allow(clippy::cast_possible_truncation)]
        let smooth1_k = Decimal::TWO / Decimal::from((smooth1 + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let smooth2_k = Decimal::TWO / Decimal::from((smooth2 + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            bars: VecDeque::with_capacity(period),
            smooth1_k,
            smooth2_k,
            ema1_diff: None,
            ema2_diff: None,
            ema1_range: None,
            ema2_range: None,
        })
    }
}

impl Signal for Smi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars.push_back(bar.clone());
        if self.bars.len() > self.period {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let hh = self.bars.iter().map(|b| b.high).max().unwrap_or(bar.high);
        let ll = self.bars.iter().map(|b| b.low).min().unwrap_or(bar.low);
        let two = Decimal::TWO;
        let midpoint = (hh + ll) / two;
        let diff = bar.close - midpoint;
        let range = (hh - ll) / two;

        // Apply double EMA to diff
        let e1d = match self.ema1_diff {
            None => diff,
            Some(p) => diff * self.smooth1_k + p * (Decimal::ONE - self.smooth1_k),
        };
        self.ema1_diff = Some(e1d);
        let e2d = match self.ema2_diff {
            None => e1d,
            Some(p) => e1d * self.smooth2_k + p * (Decimal::ONE - self.smooth2_k),
        };
        self.ema2_diff = Some(e2d);

        // Apply double EMA to range
        let e1r = match self.ema1_range {
            None => range,
            Some(p) => range * self.smooth1_k + p * (Decimal::ONE - self.smooth1_k),
        };
        self.ema1_range = Some(e1r);
        let e2r = match self.ema2_range {
            None => e1r,
            Some(p) => e1r * self.smooth2_k + p * (Decimal::ONE - self.smooth2_k),
        };
        self.ema2_range = Some(e2r);

        let smi = if e2r == Decimal::ZERO {
            Decimal::ZERO
        } else {
            Decimal::from(100u32) * e2d / e2r
        };

        Ok(SignalValue::Scalar(smi))
    }

    fn is_ready(&self) -> bool {
        self.ema2_diff.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bars.clear();
        self.ema1_diff = None;
        self.ema2_diff = None;
        self.ema1_range = None;
        self.ema2_range = None;
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
    fn test_smi_invalid_period() {
        assert!(Smi::new("s", 0, 3, 3).is_err());
        assert!(Smi::new("s", 13, 0, 3).is_err());
        assert!(Smi::new("s", 13, 3, 0).is_err());
    }

    #[test]
    fn test_smi_unavailable_before_period() {
        let mut smi = Smi::new("s", 3, 2, 2).unwrap();
        assert_eq!(smi.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(smi.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_smi_produces_scalar_after_period() {
        let mut smi = Smi::new("s", 3, 2, 2).unwrap();
        smi.update_bar(&bar("100")).unwrap();
        smi.update_bar(&bar("101")).unwrap();
        let v = smi.update_bar(&bar("102")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(smi.is_ready());
    }

    #[test]
    fn test_smi_flat_price_is_zero() {
        // close == midpoint → diff == 0 → SMI == 0
        let mut smi = Smi::new("s", 3, 2, 2).unwrap();
        for _ in 0..20 {
            smi.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = smi.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        }
    }

    #[test]
    fn test_smi_reset() {
        let mut smi = Smi::new("s", 3, 2, 2).unwrap();
        for _ in 0..10 { smi.update_bar(&bar("100")).unwrap(); }
        assert!(smi.is_ready());
        smi.reset();
        assert!(!smi.is_ready());
        assert_eq!(smi.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
