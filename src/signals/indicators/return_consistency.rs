//! Return Consistency indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Consistency — the fraction of individual bar returns whose sign matches
/// the sign of the net N-period return.
///
/// ```text
/// net_return    = close[t] - close[t-period]
/// direction     = sign(net_return)   (+1 or -1 or 0)
/// matches       = count(sign(ret[i]) == direction, i in window)
/// output        = matches / period × 100
/// ```
///
/// - **100**: every individual bar moved in the same direction as the net trend.
/// - **50**: random walk — half the bars agree with the net direction.
/// - **Low value**: price achieved net gain/loss through mostly counter-directional moves.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars are collected, or
/// when the net return is exactly zero (no directional bias to compare against).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnConsistency;
/// use fin_primitives::signals::Signal;
/// let rc = ReturnConsistency::new("rc_10", 10).unwrap();
/// assert_eq!(rc.period(), 10);
/// ```
pub struct ReturnConsistency {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl ReturnConsistency {
    /// Constructs a new `ReturnConsistency`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for ReturnConsistency {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let prices: Vec<Decimal> = self.closes.iter().copied().collect();
        let net = *prices.last().unwrap() - prices[0];

        if net.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let direction_positive = net > Decimal::ZERO;

        // Individual bar returns: (close[i] - close[i-1]) for i in 1..=period
        let matches = prices.windows(2)
            .filter(|w| {
                let r = w[1] - w[0];
                if direction_positive { r > Decimal::ZERO } else { r < Decimal::ZERO }
            })
            .count();

        let consistency = Decimal::from(matches as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(consistency))
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_rc_invalid_period() {
        assert!(ReturnConsistency::new("rc", 0).is_err());
    }

    #[test]
    fn test_rc_unavailable_during_warmup() {
        let mut rc = ReturnConsistency::new("rc", 3).unwrap();
        for p in &["100", "101", "102"] {
            assert_eq!(rc.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!rc.is_ready());
    }

    #[test]
    fn test_rc_straight_trend_100() {
        // Monotone uptrend → every bar up → consistency = 100%
        let mut rc = ReturnConsistency::new("rc", 4).unwrap();
        for p in &["100", "101", "102", "103", "104"] {
            rc.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = rc.update_bar(&bar("105")).unwrap() {
            assert_eq!(v, dec!(100), "straight trend → 100% consistency");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rc_net_zero_unavailable() {
        // Window of 5 prices that starts and ends at same value → net = 0 → Unavailable
        // period=4 → window = period+1 = 5 prices; feed exactly 5 bars
        let mut rc = ReturnConsistency::new("rc", 4).unwrap();
        let prices = ["100", "110", "90", "100", "100"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = rc.update_bar(&bar(p)).unwrap();
        }
        // Window is [100,110,90,100,100]: first=100, last=100 → net=0 → Unavailable
        assert_eq!(last, SignalValue::Unavailable);
    }

    #[test]
    fn test_rc_retracement_below_100() {
        // Net up but with some down bars → consistency < 100%
        let mut rc = ReturnConsistency::new("rc", 4).unwrap();
        for p in &["100", "105", "102", "108", "104"] {
            rc.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = rc.update_bar(&bar("110")).unwrap() {
            assert!(v < dec!(100) && v > dec!(0), "retracements → intermediate consistency: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rc_reset() {
        let mut rc = ReturnConsistency::new("rc", 3).unwrap();
        for p in &["100", "101", "102", "103"] { rc.update_bar(&bar(p)).unwrap(); }
        assert!(rc.is_ready());
        rc.reset();
        assert!(!rc.is_ready());
    }
}
