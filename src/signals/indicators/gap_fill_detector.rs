//! Gap Fill Detector indicator -- detects when a previous gap is filled.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap Fill Detector -- returns 1 when the current bar's range fills a previous
/// gap (i.e., trades through the prior gap's zone), and 0 otherwise.
///
/// A gap-up occurs when `open[t] > close[t-1]`. It's "filled" when a subsequent
/// bar's low falls at or below `close[t-1]` (the gap's lower boundary).
/// A gap-down occurs when `open[t] < close[t-1]`. It's "filled" when a bar's
/// high rises to or above `close[t-1]`.
///
/// This indicator tracks the most recent gap and signals when it is filled.
///
/// ```text
/// gap_up_target   = prev_close[gap_bar]   (lower edge of a gap-up)
/// gap_filled[t]   = 1 if gap exists AND low[t] <= gap_up_target
///                   1 if gap exists AND high[t] >= gap_down_target
///                   0 otherwise
/// ```
///
/// Returns 0 on the first bar (no prior close). Ready after the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapFillDetector;
/// use fin_primitives::signals::Signal;
/// let gfd = GapFillDetector::new("gfd");
/// assert_eq!(gfd.period(), 1);
/// ```
pub struct GapFillDetector {
    name: String,
    prev_close: Option<Decimal>,
    gap_target: Option<Decimal>,   // level that fills the gap
    gap_direction: i8,             // +1 = gap up (fill by going down), -1 = gap down
}

impl GapFillDetector {
    /// Constructs a new `GapFillDetector`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prev_close: None,
            gap_target: None,
            gap_direction: 0,
        }
    }
}

impl Signal for GapFillDetector {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pc) = self.prev_close {
            // Detect new gap
            if bar.open > pc {
                // Gap up: fill when low <= prev_close
                self.gap_target = Some(pc);
                self.gap_direction = 1;
            } else if bar.open < pc {
                // Gap down: fill when high >= prev_close
                self.gap_target = Some(pc);
                self.gap_direction = -1;
            }
            // Check if current gap is filled
            let filled = match (self.gap_target, self.gap_direction) {
                (Some(target), 1)  if bar.low  <= target => Decimal::ONE,
                (Some(target), -1) if bar.high >= target => Decimal::ONE,
                _ => Decimal::ZERO,
            };
            // If gap was filled, clear it
            if filled == Decimal::ONE {
                self.gap_target = None;
                self.gap_direction = 0;
            }
            filled
        } else {
            Decimal::ZERO
        };
        self.prev_close = Some(bar.close);
        Ok(SignalValue::Scalar(result))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gap_target = None;
        self.gap_direction = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_gfd_first_bar_is_zero() {
        let mut gfd = GapFillDetector::new("gfd");
        let v = gfd.update_bar(&bar("100", "105", "95", "102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_gfd_gap_up_not_filled() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap(); // prev_close=100
        // Gap up: open=105 > prev_close=100, low=102 > 100 -> not filled
        let v = gfd.update_bar(&bar("105", "110", "102", "108")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_gfd_gap_up_filled() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap(); // prev_close=100
        // Gap up bar: open=105 > 100, gap target = 100
        gfd.update_bar(&bar("105", "110", "102", "108")).unwrap(); // gap set to 100
        // Fill bar: low=99 <= 100 -> filled!
        let v = gfd.update_bar(&bar("108", "110", "99", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_gfd_gap_down_filled() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap(); // prev_close=100
        // Gap down bar: open=95 < 100, gap target = 100
        gfd.update_bar(&bar("95", "98", "90", "92")).unwrap(); // gap set to 100
        // Fill bar: high=101 >= 100 -> filled!
        let v = gfd.update_bar(&bar("92", "101", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_gfd_reset() {
        let mut gfd = GapFillDetector::new("gfd");
        gfd.update_bar(&bar("100", "105", "95", "100")).unwrap();
        assert!(gfd.is_ready());
        gfd.reset();
        assert!(!gfd.is_ready());
    }
}
