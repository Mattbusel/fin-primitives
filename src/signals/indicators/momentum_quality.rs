//! Momentum Quality indicator -- fraction of period gains that are bars-above-SMA.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Momentum Quality -- ratio of positive momentum bars (close above rolling SMA)
/// to total bars in the period, expressed as a percentage.
///
/// A high value (near 100%) indicates consistent bullish momentum with the price
/// rarely dipping below its average. A low value indicates choppy or bearish action.
///
/// ```text
/// sma[t]   = SMA(close, period)
/// above[t] = 1 if close[t] > sma[t], else 0
/// mq[t]    = sum(above, period) / period * 100
/// ```
///
/// Note: uses the concurrent SMA for each bar, so the first `period - 1` bars are
/// all considered "not above SMA" until the SMA is ready.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MomentumQuality;
/// use fin_primitives::signals::Signal;
/// let mq = MomentumQuality::new("mq", 14).unwrap();
/// assert_eq!(mq.period(), 14);
/// ```
pub struct MomentumQuality {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
    above_window: VecDeque<u8>,
    above_count: usize,
}

impl MomentumQuality {
    /// Constructs a new `MomentumQuality`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
            above_window: VecDeque::with_capacity(period),
            above_count: 0,
        })
    }
}

impl Signal for MomentumQuality {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Update SMA window
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        // Compute current SMA and check if close is above
        #[allow(clippy::cast_possible_truncation)]
        let above: u8 = if self.window.len() == self.period {
            let sma = self.sum / Decimal::from(self.period as u32);
            if bar.close > sma { 1 } else { 0 }
        } else {
            0 // Not yet enough data for SMA -> count as not above
        };
        self.above_window.push_back(above);
        self.above_count += above as usize;
        if self.above_window.len() > self.period {
            if let Some(old) = self.above_window.pop_front() { self.above_count -= old as usize; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let mq = Decimal::from(self.above_count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(mq))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
        self.above_window.clear();
        self.above_count = 0;
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
    fn test_mq_period_0_error() { assert!(MomentumQuality::new("mq", 0).is_err()); }

    #[test]
    fn test_mq_unavailable_before_period() {
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        assert_eq!(mq.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_mq_flat_price_is_0_or_low() {
        // Flat price: close == SMA always, so never strictly above -> 0%
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        mq.update_bar(&bar("100")).unwrap();
        mq.update_bar(&bar("100")).unwrap();
        let v = mq.update_bar(&bar("100")).unwrap();
        // SMA = 100, close = 100, not strictly above -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mq_strongly_rising_is_positive() {
        // Rising prices: last bars are above SMA -> positive MQ
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        mq.update_bar(&bar("100")).unwrap();
        mq.update_bar(&bar("110")).unwrap();
        let v = mq.update_bar(&bar("120")).unwrap();
        // SMA = (100+110+120)/3 = 110, close=120 > 110 -> above=1 in last bar
        if let SignalValue::Scalar(q) = v {
            assert!(q > dec!(0), "rising prices, some above SMA, got {q}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mq_reset() {
        let mut mq = MomentumQuality::new("mq", 3).unwrap();
        for p in ["100", "110", "120"] { mq.update_bar(&bar(p)).unwrap(); }
        assert!(mq.is_ready());
        mq.reset();
        assert!(!mq.is_ready());
    }
}
