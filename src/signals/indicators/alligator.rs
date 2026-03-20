//! Williams Alligator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Williams Alligator — three smoothed moving averages (SMMA) with offset bars.
///
/// * **Jaw**   (blue)  — SMMA(13), shifted 8 bars forward
/// * **Teeth** (red)   — SMMA(8),  shifted 5 bars forward
/// * **Lips**  (green) — SMMA(5),  shifted 3 bars forward
///
/// The `update()` call fills the current bar's unshifted SMMA values and
/// returns the **Lips** SMMA value as [`SignalValue::Scalar`].  Use the
/// `jaw()`, `teeth()`, and `lips()` accessors for the current values.
///
/// Returns [`SignalValue::Unavailable`] until the Jaw (slowest, 13 bars) has seeded.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Alligator;
/// use fin_primitives::signals::Signal;
///
/// let al = Alligator::new("alligator").unwrap();
/// assert_eq!(al.period(), 13);
/// ```
pub struct Alligator {
    name: String,
    // Jaw: SMMA(13)
    jaw_seed: VecDeque<Decimal>,
    jaw: Option<Decimal>,
    jaw_alpha: Decimal,
    // Teeth: SMMA(8)
    teeth_seed: VecDeque<Decimal>,
    teeth: Option<Decimal>,
    teeth_alpha: Decimal,
    // Lips: SMMA(5)
    lips_seed: VecDeque<Decimal>,
    lips: Option<Decimal>,
    lips_alpha: Decimal,
}

impl Alligator {
    /// Creates a new `Alligator` with standard parameters (Jaw=13, Teeth=8, Lips=5).
    ///
    /// # Errors
    /// Never fails; returns `Result` for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            jaw_seed: VecDeque::with_capacity(13),
            jaw: None,
            jaw_alpha: Decimal::ONE / Decimal::from(13u32),
            teeth_seed: VecDeque::with_capacity(8),
            teeth: None,
            teeth_alpha: Decimal::ONE / Decimal::from(8u32),
            lips_seed: VecDeque::with_capacity(5),
            lips: None,
            lips_alpha: Decimal::ONE / Decimal::from(5u32),
        })
    }

    fn smma_step(
        seed: &mut VecDeque<Decimal>,
        current: &mut Option<Decimal>,
        alpha: Decimal,
        close: Decimal,
        period: usize,
    ) -> Option<Decimal> {
        match *current {
            None => {
                seed.push_back(close);
                if seed.len() < period { return None; }
                let avg = seed.iter().sum::<Decimal>() / Decimal::from(period as u32);
                *current = Some(avg);
                Some(avg)
            }
            Some(prev) => {
                let v = prev * (Decimal::ONE - alpha) + close * alpha;
                *current = Some(v);
                Some(v)
            }
        }
    }

    /// Returns the current Jaw (SMMA-13) value.
    pub fn jaw(&self) -> Option<Decimal> { self.jaw }
    /// Returns the current Teeth (SMMA-8) value.
    pub fn teeth(&self) -> Option<Decimal> { self.teeth }
    /// Returns the current Lips (SMMA-5) value.
    pub fn lips(&self) -> Option<Decimal> { self.lips }
}

impl Signal for Alligator {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Use median price (H+L+C)/3 as input
        let price = bar.typical_price();

        Self::smma_step(&mut self.lips_seed, &mut self.lips, self.lips_alpha, price, 5);
        Self::smma_step(&mut self.teeth_seed, &mut self.teeth, self.teeth_alpha, price, 8);
        let jaw = Self::smma_step(&mut self.jaw_seed, &mut self.jaw, self.jaw_alpha, price, 13);

        match jaw {
            None => Ok(SignalValue::Unavailable),
            Some(_) => Ok(SignalValue::Scalar(self.lips.unwrap_or(price))),
        }
    }

    fn is_ready(&self) -> bool {
        self.jaw.is_some()
    }

    fn period(&self) -> usize {
        13
    }

    fn reset(&mut self) {
        self.jaw_seed.clear();
        self.jaw = None;
        self.teeth_seed.clear();
        self.teeth = None;
        self.lips_seed.clear();
        self.lips = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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
    fn test_alligator_period() {
        let al = Alligator::new("a").unwrap();
        assert_eq!(al.period(), 13);
    }

    #[test]
    fn test_alligator_unavailable_before_jaw_seeded() {
        let mut al = Alligator::new("a").unwrap();
        for _ in 0..12 {
            assert_eq!(al.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!al.is_ready());
    }

    #[test]
    fn test_alligator_scalar_after_warmup() {
        let mut al = Alligator::new("a").unwrap();
        for _ in 0..13 { al.update_bar(&bar("100")).unwrap(); }
        let v = al.update_bar(&bar("100")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(al.is_ready());
    }

    #[test]
    fn test_alligator_all_lines_available_after_warmup() {
        let mut al = Alligator::new("a").unwrap();
        for _ in 0..15 { al.update_bar(&bar("100")).unwrap(); }
        assert!(al.jaw().is_some());
        assert!(al.teeth().is_some());
        assert!(al.lips().is_some());
    }

    #[test]
    fn test_alligator_reset() {
        let mut al = Alligator::new("a").unwrap();
        for _ in 0..15 { al.update_bar(&bar("100")).unwrap(); }
        assert!(al.is_ready());
        al.reset();
        assert!(!al.is_ready());
        assert!(al.jaw().is_none());
    }
}
