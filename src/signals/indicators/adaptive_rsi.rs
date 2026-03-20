//! Adaptive RSI indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Adaptive RSI — RSI whose period adapts to market volatility.
///
/// When the Efficiency Ratio (ER) is high (trending), the effective RSI
/// period shortens (min_period) for faster signals. When ER is low (choppy),
/// the period lengthens (max_period) to filter noise.
///
/// ```text
/// ER         = |close_t − close_{t−er_period}| / path(er_period)
/// eff_period = round(min_period + (1 − ER) × (max_period − min_period))
/// output     = RSI(close, eff_period)  [Wilder smoothed]
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `er_period + max_period` bars seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AdaptiveRsi;
/// use fin_primitives::signals::Signal;
///
/// let ar = AdaptiveRsi::new("ar", 10, 2, 20).unwrap();
/// assert_eq!(ar.period(), 10);
/// ```
pub struct AdaptiveRsi {
    name: String,
    er_period: usize,
    min_period: usize,
    max_period: usize,
    closes: Vec<Decimal>,
    // current RSI state
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    rsi_seed_gains: Vec<Decimal>,
    rsi_seed_losses: Vec<Decimal>,
    current_period: Option<usize>,
    prev_rsi_close: Option<Decimal>,
}

impl AdaptiveRsi {
    /// Creates a new `AdaptiveRsi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `er_period == 0`.
    /// Returns [`FinError::InvalidInput`] if `min_period >= max_period`.
    pub fn new(
        name: impl Into<String>,
        er_period: usize,
        min_period: usize,
        max_period: usize,
    ) -> Result<Self, FinError> {
        if er_period == 0 { return Err(FinError::InvalidPeriod(er_period)); }
        if min_period == 0 { return Err(FinError::InvalidPeriod(min_period)); }
        if min_period >= max_period {
            return Err(FinError::InvalidInput("min_period must be < max_period".into()));
        }
        Ok(Self {
            name: name.into(),
            er_period,
            min_period,
            max_period,
            closes: Vec::with_capacity(er_period + 1),
            avg_gain: None,
            avg_loss: None,
            rsi_seed_gains: Vec::with_capacity(max_period),
            rsi_seed_losses: Vec::with_capacity(max_period),
            current_period: None,
            prev_rsi_close: None,
        })
    }

    fn compute_er(closes: &[Decimal], er_period: usize) -> Decimal {
        if closes.len() < er_period + 1 { return Decimal::ZERO; }
        let n = closes.len();
        let direction = (closes[n - 1] - closes[n - 1 - er_period]).abs();
        let path: Decimal = closes[n - 1 - er_period..n]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum();
        if path.is_zero() { Decimal::ZERO } else { direction / path }
    }

    fn effective_period(er: Decimal, min: usize, max: usize) -> usize {
        let er_f = er.to_string().parse::<f64>().unwrap_or(0.0);
        let p = min as f64 + (1.0 - er_f) * (max - min) as f64;
        (p.round() as usize).max(min).min(max)
    }
}

impl Signal for AdaptiveRsi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push(bar.close);

        // Need enough bars for ER calculation
        if self.closes.len() < self.er_period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let er = Self::compute_er(&self.closes, self.er_period);
        let eff_period = Self::effective_period(er, self.min_period, self.max_period);

        // RSI phase: seed or update
        let prev = match self.prev_rsi_close {
            None => {
                self.prev_rsi_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_rsi_close = Some(bar.close);

        let change = bar.close - prev;
        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };

        if self.avg_gain.is_none() || self.current_period != Some(eff_period) {
            // Reset RSI seed when period changes or not yet seeded
            if self.current_period != Some(eff_period) {
                self.rsi_seed_gains.clear();
                self.rsi_seed_losses.clear();
                self.avg_gain = None;
                self.avg_loss = None;
                self.current_period = Some(eff_period);
            }
            if self.avg_gain.is_none() {
                self.rsi_seed_gains.push(gain);
                self.rsi_seed_losses.push(loss);
                if self.rsi_seed_gains.len() == eff_period {
                    let ag = self.rsi_seed_gains.iter().sum::<Decimal>()
                        / Decimal::from(eff_period as u32);
                    let al = self.rsi_seed_losses.iter().sum::<Decimal>()
                        / Decimal::from(eff_period as u32);
                    self.avg_gain = Some(ag);
                    self.avg_loss = Some(al);
                }
                return Ok(SignalValue::Unavailable);
            }
        }

        let k = Decimal::ONE / Decimal::from(eff_period as u32);
        let ag = self.avg_gain.unwrap() * (Decimal::ONE - k) + gain * k;
        let al = self.avg_loss.unwrap() * (Decimal::ONE - k) + loss * k;
        self.avg_gain = Some(ag);
        self.avg_loss = Some(al);

        if al.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from(100u32)));
        }
        let rs = ag / al;
        let rsi = Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs);
        Ok(SignalValue::Scalar(rsi))
    }

    fn is_ready(&self) -> bool { self.avg_gain.is_some() }
    fn period(&self) -> usize { self.er_period }

    fn reset(&mut self) {
        self.closes.clear();
        self.avg_gain = None;
        self.avg_loss = None;
        self.rsi_seed_gains.clear();
        self.rsi_seed_losses.clear();
        self.current_period = None;
        self.prev_rsi_close = None;
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

    #[test]
    fn test_arsi_invalid() {
        assert!(AdaptiveRsi::new("a", 0, 2, 20).is_err());
        assert!(AdaptiveRsi::new("a", 10, 0, 20).is_err());
        assert!(AdaptiveRsi::new("a", 10, 20, 10).is_err());
        assert!(AdaptiveRsi::new("a", 10, 10, 10).is_err());
    }

    #[test]
    fn test_arsi_unavailable_before_warmup() {
        let mut a = AdaptiveRsi::new("a", 5, 2, 10).unwrap();
        assert_eq!(a.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_arsi_range_0_to_100() {
        let mut a = AdaptiveRsi::new("a", 5, 2, 10).unwrap();
        for i in 0u32..30 {
            let p = if i % 3 == 0 { format!("{}", 100 + i) } else { format!("{}", 100 + 10 - i % 5) };
            if let SignalValue::Scalar(v) = a.update_bar(&bar(&p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(100), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_arsi_reset() {
        let mut a = AdaptiveRsi::new("a", 5, 2, 10).unwrap();
        for i in 0u32..30 {
            let p = format!("{}", 100 + i);
            a.update_bar(&bar(&p)).unwrap();
        }
        a.reset();
        assert!(!a.is_ready());
    }
}
