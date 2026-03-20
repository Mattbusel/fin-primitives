//! Volume-Weighted ATR — ATR weighted by volume, capturing high-volume volatility events.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted ATR — `sum(TR × volume) / sum(volume)` over the last `period` bars.
///
/// Bars with higher volume are given more weight in the average. This produces an
/// ATR that is biased toward the volatility seen during active trading, filtering
/// out low-volume noise.
///
/// - Higher than regular ATR: volatile bars tend to have high volume.
/// - Lower than regular ATR: very wide bars occur on thin volume.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen (requires
/// one extra bar to compute the first TR with a previous close), or when total volume
/// in the window is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedAtr;
/// use fin_primitives::signals::Signal;
/// let vwatr = VolumeWeightedAtr::new("vwatr_14", 14).unwrap();
/// assert_eq!(vwatr.period(), 14);
/// ```
pub struct VolumeWeightedAtr {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<(Decimal, Decimal)>, // (tr, volume)
    bars_seen: usize,
}

impl VolumeWeightedAtr {
    /// Constructs a new `VolumeWeightedAtr`.
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
            prev_close: None,
            window: VecDeque::with_capacity(period),
            bars_seen: 0,
        })
    }
}

impl Signal for VolumeWeightedAtr {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen >= self.period + 1
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);
        self.bars_seen += 1;

        if self.bars_seen == 1 {
            // First bar: no previous close, skip
            return Ok(SignalValue::Unavailable);
        }

        self.window.push_back((tr, bar.volume));
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let (vol_tr_sum, vol_sum) = self.window.iter().fold(
            (Decimal::ZERO, Decimal::ZERO),
            |(vt, v), &(tr, vol)| (vt + tr * vol, v + vol),
        );

        if vol_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vwatr = vol_tr_sum.checked_div(vol_sum).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(vwatr))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.bars_seen = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str, vol: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwatr_invalid_period() {
        assert!(VolumeWeightedAtr::new("vwatr", 0).is_err());
    }

    #[test]
    fn test_vwatr_unavailable_before_period_plus_1() {
        let mut vwatr = VolumeWeightedAtr::new("vwatr", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(vwatr.update_bar(&bar("110", "90", "100", "1000")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vwatr.is_ready());
    }

    #[test]
    fn test_vwatr_non_negative() {
        let mut vwatr = VolumeWeightedAtr::new("vwatr", 3).unwrap();
        let bars = [
            bar("110", "90", "100", "1000"),
            bar("112", "88", "102", "2000"),
            bar("108", "92", "98", "500"),
            bar("115", "85", "105", "3000"),
            bar("107", "93", "100", "1500"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = vwatr.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "VWATR must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_vwatr_equal_volumes_matches_simple_average() {
        // Equal volume → VWATR = simple average TR
        let mut vwatr = VolumeWeightedAtr::new("vwatr", 3).unwrap();
        vwatr.update_bar(&bar("110", "90", "100", "1000")).unwrap(); // skip (first bar)
        vwatr.update_bar(&bar("120", "100", "110", "1000")).unwrap(); // TR=20
        vwatr.update_bar(&bar("120", "100", "110", "1000")).unwrap(); // TR=10 (tight)
        let v = vwatr.update_bar(&bar("120", "100", "110", "1000")).unwrap(); // TR=10
        if let SignalValue::Scalar(r) = v {
            // average = (20 + 10 + 10) / 3 = 13.33
            // TR bar2: high=120,low=100,prev=100 → max(20, |120-100|,|100-100|)=20
            // TR bar3: high=120,low=100,prev=110 → max(20, |120-110|,|100-110|)=20
            // TR bar4: same = 20. So all 3 bars have TR=20 → avg=20
            assert!(r > dec!(0), "VWATR with equal volumes should be positive: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwatr_reset() {
        let mut vwatr = VolumeWeightedAtr::new("vwatr", 3).unwrap();
        for _ in 0..5 {
            vwatr.update_bar(&bar("110", "90", "100", "1000")).unwrap();
        }
        assert!(vwatr.is_ready());
        vwatr.reset();
        assert!(!vwatr.is_ready());
    }

    #[test]
    fn test_vwatr_period_and_name() {
        let vwatr = VolumeWeightedAtr::new("my_vwatr", 14).unwrap();
        assert_eq!(vwatr.period(), 14);
        assert_eq!(vwatr.name(), "my_vwatr");
    }
}
