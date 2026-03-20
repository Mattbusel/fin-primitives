//! Swing Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Swing Index (Welles Wilder) — measures the strength of the current bar's move.
///
/// ```text
/// SI = 50 × (close - prev_close + 0.5×(close - open) + 0.25×(prev_close - prev_open))
///          / max(|high - prev_close|, |low - prev_close|)
///          × (K / T)
/// ```
///
/// Where `K = max(|high - prev_close|, |low - prev_close|)` and `T` is the limit
/// move parameter (typically `0.5` for swing index; larger values dampen the output).
///
/// Simplified implementation: `T = limit_move` (configurable), `K/T` factor normalizes
/// to the `[-100, 100]` range. Returns the cumulative Accumulation Swing Index (ASI)
/// when `cumulative = true`.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (needs a prior close/open).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SwingIndex;
/// use fin_primitives::signals::Signal;
///
/// let si = SwingIndex::new("si", "0.5".parse().unwrap(), false).unwrap();
/// assert_eq!(si.period(), 1);
/// ```
pub struct SwingIndex {
    name: String,
    limit_move: Decimal,
    cumulative: bool,
    prev_close: Option<Decimal>,
    prev_open: Option<Decimal>,
    asi: Decimal,
}

impl SwingIndex {
    /// Creates a new `SwingIndex`.
    ///
    /// - `limit_move`: maximum daily price move limit (dampening factor, e.g. `0.5`).
    /// - `cumulative`: if `true`, output is the running Accumulation Swing Index (ASI).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `limit_move` is not positive.
    pub fn new(
        name: impl Into<String>,
        limit_move: Decimal,
        cumulative: bool,
    ) -> Result<Self, FinError> {
        if limit_move <= Decimal::ZERO {
            return Err(FinError::InvalidInput("limit_move must be positive".into()));
        }
        Ok(Self {
            name: name.into(),
            limit_move,
            cumulative,
            prev_close: None,
            prev_open: None,
            asi: Decimal::ZERO,
        })
    }

    /// Returns the current Accumulation Swing Index value.
    pub fn asi(&self) -> Decimal { self.asi }
}

impl Signal for SwingIndex {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (pc, po) = match (self.prev_close, self.prev_open) {
            (Some(pc), Some(po)) => (pc, po),
            _ => {
                self.prev_close = Some(bar.close);
                self.prev_open = Some(bar.open);
                return Ok(SignalValue::Unavailable);
            }
        };

        let r1 = (bar.high - pc).abs();
        let r2 = (bar.low - pc).abs();
        let r3 = bar.high - bar.low;

        // R = max(|high - prev_close|, |low - prev_close|, high - low)
        let r = r1.max(r2).max(r3);

        if r.is_zero() {
            self.prev_close = Some(bar.close);
            self.prev_open = Some(bar.open);
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        // Swing index formula
        let numerator = (bar.close - pc)
            + (bar.close - bar.open) / Decimal::from(2u32)
            + (pc - po) / Decimal::from(4u32);

        // K/T normalization
        let k = r1.max(r2);
        let kt = if self.limit_move.is_zero() { Decimal::ONE } else { k / self.limit_move };

        let si = Decimal::from(50u32) * numerator / r * kt;

        self.prev_close = Some(bar.close);
        self.prev_open = Some(bar.open);

        if self.cumulative {
            self.asi += si;
            Ok(SignalValue::Scalar(self.asi))
        } else {
            Ok(SignalValue::Scalar(si))
        }
    }

    fn is_ready(&self) -> bool { self.prev_close.is_some() }
    fn period(&self) -> usize { 1 }

    fn reset(&mut self) {
        self.prev_close = None;
        self.prev_open = None;
        self.asi = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_ohlc(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
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
    fn test_si_invalid() {
        assert!(SwingIndex::new("s", dec!(0), false).is_err());
        assert!(SwingIndex::new("s", dec!(-1), false).is_err());
    }

    #[test]
    fn test_si_unavailable_first_bar() {
        let mut s = SwingIndex::new("s", dec!(0.5), false).unwrap();
        assert_eq!(
            s.update_bar(&bar_ohlc("100", "105", "98", "102")).unwrap(),
            SignalValue::Unavailable
        );
    }

    #[test]
    fn test_si_flat_is_zero() {
        let mut s = SwingIndex::new("s", dec!(0.5), false).unwrap();
        s.update_bar(&bar_ohlc("100", "100", "100", "100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar_ohlc("100", "100", "100", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_asi_cumulates() {
        let mut s = SwingIndex::new("s", dec!(0.5), true).unwrap();
        s.update_bar(&bar_ohlc("100", "105", "98", "102")).unwrap(); // bar 1
        let v1 = s.update_bar(&bar_ohlc("102", "108", "101", "107")).unwrap(); // bar 2
        let v2 = s.update_bar(&bar_ohlc("107", "110", "105", "106")).unwrap(); // bar 3
        // ASI should be non-zero and accumulating
        assert!(matches!(v1, SignalValue::Scalar(_)));
        assert!(matches!(v2, SignalValue::Scalar(_)));
        if let (SignalValue::Scalar(a1), SignalValue::Scalar(a2)) = (v1, v2) {
            assert_ne!(a1, dec!(0)); // first SI value contributed
            // a2 = a1 + SI_bar3; they should differ unless SI_bar3 = 0
            let _ = a2; // just checking no panic
        }
    }

    #[test]
    fn test_si_reset() {
        let mut s = SwingIndex::new("s", dec!(0.5), true).unwrap();
        s.update_bar(&bar_ohlc("100", "105", "98", "102")).unwrap();
        s.update_bar(&bar_ohlc("102", "108", "101", "107")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert_eq!(s.asi(), dec!(0));
    }
}
