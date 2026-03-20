//! Volume Price Efficiency — N-bar price return per unit of cumulative volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Price Efficiency — `|close_now - close_N_ago| / sum_volume_N`.
///
/// Measures how much price movement is achieved per unit of aggregate volume:
/// - **High values**: large price moves with little volume — efficient or thin market.
/// - **Low values**: volume is high but price barely moves — contested, choppy market.
/// - **Near zero**: price is range-bound despite heavy trading.
///
/// Uses absolute price change (not signed) so this always measures the magnitude of
/// price movement per unit volume, regardless of direction.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated
/// or if cumulative volume is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePriceEfficiency;
/// use fin_primitives::signals::Signal;
/// let vpe = VolumePriceEfficiency::new("vpe_10", 10).unwrap();
/// assert_eq!(vpe.period(), 10);
/// ```
pub struct VolumePriceEfficiency {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
    vol_sum: Decimal,
}

impl VolumePriceEfficiency {
    /// Constructs a new `VolumePriceEfficiency`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
            volumes: VecDeque::with_capacity(period),
            vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumePriceEfficiency {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.volumes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume.to_decimal();

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        self.vol_sum += vol;
        self.volumes.push_back(vol);
        if self.volumes.len() > self.period {
            let removed = self.volumes.pop_front().unwrap();
            self.vol_sum -= removed;
        }

        if self.volumes.len() < self.period || self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.vol_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let close_now = *self.closes.back().unwrap();
        let close_n_ago = *self.closes.front().unwrap();
        let price_change = (close_now - close_n_ago).abs();

        let efficiency = price_change
            .checked_div(self.vol_sum)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(efficiency))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.volumes.clear();
        self.vol_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: u64) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(Decimal::from(vol)).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vpe_invalid_period() {
        assert!(VolumePriceEfficiency::new("vpe", 0).is_err());
    }

    #[test]
    fn test_vpe_unavailable_during_warmup() {
        let mut s = VolumePriceEfficiency::new("vpe", 3).unwrap();
        for (c, v) in &[("100",1000u64),("101",1000),("102",1000)] {
            assert_eq!(s.update_bar(&bar(c, *v)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_vpe_non_negative() {
        let mut s = VolumePriceEfficiency::new("vpe", 3).unwrap();
        let bars = [("100",1000u64),("102",1000),("104",1000),("103",1000),("105",1000)];
        for &(c, v) in &bars {
            if let SignalValue::Scalar(val) = s.update_bar(&bar(c, v)).unwrap() {
                assert!(val >= dec!(0), "VPE must be non-negative: {val}");
            }
        }
    }

    #[test]
    fn test_vpe_high_move_low_vol_is_efficient() {
        // Same price move but different volumes → lower vol gives higher efficiency
        let mut s_low_vol = VolumePriceEfficiency::new("vpe", 2).unwrap();
        let mut s_high_vol = VolumePriceEfficiency::new("vpe", 2).unwrap();

        // Both move 10 points over 2 bars
        s_low_vol.update_bar(&bar("100", 100)).unwrap();
        s_low_vol.update_bar(&bar("105", 100)).unwrap();
        let low_result = s_low_vol.update_bar(&bar("110", 100)).unwrap();

        s_high_vol.update_bar(&bar("100", 10000)).unwrap();
        s_high_vol.update_bar(&bar("105", 10000)).unwrap();
        let high_result = s_high_vol.update_bar(&bar("110", 10000)).unwrap();

        if let (SignalValue::Scalar(low_eff), SignalValue::Scalar(high_eff)) = (low_result, high_result) {
            assert!(low_eff > high_eff, "low volume with same move → higher efficiency: {low_eff} vs {high_eff}");
        } else {
            panic!("expected Scalar values");
        }
    }

    #[test]
    fn test_vpe_reset() {
        let mut s = VolumePriceEfficiency::new("vpe", 3).unwrap();
        for &(c, v) in &[("100",1000u64),("102",1000),("104",1000),("106",1000)] {
            s.update_bar(&bar(c, v)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
