//! Price Compression Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Compression Index — ratio of the standard deviation of closes to the
/// average true range over `period` bars.
///
/// ```text
/// PCI = StdDev(close, n) / ATR(n)
/// ```
///
/// A value near 0 indicates price is tightly compressed relative to its
/// intrabar volatility (consolidation). A high value indicates close values are
/// more dispersed than intrabar moves (trending).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceCompressionIndex;
/// use fin_primitives::signals::Signal;
///
/// let pci = PriceCompressionIndex::new("pci", 14).unwrap();
/// assert_eq!(pci.period(), 14);
/// ```
pub struct PriceCompressionIndex {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    closes: VecDeque<Decimal>,
    trs: VecDeque<Decimal>,
    tr_sum: Decimal,
}

impl PriceCompressionIndex {
    /// Constructs a new `PriceCompressionIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            closes: VecDeque::with_capacity(period),
            trs: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceCompressionIndex {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => bar.range(),
            Some(pc) => {
                let hl = bar.range();
                let hpc = (bar.high - pc).abs();
                let lpc = (bar.low - pc).abs();
                hl.max(hpc).max(lpc)
            }
        };
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        self.tr_sum += tr;
        if self.trs.len() > self.period {
            self.tr_sum -= self.trs.pop_front().unwrap();
        }

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }

        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let nd = Decimal::from(self.period as u32);
        let atr = self.tr_sum / nd;
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.closes.iter().filter_map(|c| c.to_f64()).collect();
        if vals.len() != self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / vals.len() as f64;
        let std_dev = var.sqrt();

        let atr_f = match atr.to_f64() {
            Some(f) if f != 0.0 => f,
            _ => return Ok(SignalValue::Unavailable),
        };

        match Decimal::from_f64(std_dev / atr_f) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.closes.clear();
        self.trs.clear();
        self.tr_sum = Decimal::ZERO;
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
    fn test_pci_invalid_period() {
        assert!(PriceCompressionIndex::new("pci", 0).is_err());
        assert!(PriceCompressionIndex::new("pci", 1).is_err());
    }

    #[test]
    fn test_pci_unavailable_before_warm_up() {
        let mut pci = PriceCompressionIndex::new("pci", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(pci.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pci_constant_close_gives_zero() {
        // All closes the same → std dev = 0 → PCI = 0
        let mut pci = PriceCompressionIndex::new("pci", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = pci.update_bar(&bar("110", "90", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0.001), "constant close should give PCI ≈ 0: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pci_trending_close_positive() {
        let mut pci = PriceCompressionIndex::new("pci", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        // Large close range, moderate ATR
        for c in ["90", "100", "110"] {
            last = pci.update_bar(&bar(&(c.parse::<u32>().unwrap() + 5).to_string(), c, c)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "dispersed closes should give positive PCI: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pci_reset() {
        let mut pci = PriceCompressionIndex::new("pci", 3).unwrap();
        for _ in 0..3 { pci.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(pci.is_ready());
        pci.reset();
        assert!(!pci.is_ready());
    }
}
