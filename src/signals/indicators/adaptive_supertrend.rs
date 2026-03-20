//! Adaptive Supertrend indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Adaptive Supertrend — a Supertrend indicator whose ATR period adjusts based on
/// the Kaufman Efficiency Ratio (ER) of recent price action.
///
/// In trending markets (ER near 1.0), the ATR period shortens to `min_period`,
/// making the stop more responsive. In choppy markets (ER near 0.0), the period
/// lengthens to `max_period`, widening the band to avoid whipsaws.
///
/// Effective ATR period:
/// ```text
/// eff_period = min_period + (1 - ER) × (max_period - min_period)
/// ```
///
/// The scalar output is the Supertrend stop level:
/// - In a **long** regime: stop is below price → `stop < close`
/// - In a **short** regime: stop is above price → `stop > close`
///
/// Use [`AdaptiveSupertrend::is_long`] to determine the current regime.
///
/// Returns [`SignalValue::Unavailable`] until `max_period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AdaptiveSupertrend;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let ast = AdaptiveSupertrend::new("ast", 5, 20, dec!(3)).unwrap();
/// assert_eq!(ast.period(), 20);
/// ```
pub struct AdaptiveSupertrend {
    name: String,
    min_period: usize,
    max_period: usize,
    multiplier: Decimal,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
    stop: Option<Decimal>,
    long: bool,
}

impl AdaptiveSupertrend {
    /// Constructs a new `AdaptiveSupertrend`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `min_period == 0` or
    /// `min_period >= max_period`.
    pub fn new(
        name: impl Into<String>,
        min_period: usize,
        max_period: usize,
        multiplier: Decimal,
    ) -> Result<Self, FinError> {
        if min_period == 0 || min_period >= max_period {
            return Err(FinError::InvalidPeriod(min_period));
        }
        Ok(Self {
            name: name.into(),
            min_period,
            max_period,
            multiplier,
            highs: VecDeque::with_capacity(max_period + 1),
            lows: VecDeque::with_capacity(max_period + 1),
            closes: VecDeque::with_capacity(max_period + 2),
            stop: None,
            long: true,
        })
    }

    /// Returns `true` when in a long (bullish) regime.
    pub fn is_long(&self) -> bool {
        self.long
    }

    fn efficiency_ratio(closes: &VecDeque<Decimal>) -> Decimal {
        let n = closes.len();
        if n < 2 { return Decimal::ZERO; }
        let net = (closes[n - 1] - closes[0]).abs();
        let path: Decimal = closes.iter().zip(closes.iter().skip(1)).map(|(a, b)| (*b - *a).abs()).sum();
        if path.is_zero() { Decimal::ZERO } else { net / path }
    }

    fn adaptive_atr(
        highs: &VecDeque<Decimal>,
        lows: &VecDeque<Decimal>,
        closes: &VecDeque<Decimal>,
        eff_period: usize,
    ) -> Decimal {
        if highs.len() < eff_period { return Decimal::ZERO; }
        let h_slice = highs.iter().rev().take(eff_period);
        let l_slice = lows.iter().rev().take(eff_period);
        let c_slice = closes.iter().rev().skip(1).take(eff_period);
        let trs: Vec<Decimal> = h_slice.zip(l_slice).zip(c_slice).map(|((h, l), pc)| {
            let hl = h - l;
            let hc = (h - pc).abs();
            let lc = (l - pc).abs();
            hl.max(hc).max(lc)
        }).collect();
        if trs.is_empty() { return Decimal::ZERO; }
        let len = trs.len() as u32;
        trs.iter().sum::<Decimal>() / Decimal::from(len)
    }
}

impl Signal for AdaptiveSupertrend {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.max_period
    }

    fn is_ready(&self) -> bool {
        self.stop.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        if self.highs.len() > self.max_period + 1 { self.highs.pop_front(); }
        self.lows.push_back(bar.low);
        if self.lows.len() > self.max_period + 1 { self.lows.pop_front(); }
        self.closes.push_back(bar.close);
        if self.closes.len() > self.max_period + 2 { self.closes.pop_front(); }

        if self.highs.len() < self.max_period { return Ok(SignalValue::Unavailable); }

        let er = Self::efficiency_ratio(&self.closes);
        // eff_period in [min_period, max_period]
        let range = Decimal::from((self.max_period - self.min_period) as u32);
        let eff_period_d = Decimal::from(self.min_period as u32) + (Decimal::ONE - er) * range;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let eff_period = eff_period_d
            .to_u64()
            .map(|v| v as usize)
            .unwrap_or(self.max_period)
            .clamp(self.min_period, self.max_period);

        let atr = Self::adaptive_atr(&self.highs, &self.lows, &self.closes, eff_period);
        let hl2 = (bar.high + bar.low) / Decimal::TWO;
        let close = bar.close;

        let new_stop = match self.stop {
            None => {
                self.long = true;
                let s = hl2 - self.multiplier * atr;
                self.stop = Some(s);
                return Ok(SignalValue::Scalar(s));
            }
            Some(prev_stop) => {
                if self.long {
                    let candidate = hl2 - self.multiplier * atr;
                    if close < prev_stop {
                        self.long = false;
                        hl2 + self.multiplier * atr
                    } else {
                        candidate.max(prev_stop)
                    }
                } else {
                    let candidate = hl2 + self.multiplier * atr;
                    if close > prev_stop {
                        self.long = true;
                        hl2 - self.multiplier * atr
                    } else {
                        candidate.min(prev_stop)
                    }
                }
            }
        };
        self.stop = Some(new_stop);
        Ok(SignalValue::Scalar(new_stop))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.closes.clear();
        self.stop = None;
        self.long = true;
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
    fn test_ast_invalid_params() {
        assert!(AdaptiveSupertrend::new("a", 0, 10, dec!(3)).is_err());
        assert!(AdaptiveSupertrend::new("a", 10, 5, dec!(3)).is_err());
        assert!(AdaptiveSupertrend::new("a", 5, 5, dec!(3)).is_err());
    }

    #[test]
    fn test_ast_unavailable_before_max_period() {
        let mut ast = AdaptiveSupertrend::new("a", 3, 5, dec!(2)).unwrap();
        for _ in 0..4 {
            assert_eq!(ast.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ast.is_ready());
    }

    #[test]
    fn test_ast_ready_after_max_period() {
        let mut ast = AdaptiveSupertrend::new("a", 3, 5, dec!(2)).unwrap();
        for _ in 0..6 {
            ast.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(ast.is_ready());
    }

    #[test]
    fn test_ast_long_regime_rising() {
        let mut ast = AdaptiveSupertrend::new("a", 2, 5, dec!(1)).unwrap();
        for i in 0u32..10 {
            let p = (100 + i * 5).to_string();
            ast.update_bar(&bar(&p, &(95u32 + i * 5).to_string(), &p)).unwrap();
        }
        assert!(ast.is_long());
    }

    #[test]
    fn test_ast_reset() {
        let mut ast = AdaptiveSupertrend::new("a", 3, 5, dec!(2)).unwrap();
        for _ in 0..8 {
            ast.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(ast.is_ready());
        ast.reset();
        assert!(!ast.is_ready());
    }
}
