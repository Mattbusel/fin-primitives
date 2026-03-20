//! Camarilla Pivot Points indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Camarilla Pivot Points — uses the prior bar's H/L/C to compute eight intraday levels.
///
/// Camarilla equations use a fixed multiplier `11/12` (`≈ 0.9167`) applied to the
/// prior bar's range (`H - L`) to generate four resistance and four support levels:
///
/// ```text
/// R4 = C + (H - L) × 1.1 / 2
/// R3 = C + (H - L) × 1.1 / 4
/// R2 = C + (H - L) × 1.1 / 6
/// R1 = C + (H - L) × 1.1 / 12
/// S1 = C - (H - L) × 1.1 / 12
/// S2 = C - (H - L) × 1.1 / 6
/// S3 = C - (H - L) × 1.1 / 4
/// S4 = C - (H - L) × 1.1 / 2
/// ```
///
/// The scalar output is `R3`, the most widely watched Camarilla resistance for
/// intraday breakout setups. Use [`CamarillaP::levels`] for all eight levels.
///
/// Returns [`SignalValue::Unavailable`] until two bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CamarillaP;
/// use fin_primitives::signals::Signal;
///
/// let c = CamarillaP::new("cam").unwrap();
/// assert_eq!(c.period(), 1);
/// assert!(!c.is_ready());
/// ```
pub struct CamarillaP {
    name: String,
    prev: Option<BarInput>,
    /// Stored as (R1, R2, R3, R4, S1, S2, S3, S4).
    levels: Option<(Decimal, Decimal, Decimal, Decimal, Decimal, Decimal, Decimal, Decimal)>,
}

impl CamarillaP {
    /// Constructs a new `CamarillaP` indicator.
    ///
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev: None, levels: None })
    }

    /// Returns all eight Camarilla levels as `(R1, R2, R3, R4, S1, S2, S3, S4)`,
    /// or `None` if not yet ready.
    pub fn levels(&self) -> Option<(Decimal, Decimal, Decimal, Decimal, Decimal, Decimal, Decimal, Decimal)> {
        self.levels
    }

    /// Returns resistance level R3, or `None` if not ready.
    pub fn r3(&self) -> Option<Decimal> {
        self.levels.map(|(_, _, r3, _, _, _, _, _)| r3)
    }

    /// Returns support level S3, or `None` if not ready.
    pub fn s3(&self) -> Option<Decimal> {
        self.levels.map(|(_, _, _, _, _, _, s3, _)| s3)
    }

    /// Returns resistance level R4 (breakout level), or `None` if not ready.
    pub fn r4(&self) -> Option<Decimal> {
        self.levels.map(|(_, _, _, r4, _, _, _, _)| r4)
    }

    /// Returns support level S4 (breakout level), or `None` if not ready.
    pub fn s4(&self) -> Option<Decimal> {
        self.levels.map(|(_, _, _, _, _, _, _, s4)| s4)
    }
}

impl Signal for CamarillaP {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(prev) = self.prev {
            let h = prev.high;
            let l = prev.low;
            let c = prev.close;
            let range = h - l;
            // Multiplier 1.1 applied fractionally
            let mult = Decimal::new(11, 1); // 1.1
            let d = range * mult;
            let r1 = c + d / Decimal::from(12u32);
            let r2 = c + d / Decimal::from(6u32);
            let r3 = c + d / Decimal::from(4u32);
            let r4 = c + d / Decimal::TWO;
            let s1 = c - d / Decimal::from(12u32);
            let s2 = c - d / Decimal::from(6u32);
            let s3 = c - d / Decimal::from(4u32);
            let s4 = c - d / Decimal::TWO;
            self.levels = Some((r1, r2, r3, r4, s1, s2, s3, s4));
            self.prev = Some(*bar);
            Ok(SignalValue::Scalar(r3))
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
        self.prev   = None;
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
    fn test_camarilla_unavailable_first_bar() {
        let mut c = CamarillaP::new("cam").unwrap();
        assert_eq!(c.update_bar(&bar("120", "80", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!c.is_ready());
    }

    #[test]
    fn test_camarilla_levels_computed() {
        let mut c = CamarillaP::new("cam").unwrap();
        // H=120, L=80, C=100 → range=40, d=44
        c.update_bar(&bar("120", "80", "100")).unwrap();
        let v = c.update_bar(&bar("115", "90", "105")).unwrap();
        // R3 = 100 + 44/4 = 100 + 11 = 111
        assert!(matches!(v, SignalValue::Scalar(_)));
        let r3 = c.r3().unwrap();
        assert_eq!(r3, dec!(111));
        // S3 = 100 - 11 = 89
        assert_eq!(c.s3().unwrap(), dec!(89));
        // R4 = 100 + 22 = 122
        assert_eq!(c.r4().unwrap(), dec!(122));
        // S4 = 100 - 22 = 78
        assert_eq!(c.s4().unwrap(), dec!(78));
    }

    #[test]
    fn test_camarilla_r1_r2_symmetry() {
        let mut c = CamarillaP::new("cam").unwrap();
        c.update_bar(&bar("110", "90", "100")).unwrap();
        c.update_bar(&bar("110", "90", "100")).unwrap();
        let (r1, r2, r3, r4, s1, s2, s3, s4) = c.levels().unwrap();
        // range=20, d=22
        // R1=100+22/12, S1=100-22/12  → symmetric
        assert_eq!(r1 + s1, dec!(200));
        assert_eq!(r2 + s2, dec!(200));
        assert_eq!(r3 + s3, dec!(200));
        assert_eq!(r4 + s4, dec!(200));
    }

    #[test]
    fn test_camarilla_reset() {
        let mut c = CamarillaP::new("cam").unwrap();
        c.update_bar(&bar("120", "80", "100")).unwrap();
        c.update_bar(&bar("115", "85", "102")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
        assert!(c.levels().is_none());
    }
}
