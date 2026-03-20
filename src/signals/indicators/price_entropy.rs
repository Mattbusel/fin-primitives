//! Price Entropy indicator -- Shannon entropy of return signs over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Entropy -- Shannon entropy of the sign of close-to-close returns over
/// a rolling `period`-bar window.
///
/// Classifies each return as up (+1), down (-1), or flat (0). Entropy measures
/// how unpredictable the market is:
///
/// - High entropy (~1.0): returns are random/unpredictable (up and down equally frequent)
/// - Low entropy (~0): returns are highly predictable (mostly one direction)
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceEntropy;
/// use fin_primitives::signals::Signal;
/// let pe = PriceEntropy::new("pe", 20).unwrap();
/// assert_eq!(pe.period(), 20);
/// ```
pub struct PriceEntropy {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceEntropy {
    /// Constructs a new `PriceEntropy`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 2),
        })
    }

    fn entropy(probs: &[f64]) -> f64 {
        probs.iter()
            .filter(|&&p| p > 0.0)
            .map(|&p| -p * p.ln())
            .sum::<f64>()
            / (probs.len() as f64).ln().max(1e-10)
    }
}

impl Signal for PriceEntropy {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.closes.len() <= self.period { return Ok(SignalValue::Unavailable); }

        let prices: Vec<Decimal> = self.closes.iter().copied().collect();
        let n = prices.len() - 1; // number of returns
        let mut up = 0usize;
        let mut down = 0usize;
        let mut flat = 0usize;
        for w in prices.windows(2) {
            let ret = w[1] - w[0];
            if ret > Decimal::ZERO { up += 1; }
            else if ret < Decimal::ZERO { down += 1; }
            else { flat += 1; }
        }
        let n_f = n as f64;
        let probs = [up as f64 / n_f, down as f64 / n_f, flat as f64 / n_f];
        let e = Self::entropy(&probs);
        match Decimal::try_from(e) {
            Ok(d) => Ok(SignalValue::Scalar(d)),
            Err(_) => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_pe_period_too_small() { assert!(PriceEntropy::new("pe", 1).is_err()); }

    #[test]
    fn test_pe_unavailable_before_warmup() {
        let mut pe = PriceEntropy::new("pe", 4).unwrap();
        for _ in 0..4 {
            assert_eq!(pe.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pe_all_same_direction_low_entropy() {
        // All up -> entropy near 0
        let mut pe = PriceEntropy::new("pe", 5).unwrap();
        for i in 0u32..7 {
            pe.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(e) = pe.update_bar(&bar("107")).unwrap() {
            assert!(e < dec!(0.3), "expected low entropy for unidirectional series, got {e}");
        }
    }

    #[test]
    fn test_pe_alternating_moderate_entropy() {
        // Alternating up/down -> 50% up, 50% down -> higher entropy
        let mut pe = PriceEntropy::new("pe", 6).unwrap();
        let prices = ["100","102","100","102","100","102","100","102"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = pe.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(e) = last {
            assert!(e > dec!(0), "expected positive entropy for mixed series, got {e}");
        }
    }

    #[test]
    fn test_pe_reset() {
        let mut pe = PriceEntropy::new("pe", 4).unwrap();
        for i in 0u32..7 { pe.update_bar(&bar(&(100+i).to_string())).unwrap(); }
        assert!(pe.is_ready());
        pe.reset();
        assert!(!pe.is_ready());
    }
}
