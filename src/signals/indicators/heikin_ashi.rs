//! Heikin-Ashi smoothed close indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Heikin-Ashi Smoothed Close — a smoothed price representation using the Heikin-Ashi formula.
///
/// The Heikin-Ashi close is computed as `(open + high + low + close) / 4`, which reduces
/// noise compared to the raw close price and helps visualize trends more clearly.
///
/// Additionally, the Heikin-Ashi open is tracked as
/// `(ha_open[prev] + ha_close[prev]) / 2`, which seeds after the first bar.
///
/// ```text
/// ha_close = (open + high + low + close) / 4
/// ha_open  = (ha_open[prev] + ha_close[prev]) / 2   (seeds to open on first bar)
/// ```
///
/// Returns the `ha_close` as the scalar signal value.
/// Returns [`SignalValue::Unavailable`] on the first bar (need prior state for ha_open).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HeikinAshi;
/// use fin_primitives::signals::Signal;
///
/// let h = HeikinAshi::new("ha").unwrap();
/// assert_eq!(h.period(), 1);
/// ```
pub struct HeikinAshi {
    name: String,
    ha_open_prev: Option<Decimal>,
    ha_close_prev: Option<Decimal>,
    ready: bool,
}

impl HeikinAshi {
    /// Constructs a new `HeikinAshi`.
    ///
    /// # Errors
    /// This constructor never fails; the `Result` type is used for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            ha_open_prev: None,
            ha_close_prev: None,
            ready: false,
        })
    }
}

impl Signal for HeikinAshi {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.ready }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let four = Decimal::from(4u32);
        let two = Decimal::TWO;
        let ha_close = (bar.open + bar.high + bar.low + bar.close) / four;

        let ha_open = match (self.ha_open_prev, self.ha_close_prev) {
            (Some(op), Some(cl)) => (op + cl) / two,
            _ => {
                // Seed: first bar — store state, return Unavailable
                self.ha_open_prev = Some(bar.open);
                self.ha_close_prev = Some(ha_close);
                return Ok(SignalValue::Unavailable);
            }
        };

        self.ha_open_prev = Some(ha_open);
        self.ha_close_prev = Some(ha_close);
        self.ready = true;

        Ok(SignalValue::Scalar(ha_close))
    }

    fn reset(&mut self) {
        self.ha_open_prev = None;
        self.ha_close_prev = None;
        self.ready = false;
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ha_first_bar_unavailable() {
        let mut h = HeikinAshi::new("ha").unwrap();
        assert_eq!(h.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!h.is_ready());
    }

    #[test]
    fn test_ha_second_bar_produces_scalar() {
        let mut h = HeikinAshi::new("ha").unwrap();
        h.update_bar(&bar("100", "110", "90", "105")).unwrap();
        let v = h.update_bar(&bar("105", "115", "95", "110")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(h.is_ready());
    }

    #[test]
    fn test_ha_close_formula() {
        let mut h = HeikinAshi::new("ha").unwrap();
        h.update_bar(&bar("100", "100", "100", "100")).unwrap();
        // ha_close = (100+100+100+100)/4 = 100
        if let SignalValue::Scalar(v) = h.update_bar(&bar("100", "100", "100", "100")).unwrap() {
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ha_period() {
        let h = HeikinAshi::new("ha").unwrap();
        assert_eq!(h.period(), 1);
    }

    #[test]
    fn test_ha_reset() {
        let mut h = HeikinAshi::new("ha").unwrap();
        h.update_bar(&bar("100", "110", "90", "105")).unwrap();
        h.update_bar(&bar("105", "115", "95", "110")).unwrap();
        assert!(h.is_ready());
        h.reset();
        assert!(!h.is_ready());
        assert_eq!(h.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
    }
}
