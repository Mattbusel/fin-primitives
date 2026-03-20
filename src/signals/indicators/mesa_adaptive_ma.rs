//! MESA Adaptive Moving Average (MAMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// MESA Adaptive Moving Average (MAMA) — John Ehlers' phase-adaptive moving average.
///
/// MAMA uses the Hilbert Transform to measure the dominant cycle period in price
/// action and adapts its smoothing coefficient accordingly. It accelerates in strong
/// trends (fast alpha) and decelerates in choppy markets (slow alpha).
///
/// The indicator produces two lines:
/// - **MAMA**: the adaptive moving average itself (scalar output)
/// - **FAMA**: the Following Adaptive Moving Average (`0.5 × alpha × MAMA`)
///
/// A MAMA cross above FAMA is a buy signal; below is a sell signal.
///
/// Implementation uses the simplified phase-accumulator form of the Hilbert
/// Transform (6-bar smooth → in-phase / quadrature decomposition) as described
/// in Ehlers' *Cybernetic Analysis for Stocks and Futures* (2004), Chapter 2.
///
/// Returns [`SignalValue::Unavailable`] until 7 bars have been accumulated
/// (minimum for the Hilbert Transform to stabilise).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MesaAdaptiveMa;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let m = MesaAdaptiveMa::new("mama", dec!(0.5), dec!(0.05)).unwrap();
/// assert_eq!(m.period(), 7);
/// assert!(!m.is_ready());
/// ```
pub struct MesaAdaptiveMa {
    name: String,
    fast_limit: Decimal,
    slow_limit: Decimal,
    prices: VecDeque<Decimal>,
    smooth: VecDeque<Decimal>,
    detrender: VecDeque<Decimal>,
    /// In-phase component history.
    i1: VecDeque<Decimal>,
    /// Quadrature component history.
    q1: VecDeque<Decimal>,
    prev_phase: Decimal,
    mama: Option<Decimal>,
    fama: Option<Decimal>,
}

impl MesaAdaptiveMa {
    /// Constructs a new `MesaAdaptiveMa`.
    ///
    /// - `fast_limit`: maximum alpha (e.g. `0.5`); must be in `(0, 1]`.
    /// - `slow_limit`: minimum alpha (e.g. `0.05`); must be in `(0, fast_limit)`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if limits are out of range.
    pub fn new(
        name: impl Into<String>,
        fast_limit: Decimal,
        slow_limit: Decimal,
    ) -> Result<Self, FinError> {
        if fast_limit <= Decimal::ZERO || fast_limit > Decimal::ONE {
            return Err(FinError::InvalidPeriod(0));
        }
        if slow_limit <= Decimal::ZERO || slow_limit >= fast_limit {
            return Err(FinError::InvalidPeriod(0));
        }
        Ok(Self {
            name: name.into(),
            fast_limit,
            slow_limit,
            prices: VecDeque::with_capacity(8),
            smooth: VecDeque::with_capacity(7),
            detrender: VecDeque::with_capacity(7),
            i1: VecDeque::with_capacity(4),
            q1: VecDeque::with_capacity(4),
            prev_phase: Decimal::ZERO,
            mama: None,
            fama: None,
        })
    }

    /// Returns the current MAMA value, or `None` if not ready.
    pub fn mama(&self) -> Option<Decimal> {
        self.mama
    }

    /// Returns the current FAMA value, or `None` if not ready.
    pub fn fama(&self) -> Option<Decimal> {
        self.fama
    }

    fn push_back_bounded(buf: &mut VecDeque<Decimal>, val: Decimal, max: usize) {
        buf.push_back(val);
        if buf.len() > max {
            buf.pop_front();
        }
    }

    fn get(buf: &VecDeque<Decimal>, back: usize) -> Decimal {
        let len = buf.len();
        if back >= len { Decimal::ZERO } else { buf[len - 1 - back] }
    }

    fn compute_smooth(prices: &VecDeque<Decimal>) -> Decimal {
        // 4-price WMA: (4P + 3P[1] + 2P[2] + P[3]) / 10
        let p0 = Self::get(prices, 0);
        let p1 = Self::get(prices, 1);
        let p2 = Self::get(prices, 2);
        let p3 = Self::get(prices, 3);
        (Decimal::from(4u32) * p0 + Decimal::from(3u32) * p1
            + Decimal::TWO * p2 + p3)
            / Decimal::from(10u32)
    }

    fn compute_detrender(smooth: &VecDeque<Decimal>) -> Decimal {
        // Hilbert discriminator: (0.0962*S + 0.5769*S[2] - 0.5769*S[4] - 0.0962*S[6]) * 0.075*Prev_Period + 0.54
        // Simplified constant-period version used here for clarity:
        let s0 = Self::get(smooth, 0);
        let s2 = Self::get(smooth, 2);
        let s4 = Self::get(smooth, 4);
        let s6 = Self::get(smooth, 6);
        let c1 = Decimal::new(962, 4);
        let c2 = Decimal::new(5769, 4);
        let factor = Decimal::new(354, 3); // 0.075*4.72 + 0.54 ≈ 0.354+0.54 but simplified
        (c1 * s0 + c2 * s2 - c2 * s4 - c1 * s6) * factor
    }
}

impl Signal for MesaAdaptiveMa {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        7
    }

    fn is_ready(&self) -> bool {
        self.mama.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let price = (bar.high + bar.low) / Decimal::TWO;
        Self::push_back_bounded(&mut self.prices, price, 8);

        let smooth = Self::compute_smooth(&self.prices);
        Self::push_back_bounded(&mut self.smooth, smooth, 7);

        if self.prices.len() < 7 {
            return Ok(SignalValue::Unavailable);
        }

        let det = Self::compute_detrender(&self.smooth);
        Self::push_back_bounded(&mut self.detrender, det, 7);

        // In-phase (I1) and Quadrature (Q1) Hilbert components
        let d0 = Self::get(&self.detrender, 0);
        let d2 = Self::get(&self.detrender, 2);
        let d4 = Self::get(&self.detrender, 4);
        let d6 = Self::get(&self.detrender, 6);
        let c1 = Decimal::new(962, 4);
        let c2 = Decimal::new(5769, 4);
        let factor = Decimal::new(354, 3);

        let q1_val = (c1 * d0 + c2 * d2 - c2 * d4 - c1 * d6) * factor;
        let i1_val = Self::get(&self.detrender, 3);

        Self::push_back_bounded(&mut self.q1, q1_val, 4);
        Self::push_back_bounded(&mut self.i1, i1_val, 4);

        // Phase calculation
        let i1_v = Self::get(&self.i1, 0);
        let q1_v = Self::get(&self.q1, 0);
        let phase = if i1_v.is_zero() {
            Decimal::ZERO
        } else {
            // atan(Q1/I1) in degrees — using a rational approximation
            let ratio = q1_v / i1_v;
            // Simple linear approximation of atan in degrees: atan(x) ≈ 45*x for small x
            let forty_five = Decimal::from(45u32);
            forty_five * ratio
        };

        let delta_phase = (self.prev_phase - phase).max(Decimal::ONE);
        self.prev_phase = phase;

        let alpha = (self.fast_limit / delta_phase)
            .max(self.slow_limit)
            .min(self.fast_limit);

        let prev_mama = self.mama.unwrap_or(price);
        let prev_fama = self.fama.unwrap_or(price);

        let new_mama = alpha * price + (Decimal::ONE - alpha) * prev_mama;
        let fama_alpha = Decimal::new(5, 1) * alpha;
        let new_fama = fama_alpha * new_mama + (Decimal::ONE - fama_alpha) * prev_fama;

        self.mama = Some(new_mama);
        self.fama = Some(new_fama);

        Ok(SignalValue::Scalar(new_mama))
    }

    fn reset(&mut self) {
        self.prices.clear();
        self.smooth.clear();
        self.detrender.clear();
        self.i1.clear();
        self.q1.clear();
        self.prev_phase = Decimal::ZERO;
        self.mama = None;
        self.fama = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

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
    fn test_mama_invalid_limits() {
        assert!(MesaAdaptiveMa::new("m", dec!(0), dec!(0.05)).is_err());
        assert!(MesaAdaptiveMa::new("m", dec!(0.5), dec!(0.5)).is_err());
        assert!(MesaAdaptiveMa::new("m", dec!(0.5), dec!(0)).is_err());
    }

    #[test]
    fn test_mama_unavailable_before_seven_bars() {
        let mut m = MesaAdaptiveMa::new("m", dec!(0.5), dec!(0.05)).unwrap();
        for _ in 0..6 {
            assert_eq!(m.update_bar(&bar("100", "98")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!m.is_ready());
    }

    #[test]
    fn test_mama_ready_after_seven_bars() {
        let mut m = MesaAdaptiveMa::new("m", dec!(0.5), dec!(0.05)).unwrap();
        for _ in 0..7 {
            m.update_bar(&bar("100", "98")).unwrap();
        }
        assert!(m.is_ready());
    }

    #[test]
    fn test_mama_fama_accessible() {
        let mut m = MesaAdaptiveMa::new("m", dec!(0.5), dec!(0.05)).unwrap();
        for _ in 0..10 {
            m.update_bar(&bar("100", "98")).unwrap();
        }
        assert!(m.mama().is_some());
        assert!(m.fama().is_some());
    }

    #[test]
    fn test_mama_flat_price_converges() {
        let mut m = MesaAdaptiveMa::new("m", dec!(0.5), dec!(0.05)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..50 {
            last = m.update_bar(&bar("100", "100")).unwrap();
        }
        // After many flat bars, MAMA should converge close to the price
        if let SignalValue::Scalar(v) = last {
            let diff = (v - dec!(100)).abs();
            assert!(diff < dec!(1), "MAMA {v} should be near 100");
        }
    }

    #[test]
    fn test_mama_reset() {
        let mut m = MesaAdaptiveMa::new("m", dec!(0.5), dec!(0.05)).unwrap();
        for _ in 0..10 {
            m.update_bar(&bar("100", "98")).unwrap();
        }
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
        assert!(m.mama().is_none());
        assert!(m.fama().is_none());
    }
}
