//! Connors RSI indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Connors RSI — composite momentum oscillator combining three components.
///
/// ```text
/// Component 1: RSI(close, rsi_period)          — standard price RSI
/// Component 2: RSI(streak, 2)                  — RSI of up/down streak length
/// Component 3: PercentRank(ROC(1), rank_period) — percentile rank of 1-bar ROC
///
/// ConnorsRSI = (C1 + C2 + C3) / 3
/// ```
///
/// Streak: incremented by 1 on up bar, decremented by 1 on down bar, reset to 0 on flat.
/// Typical parameters: `rsi_period=3`, `streak_period=2`, `rank_period=100`.
///
/// Returns [`SignalValue::Unavailable`] until all three components are ready.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConnorsRsi;
/// use fin_primitives::signals::Signal;
///
/// let c = ConnorsRsi::new("crsi", 3, 2, 100).unwrap();
/// assert_eq!(c.period(), 100);
/// ```
pub struct ConnorsRsi {
    name: String,
    rsi_period: usize,
    streak_period: usize,
    rank_period: usize,

    // RSI(close) state
    prev_close: Option<Decimal>,
    rsi_count: usize,
    rsi_avg_gain: Option<Decimal>,
    rsi_avg_loss: Option<Decimal>,
    rsi_seed_gains: Vec<Decimal>,
    rsi_seed_losses: Vec<Decimal>,

    // Streak RSI state
    streak: Decimal,
    streak_rsi_count: usize,
    streak_rsi_avg_gain: Option<Decimal>,
    streak_rsi_avg_loss: Option<Decimal>,
    streak_seed_gains: Vec<Decimal>,
    streak_seed_losses: Vec<Decimal>,
    prev_streak: Option<Decimal>,

    // PercentRank state
    rocs: VecDeque<Decimal>,
}

fn rsi_value(avg_gain: Decimal, avg_loss: Decimal) -> Decimal {
    if avg_loss.is_zero() {
        return Decimal::from(100u32);
    }
    let rs = avg_gain / avg_loss;
    Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs)
}

impl ConnorsRsi {
    /// Creates a new `ConnorsRsi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero.
    pub fn new(
        name: impl Into<String>,
        rsi_period: usize,
        streak_period: usize,
        rank_period: usize,
    ) -> Result<Self, FinError> {
        if rsi_period == 0 { return Err(FinError::InvalidPeriod(rsi_period)); }
        if streak_period == 0 { return Err(FinError::InvalidPeriod(streak_period)); }
        if rank_period == 0 { return Err(FinError::InvalidPeriod(rank_period)); }
        Ok(Self {
            name: name.into(),
            rsi_period,
            streak_period,
            rank_period,
            prev_close: None,
            rsi_count: 0,
            rsi_avg_gain: None,
            rsi_avg_loss: None,
            rsi_seed_gains: Vec::with_capacity(rsi_period),
            rsi_seed_losses: Vec::with_capacity(rsi_period),
            streak: Decimal::ZERO,
            streak_rsi_count: 0,
            streak_rsi_avg_gain: None,
            streak_rsi_avg_loss: None,
            streak_seed_gains: Vec::with_capacity(streak_period),
            streak_seed_losses: Vec::with_capacity(streak_period),
            prev_streak: None,
            rocs: VecDeque::with_capacity(rank_period),
        })
    }

    fn update_rsi_component(
        close: Decimal,
        prev: Decimal,
        count: &mut usize,
        seed_gains: &mut Vec<Decimal>,
        seed_losses: &mut Vec<Decimal>,
        avg_gain: &mut Option<Decimal>,
        avg_loss: &mut Option<Decimal>,
        period: usize,
    ) -> Option<Decimal> {
        let change = close - prev;
        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };
        *count += 1;

        if avg_gain.is_none() {
            seed_gains.push(gain);
            seed_losses.push(loss);
            if seed_gains.len() == period {
                let ag = seed_gains.iter().sum::<Decimal>() / Decimal::from(period as u32);
                let al = seed_losses.iter().sum::<Decimal>() / Decimal::from(period as u32);
                *avg_gain = Some(ag);
                *avg_loss = Some(al);
                return Some(rsi_value(ag, al));
            }
            return None;
        }

        let k = Decimal::ONE / Decimal::from(period as u32);
        let ag = avg_gain.unwrap() * (Decimal::ONE - k) + gain * k;
        let al = avg_loss.unwrap() * (Decimal::ONE - k) + loss * k;
        *avg_gain = Some(ag);
        *avg_loss = Some(al);
        Some(rsi_value(ag, al))
    }
}

impl Signal for ConnorsRsi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        // --- Component 1: RSI(close) ---
        let rsi_c1 = if let Some(pc) = self.prev_close {
            Self::update_rsi_component(
                close, pc,
                &mut self.rsi_count,
                &mut self.rsi_seed_gains,
                &mut self.rsi_seed_losses,
                &mut self.rsi_avg_gain,
                &mut self.rsi_avg_loss,
                self.rsi_period,
            )
        } else {
            None
        };

        // --- Update streak ---
        let new_streak = if let Some(pc) = self.prev_close {
            if close > pc {
                if self.streak > Decimal::ZERO { self.streak + Decimal::ONE }
                else { Decimal::ONE }
            } else if close < pc {
                if self.streak < Decimal::ZERO { self.streak - Decimal::ONE }
                else { -Decimal::ONE }
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ZERO
        };

        self.prev_close = Some(close);

        // --- Component 2: RSI(streak) ---
        let rsi_c2 = if let Some(ps) = self.prev_streak {
            Self::update_rsi_component(
                new_streak, ps,
                &mut self.streak_rsi_count,
                &mut self.streak_seed_gains,
                &mut self.streak_seed_losses,
                &mut self.streak_rsi_avg_gain,
                &mut self.streak_rsi_avg_loss,
                self.streak_period,
            )
        } else {
            None
        };
        self.prev_streak = Some(new_streak);
        self.streak = new_streak;

        // --- Component 3: PercentRank of ROC(1) ---
        // Use streak as the per-bar directional change for ranking (sign + magnitude match ROC)
        let roc_val = new_streak;
        self.rocs.push_back(roc_val);
        if self.rocs.len() > self.rank_period { self.rocs.pop_front(); }

        let rsi_c3 = if self.rocs.len() == self.rank_period {
            let count_below = self.rocs.iter()
                .filter(|&&r| r < roc_val)
                .count();
            #[allow(clippy::cast_possible_truncation)]
            Some(Decimal::from(count_below as u32)
                / Decimal::from(self.rank_period as u32)
                * Decimal::from(100u32))
        } else {
            None
        };

        match (rsi_c1, rsi_c2, rsi_c3) {
            (Some(c1), Some(c2), Some(c3)) => {
                let crsi = (c1 + c2 + c3) / Decimal::from(3u32);
                Ok(SignalValue::Scalar(crsi))
            }
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.rsi_avg_gain.is_some()
            && self.streak_rsi_avg_gain.is_some()
            && self.rocs.len() >= self.rank_period
    }

    fn period(&self) -> usize { self.rank_period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.rsi_count = 0;
        self.rsi_avg_gain = None;
        self.rsi_avg_loss = None;
        self.rsi_seed_gains.clear();
        self.rsi_seed_losses.clear();
        self.streak = Decimal::ZERO;
        self.streak_rsi_count = 0;
        self.streak_rsi_avg_gain = None;
        self.streak_rsi_avg_loss = None;
        self.streak_seed_gains.clear();
        self.streak_seed_losses.clear();
        self.prev_streak = None;
        self.rocs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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
    fn test_crsi_invalid() {
        assert!(ConnorsRsi::new("c", 0, 2, 100).is_err());
        assert!(ConnorsRsi::new("c", 3, 0, 100).is_err());
        assert!(ConnorsRsi::new("c", 3, 2, 0).is_err());
    }

    #[test]
    fn test_crsi_unavailable_before_warmup() {
        let mut c = ConnorsRsi::new("c", 3, 2, 10).unwrap();
        // Not ready before warmup completes
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!c.is_ready());
    }

    #[test]
    fn test_crsi_ready_after_enough_bars() {
        let mut c = ConnorsRsi::new("c", 3, 2, 10).unwrap();
        // alternating prices generate changes for all RSI seeds
        let prices: Vec<u32> = (0..30).map(|i| 100 + (i % 3)).collect();
        let mut ready = false;
        for p in &prices {
            let _ = c.update_bar(&bar(&p.to_string())).unwrap();
            if c.is_ready() { ready = true; break; }
        }
        assert!(ready);
    }

    #[test]
    fn test_crsi_reset() {
        let mut c = ConnorsRsi::new("c", 3, 2, 10).unwrap();
        let prices: Vec<u32> = (0..30).map(|i| 100 + (i % 3)).collect();
        for p in &prices { let _ = c.update_bar(&bar(&p.to_string())).unwrap(); }
        c.reset();
        assert!(!c.is_ready());
    }

    #[test]
    fn test_crsi_output_range() {
        let mut c = ConnorsRsi::new("c", 3, 2, 10).unwrap();
        let prices: Vec<u32> = (0..50).map(|i| 100 + (i % 5)).collect();
        for p in &prices {
            if let SignalValue::Scalar(v) = c.update_bar(&bar(&p.to_string())).unwrap() {
                assert!(v >= Decimal::ZERO && v <= Decimal::from(100u32),
                    "CRSI out of range: {v}");
            }
        }
    }
}
