//! Linear Regression Forecast indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Linear Regression Forecast.
///
/// Projects the least-squares regression line N bars into the future.
///
/// Returns SignalValue::Unavailable until period bars have been seen.
///
/// # Example
///
/// ```rust
/// use fin_primitives::signals::indicators::LinRegForecast;
/// let f = LinRegForecast::new("lrf", 5, 1).unwrap();
/// assert_eq!(f.bars_ahead(), 1);
/// ```
pub struct LinRegForecast {
    name: String,
    period: usize,
    bars_ahead: usize,
    history: VecDeque<Decimal>,
}

impl LinRegForecast {
    /// Create a new `LinRegForecast` with the given `period` (minimum 2) and
    /// `bars_ahead` projection horizon.
    ///
    /// # Errors
    /// Returns `FinError::InvalidPeriod` if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize, bars_ahead: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, bars_ahead, history: VecDeque::with_capacity(period) })
    }

    /// Number of bars ahead the regression line is projected.
    pub fn bars_ahead(&self) -> usize { self.bars_ahead }
}

impl Signal for LinRegForecast {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period { self.history.pop_front(); }
        if self.history.len() < self.period { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let (mut sum_x, mut sum_y, mut sum_xy, mut sum_x2) =
            (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
        for (i, &y) in self.history.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let x = Decimal::from(i as u32);
            sum_x += x; sum_y += y; sum_xy += x * y; sum_x2 += x * x;
        }
        let mean_y = sum_y.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;
        let mean_x = sum_x.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;
        let denom = sum_x2
            .checked_sub(sum_x.checked_mul(sum_x).ok_or(FinError::ArithmeticOverflow)?
                .checked_div(n).ok_or(FinError::ArithmeticOverflow)?)
            .ok_or(FinError::ArithmeticOverflow)?;
        if denom.is_zero() {
            return Ok(SignalValue::Scalar(self.history.back().copied().unwrap_or(Decimal::ZERO)));
        }
        let slope = sum_xy
            .checked_sub(n.checked_mul(mean_x).ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(mean_y).ok_or(FinError::ArithmeticOverflow)?)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_div(denom).ok_or(FinError::ArithmeticOverflow)?;
        let intercept = mean_y
            .checked_sub(slope.checked_mul(mean_x).ok_or(FinError::ArithmeticOverflow)?)
            .ok_or(FinError::ArithmeticOverflow)?;
        #[allow(clippy::cast_possible_truncation)]
        let fx = Decimal::from((self.period - 1 + self.bars_ahead) as u32);
        let forecast = intercept.checked_add(slope.checked_mul(fx)
            .ok_or(FinError::ArithmeticOverflow)?).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(forecast))
    }

    fn is_ready(&self) -> bool { self.history.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.history.clear(); }
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
        OhlcvBar { symbol: Symbol::new("X").unwrap(), open: p, high: p, low: p, close: p,
            volume: Quantity::zero(), ts_open: NanoTimestamp::new(0), ts_close: NanoTimestamp::new(1), tick_count: 1 }
    }

    #[test]
    fn test_lrf_period_too_small() { assert!(LinRegForecast::new("f", 1, 1).is_err()); }

    #[test]
    fn test_lrf_unavailable_before_period() {
        let mut f = LinRegForecast::new("f", 3, 1).unwrap();
        assert_eq!(f.update_bar(&bar("10")).unwrap(), SignalValue::Unavailable);
        assert_eq!(f.update_bar(&bar("20")).unwrap(), SignalValue::Unavailable);
        assert!(f.update_bar(&bar("30")).unwrap().is_scalar());
    }

    #[test]
    fn test_lrf_perfect_trend_1_ahead() {
        let mut f = LinRegForecast::new("f", 3, 1).unwrap();
        f.update_bar(&bar("10")).unwrap();
        f.update_bar(&bar("20")).unwrap();
        if let SignalValue::Scalar(v) = f.update_bar(&bar("30")).unwrap() {
            assert!((v - dec!(40)).abs() < dec!(0.001), "got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_lrf_constant_price() {
        let mut f = LinRegForecast::new("f", 3, 5).unwrap();
        for _ in 0..2 { f.update_bar(&bar("50")).unwrap(); }
        assert_eq!(f.update_bar(&bar("50")).unwrap(), SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_lrf_reset() {
        let mut f = LinRegForecast::new("f", 2, 1).unwrap();
        f.update_bar(&bar("10")).unwrap();
        f.update_bar(&bar("20")).unwrap();
        assert!(f.is_ready());
        f.reset();
        assert!(!f.is_ready());
    }

    #[test]
    fn test_lrf_accessors() {
        let f = LinRegForecast::new("f", 5, 3).unwrap();
        assert_eq!(f.bars_ahead(), 3);
        assert_eq!(f.period(), 5);
    }
}
