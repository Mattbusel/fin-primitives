//! Volume-Return Correlation — rolling Pearson correlation between bar returns and volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Return Correlation — rolling Pearson correlation between `close_return` and `volume`.
///
/// Measures whether large price moves co-occur with large volume over a rolling window:
/// - **Positive (near +1)**: high-volume bars drive big moves — healthy trend confirmation.
/// - **Negative (near -1)**: big moves on low volume — weak trend or fading momentum.
/// - **Near zero**: no systematic relationship between volume and price change magnitude.
///
/// Uses `(close - prev_close) / prev_close` as the return, and raw volume.
/// Returns [`SignalValue::Unavailable`] until `period` return-volume pairs have been collected
/// or if the correlation is undefined (zero variance in either series).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeReturnCorrelation;
/// use fin_primitives::signals::Signal;
/// let vrc = VolumeReturnCorrelation::new("vrc_10", 10).unwrap();
/// assert_eq!(vrc.period(), 10);
/// ```
pub struct VolumeReturnCorrelation {
    name: String,
    period: usize,
    window: VecDeque<(f64, f64)>,
    prev_close: Option<Decimal>,
}

impl VolumeReturnCorrelation {
    /// Constructs a new `VolumeReturnCorrelation`.
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
            prev_close: None,
        })
    }
}

fn pearson_r(data: &VecDeque<(f64, f64)>) -> Option<f64> {
    let n = data.len() as f64;
    if n < 2.0 { return None; }

    let sum_x: f64 = data.iter().map(|&(x, _)| x).sum();
    let sum_y: f64 = data.iter().map(|&(_, y)| y).sum();
    let mean_x = sum_x / n;
    let mean_y = sum_y / n;

    let mut cov = 0.0_f64;
    let mut var_x = 0.0_f64;
    let mut var_y = 0.0_f64;

    for &(x, y) in data {
        let dx = x - mean_x;
        let dy = y - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }

    if var_x <= 0.0 || var_y <= 0.0 {
        return None;
    }

    Some(cov / (var_x.sqrt() * var_y.sqrt()))
}

impl Signal for VolumeReturnCorrelation {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = ((bar.close - pc) / pc).to_f64_saturating();
                let vol = bar.volume.to_f64_saturating();
                if vol.is_finite() && ret.is_finite() {
                    self.window.push_back((ret, vol));
                    if self.window.len() > self.period {
                        self.window.pop_front();
                    }
                }
            }
        }

        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        match pearson_r(&self.window) {
            Some(r) => {
                let clamped = r.clamp(-1.0, 1.0);
                Ok(SignalValue::Scalar(Decimal::try_from(clamped).unwrap_or(Decimal::ZERO)))
            }
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.window.clear();
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: u64) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(Decimal::from(vol)),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vrc_invalid_period() {
        assert!(VolumeReturnCorrelation::new("vrc", 0).is_err());
        assert!(VolumeReturnCorrelation::new("vrc", 1).is_err());
    }

    #[test]
    fn test_vrc_unavailable_during_warmup() {
        let mut s = VolumeReturnCorrelation::new("vrc", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100", 1000)).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101", 1200)).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_vrc_positive_correlation() {
        // Rising price with increasing volume → positive correlation
        let mut s = VolumeReturnCorrelation::new("vrc", 4).unwrap();
        let bars = [("100",1000u64),("102",2000),("104",3000),("106",4000),("108",5000)];
        let mut last = SignalValue::Unavailable;
        for &(c, v) in &bars { last = s.update_bar(&bar(c, v)).unwrap(); }
        if let SignalValue::Scalar(r) = last {
            assert!(r > dec!(0), "rising price + rising vol → positive correlation: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vrc_output_in_range() {
        let mut s = VolumeReturnCorrelation::new("vrc", 4).unwrap();
        let bars = [("100",500u64),("98",2000),("102",300),("99",1800),("103",400)];
        for &(c, v) in &bars {
            if let SignalValue::Scalar(r) = s.update_bar(&bar(c, v)).unwrap() {
                assert!(r >= dec!(-1) && r <= dec!(1), "correlation must be in [-1,1]: {r}");
            }
        }
    }

    #[test]
    fn test_vrc_reset() {
        let mut s = VolumeReturnCorrelation::new("vrc", 3).unwrap();
        for &(c, v) in &[("100",1000u64),("102",2000),("104",3000),("106",4000)] {
            s.update_bar(&bar(c, v)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
