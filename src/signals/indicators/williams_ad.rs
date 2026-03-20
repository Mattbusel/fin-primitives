//! Williams Accumulation/Distribution indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Williams Accumulation/Distribution (A/D) — a cumulative volume-momentum oscillator.
///
/// On each bar:
/// - If `close > prev_close`: `AD += (close - true_low) × volume`
/// - If `close < prev_close`: `AD -= (true_high - close) × volume`
/// - If `close == prev_close`: AD unchanged
///
/// where `true_high = max(high, prev_close)` and `true_low = min(low, prev_close)`.
///
/// Returns [`SignalValue::Unavailable`] for the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WilliamsAD;
/// use fin_primitives::signals::Signal;
///
/// let mut wad = WilliamsAD::new("wad");
/// assert_eq!(wad.period(), 1);
/// ```
pub struct WilliamsAD {
    name: String,
    ad: Decimal,
    prev_close: Option<Decimal>,
}

impl WilliamsAD {
    /// Constructs a new `WilliamsAD` line.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ad: Decimal::ZERO,
            prev_close: None,
        }
    }

    /// Returns the current cumulative A/D value.
    pub fn value(&self) -> Decimal {
        self.ad
    }
}

impl Signal for WilliamsAD {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let prev_close = match self.prev_close {
            Some(pc) => pc,
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
        };

        let true_high = bar.high.max(prev_close);
        let true_low  = bar.low.min(prev_close);
        let vol = bar.volume;

        if bar.close > prev_close {
            self.ad += (bar.close - true_low) * vol;
        } else if bar.close < prev_close {
            self.ad -= (true_high - bar.close) * vol;
        }
        // equal: AD unchanged

        self.prev_close = Some(bar.close);
        Ok(SignalValue::Scalar(self.ad))
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some() && !self.ad.is_zero()
            || self.prev_close.is_some()
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.ad = Decimal::ZERO;
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

    fn bar(h: &str, l: &str, c: &str, vol: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let v  = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl,
            high: hi,
            low: lo,
            close: cl,
            volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_wad_first_bar_unavailable() {
        let mut wad = WilliamsAD::new("wad");
        assert_eq!(wad.update_bar(&bar("110", "90", "100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_wad_rising_close_positive() {
        let mut wad = WilliamsAD::new("wad");
        wad.update_bar(&bar("110", "90", "100", "100")).unwrap();
        // close 110 > prev_close 100: AD += (110 - min(105, 100)) * 100 = (110-100)*100 = 1000
        let v = wad.update_bar(&bar("115", "105", "110", "100")).unwrap();
        if let SignalValue::Scalar(d) = v {
            assert!(d > dec!(0), "expected positive AD, got {d}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wad_falling_close_negative() {
        let mut wad = WilliamsAD::new("wad");
        wad.update_bar(&bar("110", "90", "100", "100")).unwrap();
        // close 90 < prev_close 100: AD -= (max(95, 100) - 90) * 100 = (100-90)*100 = 1000
        let v = wad.update_bar(&bar("95", "85", "90", "100")).unwrap();
        if let SignalValue::Scalar(d) = v {
            assert!(d < dec!(0), "expected negative AD, got {d}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wad_reset() {
        let mut wad = WilliamsAD::new("wad");
        wad.update_bar(&bar("110", "90", "100", "100")).unwrap();
        wad.update_bar(&bar("115", "105", "110", "100")).unwrap();
        wad.reset();
        assert_eq!(wad.value(), dec!(0));
    }
}
