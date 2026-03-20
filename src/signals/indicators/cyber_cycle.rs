//! Ehlers Cyber Cycle indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Ehlers Cyber Cycle — a zero-lag cycle-extraction filter by John Ehlers.
///
/// The Cyber Cycle separates the cycle component of price action from the
/// dominant trend. It is derived from a two-pole high-pass filter applied to the
/// median price `(high + low) / 2`, parameterised by `alpha` (smoothing factor).
///
/// Recurrence relation (Ehlers, *Cybernetic Analysis for Stocks and Futures*, 2004):
/// ```text
/// smooth[i] = (price + 2×price[1] + 2×price[2] + price[3]) / 6
/// cycle[i]  = (1 − α/2)² × (smooth[i] − 2×smooth[i-1] + smooth[i-2])
///           + 2×(1 − α) × cycle[i-1]
///           − (1 − α)² × cycle[i-2]
/// ```
///
/// During the first four bars the output is set to `smooth − smooth[1]` to
/// avoid a zero-filled warm-up period.
///
/// `alpha` is derived from the user-supplied `period` as `2 / (period + 1)`,
/// mirroring EMA convention. Typical period values are 5–14.
///
/// Returns [`SignalValue::Unavailable`] until 4 bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EhlersCyberCycle;
/// use fin_primitives::signals::Signal;
///
/// let cc = EhlersCyberCycle::new("cc", 7).unwrap();
/// assert_eq!(cc.period(), 7);
/// assert!(!cc.is_ready());
/// ```
pub struct EhlersCyberCycle {
    name: String,
    period: usize,
    alpha: Decimal,
    prices: VecDeque<Decimal>,
    smoothed: VecDeque<Decimal>,
    cycles: VecDeque<Decimal>,
}

impl EhlersCyberCycle {
    /// Constructs a new `EhlersCyberCycle` with the given `period`.
    ///
    /// `period` must be ≥ 2. `alpha = 2 / (period + 1)`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        let alpha = Decimal::TWO
            .checked_div(Decimal::from((period + 1) as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(Self {
            name: name.into(),
            period,
            alpha,
            prices: VecDeque::with_capacity(4),
            smoothed: VecDeque::with_capacity(4),
            cycles: VecDeque::with_capacity(3),
        })
    }
}

impl Signal for EhlersCyberCycle {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let price = (bar.high + bar.low) / Decimal::TWO;
        self.prices.push_back(price);
        if self.prices.len() > 4 {
            self.prices.pop_front();
        }

        // Smooth = (P + 2P[1] + 2P[2] + P[3]) / 6
        let smooth = if self.prices.len() >= 4 {
            let p  = self.prices[3];
            let p1 = self.prices[2];
            let p2 = self.prices[1];
            let p3 = self.prices[0];
            (p + Decimal::TWO * p1 + Decimal::TWO * p2 + p3)
                .checked_div(Decimal::from(6u32))
                .ok_or(FinError::ArithmeticOverflow)?
        } else {
            price
        };

        self.smoothed.push_back(smooth);
        if self.smoothed.len() > 3 {
            self.smoothed.pop_front();
        }

        if self.prices.len() < 4 {
            return Ok(SignalValue::Unavailable);
        }

        let a = self.alpha;
        let one_minus_a = Decimal::ONE - a;
        let one_minus_a_sq = one_minus_a * one_minus_a;
        let a_half = a / Decimal::TWO;
        let one_minus_a_half = Decimal::ONE - a_half;
        let one_minus_a_half_sq = one_minus_a_half * one_minus_a_half;

        let s0 = self.smoothed[self.smoothed.len() - 1];
        let s1 = if self.smoothed.len() >= 2 { self.smoothed[self.smoothed.len() - 2] } else { s0 };
        let s2 = if self.smoothed.len() >= 3 { self.smoothed[self.smoothed.len() - 3] } else { s1 };

        let cycle = if self.cycles.len() < 2 {
            // Warm-up: simple derivative
            s0 - s1
        } else {
            let c1 = self.cycles[self.cycles.len() - 1];
            let c2 = self.cycles[self.cycles.len() - 2];
            one_minus_a_half_sq * (s0 - Decimal::TWO * s1 + s2)
                + Decimal::TWO * one_minus_a * c1
                - one_minus_a_sq * c2
        };

        self.cycles.push_back(cycle);
        if self.cycles.len() > 3 {
            self.cycles.pop_front();
        }

        Ok(SignalValue::Scalar(cycle))
    }

    fn is_ready(&self) -> bool {
        self.prices.len() >= 4
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prices.clear();
        self.smoothed.clear();
        self.cycles.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cc_period_zero_invalid() {
        assert!(EhlersCyberCycle::new("cc", 0).is_err());
        assert!(EhlersCyberCycle::new("cc", 1).is_err());
    }

    #[test]
    fn test_cc_unavailable_before_four_bars() {
        let mut cc = EhlersCyberCycle::new("cc", 7).unwrap();
        for _ in 0..3 {
            assert_eq!(cc.update_bar(&bar("100", "95")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!cc.is_ready());
    }

    #[test]
    fn test_cc_ready_after_four_bars() {
        let mut cc = EhlersCyberCycle::new("cc", 7).unwrap();
        for _ in 0..4 {
            cc.update_bar(&bar("100", "95")).unwrap();
        }
        assert!(cc.is_ready());
    }

    #[test]
    fn test_cc_flat_price_near_zero_cycle() {
        let mut cc = EhlersCyberCycle::new("cc", 7).unwrap();
        for _ in 0..20 {
            cc.update_bar(&bar("100", "100")).unwrap();
        }
        // Flat prices → cycle should converge to near zero
        if let SignalValue::Scalar(v) = cc.update_bar(&bar("100", "100")).unwrap() {
            assert!(v.abs() < rust_decimal_macros::dec!(0.01), "expected near-zero cycle, got {v}");
        }
    }

    #[test]
    fn test_cc_reset() {
        let mut cc = EhlersCyberCycle::new("cc", 5).unwrap();
        for _ in 0..5 {
            cc.update_bar(&bar("100", "90")).unwrap();
        }
        assert!(cc.is_ready());
        cc.reset();
        assert!(!cc.is_ready());
    }
}
