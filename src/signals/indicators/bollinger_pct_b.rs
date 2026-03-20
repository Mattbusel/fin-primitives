//! Bollinger %B — position of close within Bollinger Bands.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bollinger %B — `(close - lower_band) / (upper_band - lower_band)`.
///
/// Measures where the closing price sits relative to the Bollinger Bands:
/// - **> 1.0**: close above upper band — overbought / breakout.
/// - **0.5**: close at the middle band (SMA).
/// - **< 0.0**: close below lower band — oversold / breakdown.
///
/// Uses a simple moving average for the middle band and `multiplier` standard
/// deviations for bandwidth. Returns [`SignalValue::Unavailable`] until `period`
/// bars have been seen, or when bandwidth is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BollingerPctB;
/// use fin_primitives::signals::Signal;
/// let b = BollingerPctB::new("bb_pct_b", 20, "2.0").unwrap();
/// assert_eq!(b.period(), 20);
/// ```
pub struct BollingerPctB {
    name: String,
    period: usize,
    multiplier: Decimal,
    window: VecDeque<Decimal>,
}

impl BollingerPctB {
    /// Constructs a new `BollingerPctB`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        multiplier: &str,
    ) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        let multiplier: Decimal = multiplier
            .parse()
            .map_err(|_| FinError::InvalidPeriod(period))?;
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for BollingerPctB {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = Decimal::from(self.period as u32);
        let sum: Decimal = self.window.iter().sum();
        let sma = sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;

        let variance: Decimal = self
            .window
            .iter()
            .map(|&c| {
                let d = c - sma;
                d * d
            })
            .sum::<Decimal>()
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;

        // Use f64 sqrt then convert back
        let variance_f64: f64 = variance
            .to_string()
            .parse()
            .unwrap_or(0.0_f64);
        let std_dev = Decimal::try_from(variance_f64.sqrt())
            .unwrap_or(Decimal::ZERO);

        let bandwidth = self.multiplier * std_dev * Decimal::TWO;
        if bandwidth.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let lower = sma - self.multiplier * std_dev;
        let pct_b = (bar.close - lower)
            .checked_div(bandwidth)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(pct_b))
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_bpctb_invalid_period() {
        assert!(BollingerPctB::new("b", 0, "2.0").is_err());
        assert!(BollingerPctB::new("b", 1, "2.0").is_err());
    }

    #[test]
    fn test_bpctb_unavailable_before_period() {
        let mut s = BollingerPctB::new("b", 3, "2.0").unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_bpctb_flat_prices_unavailable() {
        // Flat prices → zero std dev → bandwidth zero → Unavailable
        let mut s = BollingerPctB::new("b", 3, "2.0").unwrap();
        for _ in 0..3 { s.update_bar(&bar("100")).unwrap(); }
        let v = s.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_bpctb_at_sma_gives_half() {
        // period=3, window=[90,110,100]: SMA=100, close=100 at midpoint → %B=0.5
        // For %B=0.5: close must equal SMA. With [90,110,100]: SMA=(90+110+100)/3=100 ✓
        let mut s = BollingerPctB::new("b", 3, "2.0").unwrap();
        s.update_bar(&bar("90")).unwrap();
        s.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            let diff = (v - dec!(0.5)).abs();
            assert!(diff < dec!(0.001), "close at SMA should give %B=0.5: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bpctb_close_above_sma_gives_pctb_above_half() {
        // period=3, window=[90,100,115]: SMA=101.67, close=115 > SMA → %B > 0.5
        let mut s = BollingerPctB::new("b", 3, "2.0").unwrap();
        s.update_bar(&bar("90")).unwrap();
        s.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("115")).unwrap() {
            assert!(v > dec!(0.5), "close above SMA should give %B > 0.5: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bpctb_reset() {
        let mut s = BollingerPctB::new("b", 2, "2.0").unwrap();
        s.update_bar(&bar("90")).unwrap();
        s.update_bar(&bar("110")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
