//! Classic Pivot Points indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Classic Pivot Points — uses the prior bar's H/L/C to compute pivot, supports, and resistances.
///
/// ```text
/// P  = (H + L + C) / 3
/// R1 = 2×P − L
/// S1 = 2×P − H
/// R2 = P + (H − L)
/// S2 = P − (H − L)
/// ```
///
/// Returns `P` (the pivot point) as the scalar signal value.
/// Use [`Pivots::levels`] for the full (P, R1, S1, R2, S2) tuple.
///
/// Returns [`SignalValue::Unavailable`] until the first bar has been seen (needs 2 bars total).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Pivots;
/// use fin_primitives::signals::Signal;
///
/// let p = Pivots::new("piv").unwrap();
/// assert_eq!(p.period(), 1);
/// assert!(!p.is_ready());
/// ```
pub struct Pivots {
    name: String,
    prev: Option<BarInput>,
    levels: Option<(Decimal, Decimal, Decimal, Decimal, Decimal)>,
}

impl Pivots {
    /// Constructs a new `Pivots` indicator.
    ///
    /// # Errors
    /// Never errors — provided for API consistency with other indicators.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            prev: None,
            levels: None,
        })
    }

    /// Returns `(P, R1, S1, R2, S2)` pivot levels, or `None` if not ready.
    pub fn levels(&self) -> Option<(Decimal, Decimal, Decimal, Decimal, Decimal)> {
        self.levels
    }

    /// Returns the pivot point `P`, or `None` if not ready.
    pub fn pivot(&self) -> Option<Decimal> {
        self.levels.map(|(p, _, _, _, _)| p)
    }

    /// Returns resistance level R1, or `None` if not ready.
    pub fn r1(&self) -> Option<Decimal> {
        self.levels.map(|(_, r1, _, _, _)| r1)
    }

    /// Returns support level S1, or `None` if not ready.
    pub fn s1(&self) -> Option<Decimal> {
        self.levels.map(|(_, _, s1, _, _)| s1)
    }
}

impl Signal for Pivots {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(prev) = self.prev {
            let h = prev.high;
            let l = prev.low;
            let c = prev.close;
            let p  = (h + l + c) / Decimal::from(3u32);
            let r1 = Decimal::TWO * p - l;
            let s1 = Decimal::TWO * p - h;
            let r2 = p + (h - l);
            let s2 = p - (h - l);
            self.levels = Some((p, r1, s1, r2, s2));
            self.prev = Some(*bar);
            Ok(SignalValue::Scalar(p))
        } else {
            self.prev = Some(*bar);
            Ok(SignalValue::Unavailable)
        }
    }

    fn is_ready(&self) -> bool {
        self.levels.is_some()
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.prev = None;
        self.levels = None;
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
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pivots_unavailable_first_bar() {
        let mut p = Pivots::new("piv").unwrap();
        assert_eq!(p.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!p.is_ready());
    }

    #[test]
    fn test_pivots_correct_values() {
        let mut p = Pivots::new("piv").unwrap();
        p.update_bar(&bar("120", "80", "100")).unwrap();
        // P = (120+80+100)/3 = 100; R1=2*100-80=120; S1=2*100-120=80; R2=100+40=140; S2=100-40=60
        let v = p.update_bar(&bar("105", "95", "102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
        let (pivot, r1, s1, r2, s2) = p.levels().unwrap();
        assert_eq!(pivot, dec!(100));
        assert_eq!(r1, dec!(120));
        assert_eq!(s1, dec!(80));
        assert_eq!(r2, dec!(140));
        assert_eq!(s2, dec!(60));
    }

    #[test]
    fn test_pivots_accessors() {
        let mut p = Pivots::new("piv").unwrap();
        p.update_bar(&bar("120", "80", "100")).unwrap();
        p.update_bar(&bar("105", "95", "102")).unwrap();
        assert_eq!(p.pivot(), Some(dec!(100)));
        assert_eq!(p.r1(), Some(dec!(120)));
        assert_eq!(p.s1(), Some(dec!(80)));
    }

    #[test]
    fn test_pivots_reset() {
        let mut p = Pivots::new("piv").unwrap();
        p.update_bar(&bar("120", "80", "100")).unwrap();
        p.update_bar(&bar("105", "95", "102")).unwrap();
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
