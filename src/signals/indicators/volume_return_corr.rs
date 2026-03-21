//! Volume-Return Correlation indicator.
//!
//! Measures the rolling Pearson correlation between bar volume and absolute
//! bar return over a sliding window, capturing whether large moves tend to
//! occur on high or low volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Rolling Pearson correlation between `volume` and `|close - open|`.
///
/// A positive value means large absolute returns co-occur with high volume
/// (conviction moves). A negative value means large moves happen on thin volume
/// (potential liquidity gaps). Near zero indicates no relationship.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// when either series has zero variance (all identical values).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeReturnCorr;
/// use fin_primitives::signals::Signal;
///
/// let vrc = VolumeReturnCorr::new("vrc", 20).unwrap();
/// assert_eq!(vrc.period(), 20);
/// assert!(!vrc.is_ready());
/// ```
pub struct VolumeReturnCorr {
    name: String,
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (volume, abs_return)
}

impl VolumeReturnCorr {
    /// Constructs a new `VolumeReturnCorr`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeReturnCorr {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let abs_ret = (bar.close - bar.open).abs();
        let vol = bar.volume;

        self.window.push_back((vol, abs_ret));
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        for (v, r) in &self.window {
            sum_x += v.to_f64().unwrap_or(0.0);
            sum_y += r.to_f64().unwrap_or(0.0);
        }
        let mean_x = sum_x / n;
        let mean_y = sum_y / n;

        let mut cov = 0.0_f64;
        let mut var_x = 0.0_f64;
        let mut var_y = 0.0_f64;
        for (v, r) in &self.window {
            let dx = v.to_f64().unwrap_or(0.0) - mean_x;
            let dy = r.to_f64().unwrap_or(0.0) - mean_y;
            cov += dx * dy;
            var_x += dx * dx;
            var_y += dy * dy;
        }

        let denom = (var_x * var_y).sqrt();
        if denom == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let corr = cov / denom;
        let result = Decimal::try_from(corr).unwrap_or(Decimal::ZERO);
        Ok(SignalValue::Scalar(result))
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

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let (high, low) = if c >= o { (c, o) } else { (o, c) };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o,
            high,
            low,
            close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vrc_invalid_period() {
        assert!(VolumeReturnCorr::new("vrc", 0).is_err());
        assert!(VolumeReturnCorr::new("vrc", 1).is_err());
    }

    #[test]
    fn test_vrc_unavailable_during_warmup() {
        let mut vrc = VolumeReturnCorr::new("vrc", 5).unwrap();
        for _ in 0..4 {
            assert_eq!(vrc.update_bar(&bar("100", "110", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vrc_perfect_positive_correlation() {
        // volume and |return| always identical → perfect correlation = 1
        let mut vrc = VolumeReturnCorr::new("vrc", 5).unwrap();
        let pairs = [("100", "101", "10"), ("100", "102", "20"), ("100", "103", "30"),
                     ("100", "104", "40"), ("100", "105", "50")];
        let mut last = SignalValue::Unavailable;
        for (o, c, v) in pairs {
            last = vrc.update_bar(&bar(o, c, v)).unwrap();
        }
        if let SignalValue::Scalar(s) = last {
            assert!(s > dec!(0.99), "expected ~1.0 correlation: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vrc_zero_variance_unavailable() {
        // All volumes identical → var_x = 0 → Unavailable
        let mut vrc = VolumeReturnCorr::new("vrc", 3).unwrap();
        vrc.update_bar(&bar("100", "105", "1000")).unwrap();
        vrc.update_bar(&bar("100", "110", "1000")).unwrap();
        let v = vrc.update_bar(&bar("100", "115", "1000")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vrc_result_in_range() {
        let mut vrc = VolumeReturnCorr::new("vrc", 4).unwrap();
        let pairs = [("100", "105", "100"), ("100", "95", "200"),
                     ("100", "110", "50"), ("100", "90", "400")];
        let mut last = SignalValue::Unavailable;
        for (o, c, v) in pairs {
            last = vrc.update_bar(&bar(o, c, v)).unwrap();
        }
        if let SignalValue::Scalar(s) = last {
            assert!(s >= dec!(-1) && s <= dec!(1), "correlation out of [-1,1]: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vrc_reset() {
        let mut vrc = VolumeReturnCorr::new("vrc", 3).unwrap();
        for _ in 0..3 {
            vrc.update_bar(&bar("100", "105", "1000")).unwrap();
        }
        assert!(vrc.is_ready());
        vrc.reset();
        assert!(!vrc.is_ready());
    }
}
