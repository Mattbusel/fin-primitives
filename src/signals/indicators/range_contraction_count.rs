//! Range Contraction Count indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Range Contraction Count — counts consecutive bars where the current bar's
/// range (`high - low`) is strictly less than the previous bar's range.
///
/// Resets to 0 when the current range is ≥ the previous range.
/// Returns [`SignalValue::Unavailable`] on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeContractionCount;
/// use fin_primitives::signals::Signal;
///
/// let rcc = RangeContractionCount::new("rcc").unwrap();
/// assert_eq!(rcc.period(), 1);
/// ```
pub struct RangeContractionCount {
    name: String,
    prev_range: Option<Decimal>,
    count: u32,
}

impl RangeContractionCount {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_range: None, count: 0 })
    }

    /// Returns the current contraction streak.
    pub fn count(&self) -> u32 { self.count }
}

impl Signal for RangeContractionCount {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_range.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let result = match self.prev_range {
            None => SignalValue::Unavailable,
            Some(pr) => {
                if range < pr {
                    self.count += 1;
                } else {
                    self.count = 0;
                }
                SignalValue::Scalar(Decimal::from(self.count))
            }
        };
        self.prev_range = Some(range);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_range = None;
        self.count = 0;
    }
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
    fn test_rcc_unavailable_first_bar() {
        let mut rcc = RangeContractionCount::new("rcc").unwrap();
        assert_eq!(rcc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rcc_contracting_streak() {
        let mut rcc = RangeContractionCount::new("rcc").unwrap();
        rcc.update_bar(&bar("120", "80")).unwrap(); // range=40, seed
        let r1 = rcc.update_bar(&bar("115", "85")).unwrap(); // range=30 < 40 → count=1
        let r2 = rcc.update_bar(&bar("112", "88")).unwrap(); // range=24 < 30 → count=2
        assert_eq!(r1, SignalValue::Scalar(dec!(1)));
        assert_eq!(r2, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_rcc_expansion_resets() {
        let mut rcc = RangeContractionCount::new("rcc").unwrap();
        rcc.update_bar(&bar("120", "80")).unwrap();
        rcc.update_bar(&bar("115", "85")).unwrap();
        // expansion
        let result = rcc.update_bar(&bar("125", "75")).unwrap(); // range=50 > 30 → 0
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rcc_reset() {
        let mut rcc = RangeContractionCount::new("rcc").unwrap();
        rcc.update_bar(&bar("120", "80")).unwrap();
        rcc.update_bar(&bar("115", "85")).unwrap();
        rcc.reset();
        assert_eq!(rcc.count(), 0);
        assert_eq!(rcc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }
}
