//! Linear Regression R² indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;
use rust_decimal::prelude::ToPrimitive;

/// Linear Regression R² — coefficient of determination of close prices over a window.
///
/// ```text
/// R² = 1 − (SS_res / SS_tot)
///
/// SS_res = Σ (close_t − predicted_t)²    (residual sum of squares)
/// SS_tot = Σ (close_t − mean_close)²     (total sum of squares)
/// ```
///
/// Values near 1 indicate price is moving in a straight line (strong trend).
/// Values near 0 indicate random/choppy movement.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LinRegR2;
/// use fin_primitives::signals::Signal;
///
/// let r = LinRegR2::new("r2", 20).unwrap();
/// assert_eq!(r.period(), 20);
/// ```
pub struct LinRegR2 {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl LinRegR2 {
    /// Creates a new `LinRegR2`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for LinRegR2 {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let n = self.period as f64;
        let ys: Vec<f64> = self.closes.iter()
            .filter_map(|c| c.to_f64())
            .collect();

        if ys.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean_y = ys.iter().sum::<f64>() / n;
        let mean_x = (n - 1.0) / 2.0;

        let mut ss_xy = 0.0f64;
        let mut ss_xx = 0.0f64;
        for (i, &y) in ys.iter().enumerate() {
            let x = i as f64 - mean_x;
            ss_xy += x * (y - mean_y);
            ss_xx += x * x;
        }

        let (slope, intercept) = if ss_xx == 0.0 {
            (0.0, mean_y)
        } else {
            let slope = ss_xy / ss_xx;
            (slope, mean_y - slope * mean_x)
        };

        let ss_res: f64 = ys.iter().enumerate()
            .map(|(i, &y)| { let pred = intercept + slope * i as f64; (y - pred).powi(2) })
            .sum();
        let ss_tot: f64 = ys.iter().map(|&y| (y - mean_y).powi(2)).sum();

        let r2 = if ss_tot == 0.0 { 1.0 } else { 1.0 - ss_res / ss_tot };
        let r2_clamped = r2.max(0.0).min(1.0);

        Ok(SignalValue::Scalar(
            Decimal::try_from(r2_clamped).unwrap_or(Decimal::ZERO)
        ))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_r2_invalid() {
        assert!(LinRegR2::new("r", 0).is_err());
        assert!(LinRegR2::new("r", 1).is_err());
    }

    #[test]
    fn test_r2_unavailable_before_warmup() {
        let mut r = LinRegR2::new("r", 4).unwrap();
        for _ in 0..3 {
            assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_r2_flat_is_one() {
        // Flat price → SS_tot = 0 → R² = 1 (perfect fit, trivially)
        let mut r = LinRegR2::new("r", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..6 { last = r.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_r2_linear_is_one() {
        // Perfectly linear prices → R² = 1
        let mut r = LinRegR2::new("r", 4).unwrap();
        let prices = ["100", "101", "102", "103"];
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = r.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            // Should be very close to 1
            assert!(v > dec!(0.99), "expected ~1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_r2_noisy_below_one() {
        // Zigzag prices → R² < 1
        let mut r = LinRegR2::new("r", 4).unwrap();
        let prices = ["100", "110", "90", "115"];
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = r.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_r2_range_0_to_1() {
        let mut r = LinRegR2::new("r", 4).unwrap();
        for price in ["100", "105", "95", "102", "98", "110", "88"] {
            if let SignalValue::Scalar(v) = r.update_bar(&bar(price)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_r2_reset() {
        let mut r = LinRegR2::new("r", 4).unwrap();
        for _ in 0..6 { r.update_bar(&bar("100")).unwrap(); }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
