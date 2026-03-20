//! Trend Magic indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Magic — trend direction indicator combining CCI and ATR.
///
/// Originally published by mohanee for TradingView. The indicator draws a support/
/// resistance line that flips when:
/// - CCI crosses above `+100` → flip to bullish (stop trails below)
/// - CCI crosses below `-100` → flip to bearish (stop trails above)
///
/// The stop line itself uses ATR-based trailing:
/// ```text
/// long_stop  = high - multiplier × ATR(atr_period)
/// short_stop = low  + multiplier × ATR(atr_period)
/// ```
///
/// Outputs `+1` (bullish), `-1` (bearish), or `0` (not yet determined).
///
/// Returns [`SignalValue::Unavailable`] until `max(cci_period, atr_period) + 1` bars.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendMagic;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let tm = TrendMagic::new("tm", 20, 5, dec!(1.5)).unwrap();
/// assert_eq!(tm.period(), 20);
/// ```
pub struct TrendMagic {
    name: String,
    cci_period: usize,
    atr_period: usize,
    multiplier: Decimal,
    typical_prices: VecDeque<Decimal>,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
    prev_cci: Option<Decimal>,
    stop: Option<Decimal>,
    bullish: Option<bool>,
}

impl TrendMagic {
    /// Constructs a new `TrendMagic` indicator.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero.
    pub fn new(
        name: impl Into<String>,
        cci_period: usize,
        atr_period: usize,
        multiplier: Decimal,
    ) -> Result<Self, FinError> {
        if cci_period == 0 { return Err(FinError::InvalidPeriod(cci_period)); }
        if atr_period  == 0 { return Err(FinError::InvalidPeriod(atr_period)); }
        let warm_up = cci_period.max(atr_period);
        Ok(Self {
            name: name.into(),
            cci_period,
            atr_period,
            multiplier,
            typical_prices: VecDeque::with_capacity(cci_period),
            highs: VecDeque::with_capacity(warm_up + 1),
            lows: VecDeque::with_capacity(warm_up + 1),
            closes: VecDeque::with_capacity(warm_up + 2),
            prev_cci: None,
            stop: None,
            bullish: None,
        })
    }

    /// Returns `true` if the current regime is bullish.
    pub fn is_bullish(&self) -> bool {
        self.bullish == Some(true)
    }

    /// Returns `true` if the current regime is bearish.
    pub fn is_bearish(&self) -> bool {
        self.bullish == Some(false)
    }

    fn cci(typicals: &VecDeque<Decimal>) -> Option<Decimal> {
        let n = typicals.len();
        if n == 0 { return None; }
        #[allow(clippy::cast_possible_truncation)]
        let nd = Decimal::from(n as u32);
        let mean: Decimal = typicals.iter().sum::<Decimal>() / nd;
        let mad: Decimal = typicals.iter().map(|v| (*v - mean).abs()).sum::<Decimal>() / nd;
        if mad.is_zero() { return Some(Decimal::ZERO); }
        let latest = *typicals.back()?;
        let cci_015 = Decimal::from_str_exact("0.015").ok()?;
        (latest - mean).checked_div(cci_015 * mad)
    }

    fn atr(highs: &VecDeque<Decimal>, lows: &VecDeque<Decimal>, closes: &VecDeque<Decimal>, period: usize) -> Decimal {
        if highs.len() < period || closes.len() < 2 { return Decimal::ZERO; }
        let n = highs.len().min(period);
        let h_slice = highs.iter().rev().take(n);
        let l_slice = lows.iter().rev().take(n);
        let c_slice = closes.iter().rev().skip(1).take(n);
        let trs: Vec<Decimal> = h_slice.zip(l_slice).zip(c_slice).map(|((h, l), pc)| {
            let hl = h - l;
            let hc = (h - pc).abs();
            let lc = (l - pc).abs();
            hl.max(hc).max(lc)
        }).collect();
        if trs.is_empty() { return Decimal::ZERO; }
        #[allow(clippy::cast_possible_truncation)]
        let n = trs.len() as u32;
        trs.iter().sum::<Decimal>() / Decimal::from(n)
    }
}

impl Signal for TrendMagic {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.cci_period.max(self.atr_period) }
    fn is_ready(&self) -> bool { self.bullish.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = bar.typical_price();
        self.typical_prices.push_back(tp);
        if self.typical_prices.len() > self.cci_period { self.typical_prices.pop_front(); }
        self.highs.push_back(bar.high);
        if self.highs.len() > self.atr_period + 1 { self.highs.pop_front(); }
        self.lows.push_back(bar.low);
        if self.lows.len() > self.atr_period + 1 { self.lows.pop_front(); }
        self.closes.push_back(bar.close);
        if self.closes.len() > self.atr_period + 2 { self.closes.pop_front(); }

        let warm = self.cci_period.max(self.atr_period);
        if self.typical_prices.len() < self.cci_period || self.highs.len() < self.atr_period {
            return Ok(SignalValue::Unavailable);
        }

        let cci = Self::cci(&self.typical_prices).unwrap_or(Decimal::ZERO);
        let atr = Self::atr(&self.highs, &self.lows, &self.closes, self.atr_period);
        let prev_cci = self.prev_cci.unwrap_or(cci);
        let cci_100 = Decimal::ONE_HUNDRED;

        // Flip logic
        let now_bullish = match self.bullish {
            None => {
                // Bootstrap based on CCI level
                cci >= cci_100
            }
            Some(was_bullish) => {
                if !was_bullish && cci > cci_100 && prev_cci <= cci_100 {
                    true  // crossover above +100
                } else if was_bullish && cci < -cci_100 && prev_cci >= -cci_100 {
                    false // crossunder below -100
                } else {
                    was_bullish
                }
            }
        };
        self.bullish = Some(now_bullish);
        self.prev_cci = Some(cci);

        // Compute stop
        let new_stop = if now_bullish {
            let candidate = bar.high - self.multiplier * atr;
            self.stop.map(|s| s.max(candidate)).unwrap_or(candidate)
        } else {
            let candidate = bar.low + self.multiplier * atr;
            self.stop.map(|s| s.min(candidate)).unwrap_or(candidate)
        };
        self.stop = Some(new_stop);

        let signal = if now_bullish { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
        let _ = warm; // suppress unused warning
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {
        self.typical_prices.clear();
        self.highs.clear();
        self.lows.clear();
        self.closes.clear();
        self.prev_cci = None;
        self.stop     = None;
        self.bullish  = None;
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
    fn test_tm_invalid_period() {
        assert!(TrendMagic::new("t", 0, 5, dec!(1.5)).is_err());
        assert!(TrendMagic::new("t", 5, 0, dec!(1.5)).is_err());
    }

    #[test]
    fn test_tm_unavailable_before_warm_up() {
        let mut tm = TrendMagic::new("t", 3, 3, dec!(1.5)).unwrap();
        for _ in 0..2 {
            assert_eq!(tm.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!tm.is_ready());
    }

    #[test]
    fn test_tm_produces_signal_after_warm_up() {
        let mut tm = TrendMagic::new("t", 3, 3, dec!(1.5)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = tm.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)));
        assert!(tm.is_ready());
    }

    #[test]
    fn test_tm_reset() {
        let mut tm = TrendMagic::new("t", 3, 3, dec!(1.5)).unwrap();
        for _ in 0..5 { tm.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(tm.is_ready());
        tm.reset();
        assert!(!tm.is_ready());
    }

    #[test]
    fn test_tm_bullish_in_strong_uptrend() {
        let mut tm = TrendMagic::new("t", 5, 3, dec!(1.5)).unwrap();
        for i in 0u32..20 {
            let p = (100 + i * 5).to_string();
            tm.update_bar(&bar(&p, &(95u32 + i * 5).to_string(), &p)).unwrap();
        }
        assert!(tm.is_bullish());
    }
}
