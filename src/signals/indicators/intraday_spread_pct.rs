//! Intraday Spread Percent indicator -- bar spread as a percentage of the midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Intraday Spread Percent -- models the bar's bid-ask spread proxy as the range
/// expressed as a percentage of the midpoint.
///
/// ```text
/// spread_pct[t] = (high - low) / ((high + low) / 2) x 100
/// ```
///
/// High values indicate wide spreads (illiquid or volatile); low values indicate
/// tight spreads (liquid or calm market).
///
/// Returns [`SignalValue::Unavailable`] if `high + low == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::IntradaySpreadPct;
/// use fin_primitives::signals::Signal;
/// let isp = IntradaySpreadPct::new("isp");
/// assert_eq!(isp.period(), 1);
/// ```
pub struct IntradaySpreadPct {
    name: String,
}

impl IntradaySpreadPct {
    /// Constructs a new `IntradaySpreadPct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for IntradaySpreadPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = bar.high + bar.low;
        if mid.is_zero() { return Ok(SignalValue::Unavailable); }
        let spread_pct = (bar.range()) / mid * Decimal::from(200u32);
        Ok(SignalValue::Scalar(spread_pct))
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_isp_zero_range_is_zero() {
        let mut isp = IntradaySpreadPct::new("isp");
        let v = isp.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_isp_spread_correct() {
        // high=110, low=90, mid=100, spread=20 -> 20/100*100 = 20%
        let mut isp = IntradaySpreadPct::new("isp");
        let v = isp.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_isp_always_ready() {
        let isp = IntradaySpreadPct::new("isp");
        assert!(isp.is_ready());
    }

    #[test]
    fn test_isp_period_is_1() {
        let isp = IntradaySpreadPct::new("isp");
        assert_eq!(isp.period(), 1);
    }
}
