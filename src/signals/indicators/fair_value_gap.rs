//! Fair Value Gap (FVG) detector — identifies imbalance gaps between candles.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Fair Value Gap (FVG) — detects imbalance gaps and measures their size in ATR units.
///
/// A **bullish FVG** occurs when `low[t] > high[t-2]` (a gap-up leaving unfilled space).
/// A **bearish FVG** occurs when `high[t] < low[t-2]` (a gap-down leaving unfilled space).
///
/// Output is the gap size normalized by ATR:
/// - **Positive**: bullish FVG (gap-up imbalance).
/// - **Negative**: bearish FVG (gap-down imbalance).
/// - **Zero**: no Fair Value Gap on this bar.
///
/// ATR uses Wilder's smoothing with the given `atr_period`.
///
/// Returns [`SignalValue::Unavailable`] until `atr_period + 2` bars have been seen
/// (2 extra for the 3-bar lookback), or when ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::FairValueGap;
/// use fin_primitives::signals::Signal;
/// let fvg = FairValueGap::new("fvg_14", 14).unwrap();
/// assert_eq!(fvg.period(), 14);
/// ```
pub struct FairValueGap {
    name: String,
    atr_period: usize,
    atr: Option<Decimal>,
    prev_close: Option<Decimal>,
    bars_seen: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl FairValueGap {
    /// Constructs a new `FairValueGap`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `atr_period == 0`.
    pub fn new(name: impl Into<String>, atr_period: usize) -> Result<Self, FinError> {
        if atr_period == 0 {
            return Err(FinError::InvalidPeriod(atr_period));
        }
        Ok(Self {
            name: name.into(),
            atr_period,
            atr: None,
            prev_close: None,
            bars_seen: 0,
            highs: VecDeque::with_capacity(3),
            lows: VecDeque::with_capacity(3),
        })
    }
}

impl Signal for FairValueGap {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.atr_period
    }

    fn is_ready(&self) -> bool {
        self.bars_seen >= self.atr_period + 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);
        self.bars_seen += 1;

        let period_d = Decimal::from(self.atr_period as u32);
        self.atr = Some(match self.atr {
            None => tr,
            Some(prev) => (prev * (period_d - Decimal::ONE) + tr) / period_d,
        });

        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > 3 {
            self.highs.pop_front();
            self.lows.pop_front();
        }

        if self.bars_seen < self.atr_period + 2 {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.atr.unwrap();
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        // highs/lows[0]=t-2, [1]=t-1, [2]=t
        let high_t2 = self.highs[0];
        let low_t2 = self.lows[0];
        let high_t = self.highs[2];
        let low_t = self.lows[2];

        let gap_size = if low_t > high_t2 {
            // Bullish FVG: current low > bar-2 high
            let gap = low_t - high_t2;
            gap.checked_div(atr).ok_or(FinError::ArithmeticOverflow)?
        } else if high_t < low_t2 {
            // Bearish FVG: current high < bar-2 low
            let gap = high_t - low_t2; // negative
            gap.checked_div(atr).ok_or(FinError::ArithmeticOverflow)?
        } else {
            Decimal::ZERO
        };

        Ok(SignalValue::Scalar(gap_size))
    }

    fn reset(&mut self) {
        self.atr = None;
        self.prev_close = None;
        self.bars_seen = 0;
        self.highs.clear();
        self.lows.clear();
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
    fn test_fvg_invalid_period() {
        assert!(FairValueGap::new("fvg", 0).is_err());
    }

    #[test]
    fn test_fvg_unavailable_before_atr_period_plus_2() {
        let mut fvg = FairValueGap::new("fvg", 3).unwrap();
        // Need atr_period+2 = 5 bars
        for _ in 0..4 {
            assert_eq!(fvg.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!fvg.is_ready());
    }

    #[test]
    fn test_fvg_no_gap_gives_zero() {
        // Normal overlapping bars → no FVG
        let mut fvg = FairValueGap::new("fvg", 3).unwrap();
        for _ in 0..6 {
            fvg.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let v = fvg.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_fvg_bullish_gap_positive() {
        // bar t-2: high=100, then bar t: low=110 → bullish FVG = (110-100)/ATR > 0
        let mut fvg = FairValueGap::new("fvg", 3).unwrap();
        // Seed ATR with normal bars
        fvg.update_bar(&bar("110", "90", "100")).unwrap();
        fvg.update_bar(&bar("110", "90", "100")).unwrap();
        fvg.update_bar(&bar("110", "90", "100")).unwrap();
        // t-2: high=100, low=90
        fvg.update_bar(&bar("100", "90", "95")).unwrap();
        // t-1: normal
        fvg.update_bar(&bar("105", "95", "100")).unwrap();
        // t: low=115 > high[t-2]=100 → bullish FVG
        let v = fvg.update_bar(&bar("125", "115", "120")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "bullish FVG should be positive: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_fvg_bearish_gap_negative() {
        // bar t-2: low=100, then bar t: high=85 → bearish FVG = (85-100)/ATR < 0
        let mut fvg = FairValueGap::new("fvg", 3).unwrap();
        fvg.update_bar(&bar("110", "90", "100")).unwrap();
        fvg.update_bar(&bar("110", "90", "100")).unwrap();
        fvg.update_bar(&bar("110", "90", "100")).unwrap();
        // t-2: high=110, low=100
        fvg.update_bar(&bar("110", "100", "105")).unwrap();
        // t-1: normal
        fvg.update_bar(&bar("105", "95", "100")).unwrap();
        // t: high=85 < low[t-2]=100 → bearish FVG
        let v = fvg.update_bar(&bar("85", "75", "80")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(0), "bearish FVG should be negative: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_fvg_reset() {
        let mut fvg = FairValueGap::new("fvg", 3).unwrap();
        for _ in 0..7 {
            fvg.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(fvg.is_ready());
        fvg.reset();
        assert!(!fvg.is_ready());
    }

    #[test]
    fn test_fvg_period_and_name() {
        let fvg = FairValueGap::new("my_fvg", 14).unwrap();
        assert_eq!(fvg.period(), 14);
        assert_eq!(fvg.name(), "my_fvg");
    }
}
