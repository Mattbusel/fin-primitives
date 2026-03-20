//! True Range Z-Score — current true range normalized against rolling mean and std dev.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// True Range Z-Score — `(TR - mean_TR) / std_TR` over a rolling window.
///
/// Measures how unusual the current bar's volatility is relative to recent history:
/// - **Large positive**: abnormally wide bar — potential breakout or news event.
/// - **Large negative**: unusually narrow bar — compression, consolidation, or low activity.
/// - **Near zero**: volatility is in line with recent average.
///
/// Uses population standard deviation computed from the rolling window.
/// Returns [`SignalValue::Unavailable`] until `period` true ranges have been collected
/// or if the standard deviation is zero (constant TR series).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrueRangeZScore;
/// use fin_primitives::signals::Signal;
/// let trz = TrueRangeZScore::new("trz_14", 14).unwrap();
/// assert_eq!(trz.period(), 14);
/// ```
pub struct TrueRangeZScore {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
    sum_sq: Decimal,
    prev_close: Option<Decimal>,
}

impl TrueRangeZScore {
    /// Constructs a new `TrueRangeZScore`.
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
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
            sum_sq: Decimal::ZERO,
            prev_close: None,
        })
    }
}

impl Signal for TrueRangeZScore {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.sum += tr;
        self.sum_sq += tr * tr;
        self.window.push_back(tr);

        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
            self.sum_sq -= removed * removed;
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = Decimal::from(self.period as u32);
        let mean = self.sum
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;

        // population variance = E[x^2] - (E[x])^2
        let mean_sq = self.sum_sq
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;
        let variance = (mean_sq - mean * mean).max(Decimal::ZERO);

        // sqrt via f64
        let std_f64 = variance.to_f64().unwrap_or(0.0).sqrt();
        if std_f64 <= 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let tr_f64 = tr.to_f64().unwrap_or(0.0);
        let mean_f64 = mean.to_f64().unwrap_or(0.0);
        let z = (tr_f64 - mean_f64) / std_f64;

        Ok(SignalValue::Scalar(
            Decimal::try_from(z).unwrap_or(Decimal::ZERO),
        ))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
        self.sum_sq = Decimal::ZERO;
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
    fn test_trz_invalid_period() {
        assert!(TrueRangeZScore::new("trz", 0).is_err());
        assert!(TrueRangeZScore::new("trz", 1).is_err());
    }

    #[test]
    fn test_trz_unavailable_during_warmup() {
        let mut s = TrueRangeZScore::new("trz", 3).unwrap();
        assert_eq!(s.update_bar(&bar("105","95","100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("106","94","100")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_trz_large_bar_gives_positive_z() {
        // 3 bars with range ~10, then a spike bar with range ~50 → z >> 0
        let mut s = TrueRangeZScore::new("trz", 3).unwrap();
        s.update_bar(&bar("105","95","100")).unwrap();  // TR=10
        s.update_bar(&bar("106","96","101")).unwrap();  // TR=10
        s.update_bar(&bar("107","97","102")).unwrap();  // TR=10 → mean=10, std≈0
        // std=0 → Unavailable for uniform TR; use varying TRs for the window
        let mut s2 = TrueRangeZScore::new("trz", 3).unwrap();
        s2.update_bar(&bar("105","95","100")).unwrap();  // TR=10
        s2.update_bar(&bar("108","94","101")).unwrap();  // TR=14
        s2.update_bar(&bar("106","98","102")).unwrap();  // TR=8
        // Now a large spike bar
        if let SignalValue::Scalar(z) = s2.update_bar(&bar("130","80","100")).unwrap() {
            // TR=50, mean of window=[14,8,50]=24, big z expected
            assert!(z > dec!(0), "large spike bar gives positive z: {z}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_trz_small_bar_gives_negative_z() {
        // 3 bars with range ~20, then a tiny bar → z < 0
        let mut s = TrueRangeZScore::new("trz", 3).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();   // TR=20
        s.update_bar(&bar("115","85","100")).unwrap();   // TR=30
        if let SignalValue::Scalar(z) = s.update_bar(&bar("102","98","100")).unwrap() {
            // TR=4, mean of [20,30,4]≈18, z < 0
            assert!(z < dec!(0), "tiny bar gives negative z: {z}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_trz_reset() {
        let mut s = TrueRangeZScore::new("trz", 3).unwrap();
        for (h, l, c) in &[("110","90","100"),("115","85","105"),("112","92","108")] {
            s.update_bar(&bar(h, l, c)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
