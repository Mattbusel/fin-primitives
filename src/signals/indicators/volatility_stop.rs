//! Volatility Stop indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Stop — a trailing stop level computed as `price ± multiplier × ATR`.
///
/// Similar to Chandelier Exit but uses a simpler directional flip logic:
/// - In a **long** regime: stop = `highest_close(period) - multiplier × ATR`
/// - In a **short** regime: stop = `lowest_close(period) + multiplier × ATR`
///
/// The regime flips when the close crosses the current stop level.
///
/// The scalar output is the stop price. Use [`VolatilityStop::is_long`] to
/// determine the current regime.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (requires `period` bars to compute the initial ATR).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityStop;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let vs = VolatilityStop::new("vstop", 10, dec!(2)).unwrap();
/// assert_eq!(vs.period(), 10);
/// ```
pub struct VolatilityStop {
    name: String,
    period: usize,
    multiplier: Decimal,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    stop: Option<Decimal>,
    long: bool,
}

impl VolatilityStop {
    /// Constructs a new `VolatilityStop`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        multiplier: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            closes: VecDeque::with_capacity(period + 1),
            prev_close: None,
            stop: None,
            long: true,
        })
    }

    /// Returns `true` when the indicator is in a long (bullish) regime.
    pub fn is_long(&self) -> bool {
        self.long
    }

    /// Returns `true` when the indicator is in a short (bearish) regime.
    pub fn is_short(&self) -> bool {
        !self.long
    }

    fn atr(&self) -> Decimal {
        if self.highs.len() < self.period { return Decimal::ZERO; }
        let trs: Vec<Decimal> = self.highs.iter().zip(self.lows.iter()).zip(self.closes.iter())
            .map(|((h, l), pc)| {
                let hl = h - l;
                let hc = (h - pc).abs();
                let lc = (l - pc).abs();
                hl.max(hc).max(lc)
            })
            .collect();
        if trs.is_empty() { return Decimal::ZERO; }
        let n = trs.len();
        #[allow(clippy::cast_possible_truncation)]
        let n_d = Decimal::from(n as u32);
        trs.iter().sum::<Decimal>() / n_d
    }
}

impl Signal for VolatilityStop {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.stop.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Build prev_close window for TR computation
        if let Some(pc) = self.prev_close {
            self.closes.push_back(pc);
            if self.closes.len() > self.period {
                self.closes.pop_front();
            }
            self.highs.push_back(bar.high);
            if self.highs.len() > self.period {
                self.highs.pop_front();
            }
            self.lows.push_back(bar.low);
            if self.lows.len() > self.period {
                self.lows.pop_front();
            }
        }
        self.prev_close = Some(bar.close);

        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let atr = self.atr();
        let close = bar.close;

        let new_stop = match self.stop {
            None => {
                // Bootstrap: start long
                self.long = true;
                let max_close = self.closes.iter().copied().fold(close, Decimal::max);
                let s = max_close - self.multiplier * atr;
                self.stop = Some(s);
                s
            }
            Some(prev_stop) => {
                let s = if self.long {
                    let max_close = self.closes.iter().copied().fold(close, Decimal::max);
                    let candidate = max_close - self.multiplier * atr;
                    if close < prev_stop {
                        // Flip to short
                        self.long = false;
                        let min_close = self.closes.iter().copied().fold(close, Decimal::min);
                        min_close + self.multiplier * atr
                    } else {
                        candidate.max(prev_stop) // trail up only
                    }
                } else {
                    let min_close = self.closes.iter().copied().fold(close, Decimal::min);
                    let candidate = min_close + self.multiplier * atr;
                    if close > prev_stop {
                        // Flip to long
                        self.long = true;
                        let max_close = self.closes.iter().copied().fold(close, Decimal::max);
                        max_close - self.multiplier * atr
                    } else {
                        candidate.min(prev_stop) // trail down only
                    }
                };
                self.stop = Some(s);
                s
            }
        };

        Ok(SignalValue::Scalar(new_stop))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.closes.clear();
        self.prev_close = None;
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
    fn test_vstop_period_zero_fails() {
        assert!(VolatilityStop::new("vs", 0, dec!(2)).is_err());
    }

    #[test]
    fn test_vstop_unavailable_before_period() {
        let mut vs = VolatilityStop::new("vs", 3, dec!(2)).unwrap();
        for _ in 0..3 {
            assert_eq!(vs.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vs.is_ready());
    }

    #[test]
    fn test_vstop_ready_after_period() {
        let mut vs = VolatilityStop::new("vs", 3, dec!(2)).unwrap();
        for _ in 0..4 {
            vs.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(vs.is_ready());
    }

    #[test]
    fn test_vstop_long_regime_rising_prices() {
        let mut vs = VolatilityStop::new("vs", 3, dec!(1)).unwrap();
        for i in 0..6u32 {
            let p = (100 + i).to_string();
            vs.update_bar(&bar(&p, &(90u32 + i).to_string(), &p)).unwrap();
        }
        assert!(vs.is_long());
    }

    #[test]
    fn test_vstop_stop_is_below_close_in_long_regime() {
        let mut vs = VolatilityStop::new("vs", 3, dec!(2)).unwrap();
        let mut last_stop = Decimal::ZERO;
        for i in 0..6u32 {
            let p = (100 + i * 2).to_string();
            if let SignalValue::Scalar(s) = vs.update_bar(&bar(&p, &(95u32 + i).to_string(), &p)).unwrap() {
                last_stop = s;
            }
        }
        assert!(vs.is_long());
        // Stop should be below the last close
        assert!(last_stop < dec!(110));
    }

    #[test]
    fn test_vstop_reset() {
        let mut vs = VolatilityStop::new("vs", 3, dec!(2)).unwrap();
        for _ in 0..5 {
            vs.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(vs.is_ready());
        vs.reset();
        assert!(!vs.is_ready());
        assert!(vs.is_long()); // resets to default long
    }
}
