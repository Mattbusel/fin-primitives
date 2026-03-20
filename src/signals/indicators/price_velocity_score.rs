//! Price Velocity Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Velocity Score — measures the current N-bar price velocity relative to
/// its historical baseline, expressed as a ratio.
///
/// ```text
/// velocity[t]       = close[t] - close[t - fast]
/// avg_velocity[t]   = SMA(|velocity|, slow)
/// score[t]          = velocity[t] / avg_velocity[t]
/// ```
///
/// Values significantly above 1 indicate above-average upward momentum;
/// significantly below -1 indicate below-average downward momentum.
///
/// Returns [`SignalValue::Unavailable`] until enough bars exist (max of fast and
/// slow periods + fast).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceVelocityScore;
/// use fin_primitives::signals::Signal;
///
/// let pvs = PriceVelocityScore::new("pvs", 5, 20).unwrap();
/// assert_eq!(pvs.period(), 20);
/// ```
pub struct PriceVelocityScore {
    name: String,
    fast: usize,
    slow: usize,
    closes: VecDeque<Decimal>,
    velocities: VecDeque<Decimal>,
}

impl PriceVelocityScore {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast == 0` or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 || fast >= slow { return Err(FinError::InvalidPeriod(fast)); }
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            closes: VecDeque::with_capacity(fast + 1),
            velocities: VecDeque::with_capacity(slow),
        })
    }
}

impl Signal for PriceVelocityScore {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow }
    fn is_ready(&self) -> bool { self.velocities.len() >= self.slow }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.fast + 1 { self.closes.pop_front(); }

        if self.closes.len() < self.fast + 1 { return Ok(SignalValue::Unavailable); }

        let velocity = *self.closes.back().unwrap() - *self.closes.front().unwrap();
        self.velocities.push_back(velocity.abs());
        if self.velocities.len() > self.slow { self.velocities.pop_front(); }

        if self.velocities.len() < self.slow { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let avg_vel = self.velocities.iter().sum::<Decimal>()
            / Decimal::from(self.slow as u32);
        if avg_vel.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(velocity / avg_vel))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.velocities.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_pvs_invalid() {
        assert!(PriceVelocityScore::new("p", 0, 10).is_err());
        assert!(PriceVelocityScore::new("p", 10, 5).is_err());
    }

    #[test]
    fn test_pvs_unavailable() {
        let mut pvs = PriceVelocityScore::new("p", 3, 5).unwrap();
        for _ in 0..7 {
            let r = pvs.update_bar(&bar("100")).unwrap();
            if !pvs.is_ready() {
                assert_eq!(r, SignalValue::Unavailable);
            }
        }
    }

    #[test]
    fn test_pvs_ready_after_warm_up() {
        let mut pvs = PriceVelocityScore::new("p", 2, 5).unwrap();
        for i in 0u32..8 { pvs.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(pvs.is_ready());
    }

    #[test]
    fn test_pvs_reset() {
        let mut pvs = PriceVelocityScore::new("p", 2, 5).unwrap();
        for i in 0u32..8 { pvs.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        pvs.reset();
        assert!(!pvs.is_ready());
    }
}
