//! KST (Know Sure Thing) oscillator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// KST — smoothed sum of four rate-of-change values weighted by period length.
///
/// ```text
/// RCMA1 = SMA(r1, ROC(roc1))
/// RCMA2 = SMA(r2, ROC(roc2))
/// RCMA3 = SMA(r3, ROC(roc3))
/// RCMA4 = SMA(r4, ROC(roc4))
/// KST   = RCMA1×1 + RCMA2×2 + RCMA3×3 + RCMA4×4
/// ```
///
/// Daily defaults: roc=(10,13,14,15), sma=(10,13,14,15), weights=(1,2,3,4).
/// Returns [`SignalValue::Unavailable`] until fully warmed up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Kst;
/// use fin_primitives::signals::Signal;
///
/// let k = Kst::new_daily("kst").unwrap();
/// assert_eq!(k.period(), 15);
/// assert!(!k.is_ready());
/// ```
pub struct Kst {
    name: String,
    roc_periods: [usize; 4],
    sma_periods: [usize; 4],
    closes: VecDeque<Decimal>,
    sma_bufs: [VecDeque<Decimal>; 4],
    ready: bool,
}

impl Kst {
    /// Constructs a `Kst` with default daily parameters.
    ///
    /// # Errors
    /// Never errors — provided for consistency.
    pub fn new_daily(name: impl Into<String>) -> Result<Self, FinError> {
        Self::new(name, [10, 13, 14, 15], [10, 13, 14, 15])
    }

    /// Constructs a `Kst` with custom ROC and SMA periods.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero.
    pub fn new(
        name: impl Into<String>,
        roc_periods: [usize; 4],
        sma_periods: [usize; 4],
    ) -> Result<Self, FinError> {
        for &p in roc_periods.iter().chain(sma_periods.iter()) {
            if p == 0 { return Err(FinError::InvalidPeriod(p)); }
        }
        let max_hist = roc_periods.iter().max().copied().unwrap_or(0);
        Ok(Self {
            name: name.into(),
            roc_periods,
            sma_periods,
            closes: VecDeque::with_capacity(max_hist + 1),
            sma_bufs: [
                VecDeque::with_capacity(sma_periods[0]),
                VecDeque::with_capacity(sma_periods[1]),
                VecDeque::with_capacity(sma_periods[2]),
                VecDeque::with_capacity(sma_periods[3]),
            ],
            ready: false,
        })
    }
}

impl Signal for Kst {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let max_roc = *self.roc_periods.iter().max().unwrap();
        self.closes.push_back(bar.close);
        if self.closes.len() > max_roc + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < max_roc + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let mut all_ready = true;
        let mut kst = Decimal::ZERO;
        for i in 0..4 {
            let rp = self.roc_periods[i];
            let sp = self.sma_periods[i];
            let len = self.closes.len();
            let current = self.closes[len - 1];
            let prev = self.closes[len - 1 - rp.min(len - 1)];
            let roc = if prev.is_zero() {
                Decimal::ZERO
            } else {
                (current - prev) / prev * Decimal::ONE_HUNDRED
            };
            self.sma_bufs[i].push_back(roc);
            if self.sma_bufs[i].len() > sp {
                self.sma_bufs[i].pop_front();
            }
            if self.sma_bufs[i].len() < sp {
                all_ready = false;
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let sma = self.sma_bufs[i].iter().copied().sum::<Decimal>()
                / Decimal::from(sp as u32);
            #[allow(clippy::cast_possible_truncation)]
            let weight = Decimal::from((i + 1) as u32);
            kst += sma * weight;
        }

        if !all_ready {
            return Ok(SignalValue::Unavailable);
        }
        self.ready = true;
        Ok(SignalValue::Scalar(kst))
    }

    fn is_ready(&self) -> bool { self.ready }

    fn period(&self) -> usize { *self.roc_periods.iter().max().unwrap_or(&0) }

    fn reset(&mut self) {
        self.closes.clear();
        for buf in &mut self.sma_bufs { buf.clear(); }
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
    fn test_kst_invalid_period() {
        assert!(Kst::new("k", [0, 13, 14, 15], [10, 13, 14, 15]).is_err());
    }

    #[test]
    fn test_kst_unavailable_early() {
        let mut k = Kst::new("k", [3, 4, 5, 6], [2, 2, 2, 2]).unwrap();
        for _ in 0..5 {
            assert_eq!(k.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_kst_ready_after_warmup() {
        let mut k = Kst::new("k", [3, 4, 5, 6], [2, 2, 2, 2]).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = k.update_bar(&bar("100")).unwrap();
        }
        assert!(k.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_kst_flat_market_near_zero() {
        let mut k = Kst::new("k", [3, 4, 5, 6], [2, 2, 2, 2]).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..30 { last = k.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < dec!(1), "KST flat should be near 0: {v}");
        } else { panic!("expected scalar"); }
    }

    #[test]
    fn test_kst_reset() {
        let mut k = Kst::new("k", [3, 4, 5, 6], [2, 2, 2, 2]).unwrap();
        for _ in 0..20 { k.update_bar(&bar("100")).unwrap(); }
        assert!(k.is_ready());
        k.reset();
        assert!(!k.is_ready());
    }

    #[test]
    fn test_kst_daily_period() {
        let k = Kst::new_daily("k").unwrap();
        assert_eq!(k.period(), 15);
    }
}
