//! Coral Trend indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Coral Trend — a low-lag adaptive smoothing indicator using an IIR (Infinite
/// Impulse Response) filter originally developed by LazyBear for TradingView.
///
/// The filter smoothes price via a cascade of exponential smoothing stages,
/// producing a line that tracks trends with minimal lag while staying flat in
/// consolidation. The output oscillates above/below a trend threshold:
///
/// - When price is above the Coral line → `+1` (bullish)
/// - When price is below the Coral line → `-1` (bearish)
///
/// The `smoothing_factor` (`sm`) controls the lag:
/// - `sm = 1` → no smoothing (raw price)
/// - `sm = 2` → moderate smoothing
/// - `sm = 21` → corresponds to a ~21-period smooth
///
/// The actual coral value is accessible via [`CoralTrend::coral`].
///
/// Returns [`SignalValue::Scalar`] from the first bar (always ready).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CoralTrend;
/// use fin_primitives::signals::Signal;
///
/// let ct = CoralTrend::new("coral", 21).unwrap();
/// assert_eq!(ct.period(), 21);
/// ```
pub struct CoralTrend {
    name: String,
    period: usize,
    /// EMA multiplier: `k = 2 / (period + 1)`.
    k: Decimal,
    i1: Option<Decimal>,
    i2: Option<Decimal>,
    i3: Option<Decimal>,
    i4: Option<Decimal>,
    i5: Option<Decimal>,
    i6: Option<Decimal>,
    coral: Option<Decimal>,
}

impl CoralTrend {
    /// Constructs a new `CoralTrend` with the given smoothing period.
    ///
    /// `period` must be ≥ 1.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            k,
            i1: None,
            i2: None,
            i3: None,
            i4: None,
            i5: None,
            i6: None,
            coral: None,
        })
    }

    /// Returns the current Coral line value, or `None` if not yet computed.
    pub fn coral(&self) -> Option<Decimal> {
        self.coral
    }

    fn ema_step(prev: Option<Decimal>, input: Decimal, k: Decimal) -> Decimal {
        match prev {
            None => input,
            Some(p) => k * input + (Decimal::ONE - k) * p,
        }
    }
}

impl Signal for CoralTrend {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let price = bar.close;
        let k = self.k;

        let i1 = Self::ema_step(self.i1, price, k);
        let i2 = Self::ema_step(self.i2, i1, k);
        let i3 = Self::ema_step(self.i3, i2, k);
        let i4 = Self::ema_step(self.i4, i3, k);
        let i5 = Self::ema_step(self.i5, i4, k);
        let i6 = Self::ema_step(self.i6, i5, k);

        self.i1 = Some(i1);
        self.i2 = Some(i2);
        self.i3 = Some(i3);
        self.i4 = Some(i4);
        self.i5 = Some(i5);
        self.i6 = Some(i6);

        let coral = i6;
        self.coral = Some(coral);

        // Signal: +1 if price above coral, -1 if below
        let regime = if price > coral {
            Decimal::ONE
        } else if price < coral {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };
        Ok(SignalValue::Scalar(regime))
    }

    fn reset(&mut self) {
        self.i1 = None;
        self.i2 = None;
        self.i3 = None;
        self.i4 = None;
        self.i5 = None;
        self.i6 = None;
        self.coral = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_coral_period_zero_fails() {
        assert!(CoralTrend::new("c", 0).is_err());
    }

    #[test]
    fn test_coral_ready_immediately() {
        let mut ct = CoralTrend::new("c", 10).unwrap();
        ct.update_bar(&bar("100")).unwrap();
        assert!(ct.is_ready());
    }

    #[test]
    fn test_coral_bullish_in_uptrend() {
        let mut ct = CoralTrend::new("c", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..20 {
            last = ct.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        // In a strong uptrend, price should be above coral
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_coral_bearish_in_downtrend() {
        let mut ct = CoralTrend::new("c", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..20 {
            last = ct.update_bar(&bar(&(200 - i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_coral_accessor() {
        let mut ct = CoralTrend::new("c", 3).unwrap();
        ct.update_bar(&bar("100")).unwrap();
        assert!(ct.coral().is_some());
    }

    #[test]
    fn test_coral_flat_price_equals_price() {
        let mut ct = CoralTrend::new("c", 1).unwrap();
        // period=1 → di=0, c1=1, c2=0, c3=0 → coral = price instantly
        for _ in 0..5 {
            ct.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(ct.coral(), Some(dec!(100)));
    }

    #[test]
    fn test_coral_reset() {
        let mut ct = CoralTrend::new("c", 10).unwrap();
        ct.update_bar(&bar("100")).unwrap();
        assert!(ct.coral().is_some());
        ct.reset();
        assert!(ct.coral().is_none());
    }
}
