//! SMA Distance Percent — close's percentage distance above/below its N-period SMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// SMA Distance Percent — `(close - SMA(close, period)) / SMA(close, period) * 100`.
///
/// Measures how far price has deviated from its rolling average, expressed as a
/// percentage:
/// - **Positive**: price is above its SMA (extended to the upside).
/// - **Negative**: price is below its SMA (extended to the downside).
/// - **Zero**: price equals the SMA.
///
/// Useful for mean-reversion strategies and identifying overbought/oversold conditions
/// without the fixed ±2 std-dev scaling of Bollinger Bands.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen,
/// or when the SMA is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SmaDistancePct;
/// use fin_primitives::signals::Signal;
/// let sdp = SmaDistancePct::new("sdp_20", 20).unwrap();
/// assert_eq!(sdp.period(), 20);
/// ```
pub struct SmaDistancePct {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    sum: Decimal,
}

impl SmaDistancePct {
    /// Constructs a new `SmaDistancePct`.
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
            closes: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for SmaDistancePct {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.sum += bar.close;
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            let removed = self.closes.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if sma.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let dist_pct = (bar.close - sma)
            .checked_div(sma)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::ONE_HUNDRED)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(dist_pct))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.sum = Decimal::ZERO;
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
    fn test_sdp_invalid_period() {
        assert!(SmaDistancePct::new("sdp", 0).is_err());
    }

    #[test]
    fn test_sdp_unavailable_before_period() {
        let mut sdp = SmaDistancePct::new("sdp", 3).unwrap();
        assert_eq!(sdp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sdp.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!sdp.is_ready());
    }

    #[test]
    fn test_sdp_price_at_sma_gives_zero() {
        // Constant prices → close = SMA → distance = 0
        let mut sdp = SmaDistancePct::new("sdp", 3).unwrap();
        for _ in 0..4 {
            sdp.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = sdp.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(0.0001), "constant prices should give ~0 distance: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sdp_known_value() {
        // SMA of [100, 100, 110] = 310/3 ≈ 103.33, close=110
        // distance = (110 - 103.33) / 103.33 * 100 ≈ 6.45%
        let mut sdp = SmaDistancePct::new("sdp", 3).unwrap();
        sdp.update_bar(&bar("100")).unwrap();
        sdp.update_bar(&bar("100")).unwrap();
        let v = sdp.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "above-SMA close should give positive distance: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sdp_below_sma_negative() {
        // Feed high bars then a low one
        let mut sdp = SmaDistancePct::new("sdp", 3).unwrap();
        sdp.update_bar(&bar("110")).unwrap();
        sdp.update_bar(&bar("110")).unwrap();
        let v = sdp.update_bar(&bar("90")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(0), "below-SMA close should give negative distance: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sdp_reset() {
        let mut sdp = SmaDistancePct::new("sdp", 3).unwrap();
        for _ in 0..4 {
            sdp.update_bar(&bar("100")).unwrap();
        }
        assert!(sdp.is_ready());
        sdp.reset();
        assert!(!sdp.is_ready());
    }

    #[test]
    fn test_sdp_period_and_name() {
        let sdp = SmaDistancePct::new("my_sdp", 20).unwrap();
        assert_eq!(sdp.period(), 20);
        assert_eq!(sdp.name(), "my_sdp");
    }
}
