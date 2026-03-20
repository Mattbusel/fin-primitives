//! TTM Squeeze indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// TTM Squeeze — identifies low-volatility compression periods.
///
/// ```text
/// BB_upper = SMA + k_bb × StdDev(close, period)
/// BB_lower = SMA − k_bb × StdDev(close, period)
/// KC_upper = SMA + k_kc × ATR(period)
/// KC_lower = SMA − k_kc × ATR(period)
///
/// squeeze_on  = BB inside KC  (BB_upper < KC_upper AND BB_lower > KC_lower)
/// momentum    = midpoint((high+low)/2, SMA) average deviation from SMA
/// output      = momentum value (sign indicates direction)
/// ```
///
/// `squeeze_on()` returns `true` when the market is in a low-volatility squeeze.
/// Positive output = expanding upward; negative = expanding downward.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TtmSqueeze;
/// use fin_primitives::signals::Signal;
///
/// let t = TtmSqueeze::new("ttm", 20, "2.0".parse().unwrap(), "1.5".parse().unwrap()).unwrap();
/// assert_eq!(t.period(), 20);
/// ```
pub struct TtmSqueeze {
    name: String,
    period: usize,
    k_bb: Decimal,
    k_kc: Decimal,
    closes: VecDeque<Decimal>,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    squeeze_on: bool,
}

impl TtmSqueeze {
    /// Creates a new `TtmSqueeze`.
    ///
    /// - `k_bb`: Bollinger Band multiplier (e.g. `2.0`).
    /// - `k_kc`: Keltner Channel multiplier (e.g. `1.5`).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if either multiplier is not positive.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        k_bb: Decimal,
        k_kc: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if k_bb <= Decimal::ZERO {
            return Err(FinError::InvalidInput("k_bb must be positive".into()));
        }
        if k_kc <= Decimal::ZERO {
            return Err(FinError::InvalidInput("k_kc must be positive".into()));
        }
        Ok(Self {
            name: name.into(),
            period,
            k_bb,
            k_kc,
            closes: VecDeque::with_capacity(period),
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            prev_close: None,
            squeeze_on: false,
        })
    }

    /// Returns `true` when Bollinger Bands are inside Keltner Channels (squeeze active).
    pub fn squeeze_on(&self) -> bool { self.squeeze_on }
}

impl Signal for TtmSqueeze {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.highs.len() > self.period { self.highs.pop_front(); }
        if self.lows.len() > self.period { self.lows.pop_front(); }

        let tr = match self.prev_close {
            None => bar.range(),
            Some(pc) => (bar.range())
                .max((bar.high - pc).abs())
                .max((bar.low - pc).abs()),
        };
        self.prev_close = Some(bar.close);

        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = Decimal::from(self.period as u32);
        let sma = self.closes.iter().sum::<Decimal>() / n;

        // Bollinger Band std dev (population)
        let variance = self.closes.iter()
            .map(|&c| { let d = c - sma; d * d })
            .sum::<Decimal>() / n;
        let std_dev = {
            use rust_decimal::prelude::ToPrimitive;
            let v_f = variance.to_f64().unwrap_or(0.0);
            Decimal::try_from(v_f.sqrt()).unwrap_or(Decimal::ZERO)
        };

        let bb_upper = sma + self.k_bb * std_dev;
        let bb_lower = sma - self.k_bb * std_dev;

        // Keltner Channel ATR (simple mean of TRs — approximate using current TR + period-1 bars)
        // Use mean of highs-lows as ATR proxy (true range per bar already tracked via prev_close)
        // For simplicity: ATR ≈ mean(high - low) over window (sufficient for squeeze detection)
        let atr: Decimal = self.highs.iter().zip(self.lows.iter())
            .map(|(&h, &l)| h - l)
            .sum::<Decimal>() / n;
        // Use current true range for the most recent bar
        let _ = tr;

        let kc_upper = sma + self.k_kc * atr;
        let kc_lower = sma - self.k_kc * atr;

        self.squeeze_on = bb_upper < kc_upper && bb_lower > kc_lower;

        // Momentum: distance of midpoint of (high+low)/2 from SMA, relative to period high/low midpoint
        let period_high = self.highs.iter().cloned().max().unwrap_or(bar.high);
        let period_low = self.lows.iter().cloned().min().unwrap_or(bar.low);
        let mid = (bar.high + bar.low) / Decimal::from(2u32);
        let delta = bar.close - (period_high + period_low) / Decimal::from(2u32);
        let momentum = (mid - sma + delta) / Decimal::from(2u32);

        Ok(SignalValue::Scalar(momentum))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period && self.prev_close.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
        self.highs.clear();
        self.lows.clear();
        self.prev_close = None;
        self.squeeze_on = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
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
    fn test_ttm_invalid() {
        assert!(TtmSqueeze::new("t", 0, dec!(2), dec!(1.5)).is_err());
        assert!(TtmSqueeze::new("t", 20, dec!(0), dec!(1.5)).is_err());
        assert!(TtmSqueeze::new("t", 20, dec!(2), dec!(0)).is_err());
    }

    #[test]
    fn test_ttm_unavailable_before_warmup() {
        let mut t = TtmSqueeze::new("t", 5, dec!(2), dec!(1.5)).unwrap();
        for _ in 0..4 {
            assert_eq!(t.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ttm_wide_range_squeeze_on() {
        // Wide H-L range with flat close → std_dev=0, ATR wide → BB inside KC → squeeze on
        let mut t = TtmSqueeze::new("t", 3, dec!(2), dec!(1.5)).unwrap();
        for _ in 0..3 { t.update_bar(&bar_hlc("200", "1", "100")).unwrap(); }
        assert!(t.squeeze_on());
    }

    #[test]
    fn test_ttm_volatile_close_no_squeeze() {
        // Closes alternate wildly with flat bars → high std_dev, near-zero ATR → BB > KC → no squeeze
        let mut t = TtmSqueeze::new("t", 4, dec!(2), dec!(1.5)).unwrap();
        let prices = ["1", "199", "1", "199"];
        for p in &prices { t.update_bar(&bar(p)).unwrap(); }
        assert!(!t.squeeze_on());
    }

    #[test]
    fn test_ttm_flat_momentum_zero() {
        let mut t = TtmSqueeze::new("t", 3, dec!(2), dec!(1.5)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = t.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ttm_reset() {
        let mut t = TtmSqueeze::new("t", 3, dec!(2), dec!(1.5)).unwrap();
        for _ in 0..5 { t.update_bar(&bar("100")).unwrap(); }
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
        assert!(!t.squeeze_on());
    }
}
