//! Price Compression Breakout — detects breakouts from a compressed (narrow-range) period.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Compression Breakout — signals when price breaks out of a low-volatility zone.
///
/// Over the last `period` bars, the indicator tracks the rolling range
/// `(max_high - min_low)`. A **breakout** is detected when the current bar's close
/// exceeds `max_high` (bullish) or falls below `min_low` (bearish) of the *previous*
/// window (the window ending at the bar before the current one).
///
/// Output:
/// - **+1**: bullish breakout (close above prior window's max high).
/// - **-1**: bearish breakout (close below prior window's min low).
/// - **0**: no breakout (close is within the prior window's range).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceCompressionBreakout;
/// use fin_primitives::signals::Signal;
/// let pcb = PriceCompressionBreakout::new("pcb_20", 20).unwrap();
/// assert_eq!(pcb.period(), 20);
/// ```
pub struct PriceCompressionBreakout {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    bars_seen: usize,
}

impl PriceCompressionBreakout {
    /// Constructs a new `PriceCompressionBreakout`.
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
            highs: VecDeque::with_capacity(period + 1),
            lows: VecDeque::with_capacity(period + 1),
            bars_seen: 0,
        })
    }
}

impl Signal for PriceCompressionBreakout {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen > self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars_seen += 1;

        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period + 1 {
            self.highs.pop_front();
            self.lows.pop_front();
        }

        if self.bars_seen <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Prior window = all bars except the current one (last element)
        let n = self.highs.len();
        let prior_high = self.highs.iter().take(n - 1).copied().fold(Decimal::MIN, Decimal::max);
        let prior_low = self.lows.iter().take(n - 1).copied().fold(Decimal::MAX, Decimal::min);

        let signal = if bar.close > prior_high {
            Decimal::ONE
        } else if bar.close < prior_low {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };

        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.bars_seen = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pcb_invalid_period() {
        assert!(PriceCompressionBreakout::new("pcb", 0).is_err());
    }

    #[test]
    fn test_pcb_unavailable_before_period_plus_1() {
        let mut pcb = PriceCompressionBreakout::new("pcb", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(pcb.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!pcb.is_ready());
    }

    #[test]
    fn test_pcb_no_breakout_gives_zero() {
        let mut pcb = PriceCompressionBreakout::new("pcb", 3).unwrap();
        for _ in 0..4 {
            pcb.update_bar(&bar("110", "90", "100")).unwrap();
        }
        // close=100 is within prior range [90,110]
        let v = pcb.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcb_bullish_breakout() {
        let mut pcb = PriceCompressionBreakout::new("pcb", 3).unwrap();
        for _ in 0..3 {
            pcb.update_bar(&bar("110", "90", "100")).unwrap();
        }
        // close=115 > prior max_high=110 → bullish breakout
        let v = pcb.update_bar(&bar("120", "112", "115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_pcb_bearish_breakout() {
        let mut pcb = PriceCompressionBreakout::new("pcb", 3).unwrap();
        for _ in 0..3 {
            pcb.update_bar(&bar("110", "90", "100")).unwrap();
        }
        // close=85 < prior min_low=90 → bearish breakout
        let v = pcb.update_bar(&bar("88", "80", "85")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_pcb_reset() {
        let mut pcb = PriceCompressionBreakout::new("pcb", 3).unwrap();
        for _ in 0..5 {
            pcb.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(pcb.is_ready());
        pcb.reset();
        assert!(!pcb.is_ready());
    }

    #[test]
    fn test_pcb_period_and_name() {
        let pcb = PriceCompressionBreakout::new("my_pcb", 20).unwrap();
        assert_eq!(pcb.period(), 20);
        assert_eq!(pcb.name(), "my_pcb");
    }
}
