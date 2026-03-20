//! Market Regime Filter indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Market Regime Filter — classifies price action as **trending** (+1),
/// **ranging** (-1), or **transitioning** (0) using the Kaufman Efficiency
/// Ratio (ER) over a rolling window.
///
/// The Efficiency Ratio measures how directional price movement is:
///
/// ```text
/// ER = |close[n] - close[0]| / sum(|close[i] - close[i-1]|)
/// ```
///
/// - `ER > trend_threshold` → trending market → `+1`
/// - `ER < range_threshold` → ranging market  → `-1`
/// - Otherwise → transitioning                → `0`
///
/// Typical thresholds: `trend_threshold = 0.6`, `range_threshold = 0.3`.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MarketRegimeFilter;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let m = MarketRegimeFilter::new("regime", 10, dec!(0.6), dec!(0.3)).unwrap();
/// assert_eq!(m.period(), 10);
/// ```
pub struct MarketRegimeFilter {
    name: String,
    period: usize,
    trend_threshold: Decimal,
    range_threshold: Decimal,
    closes: VecDeque<Decimal>,
    last_er: Option<Decimal>,
}

impl MarketRegimeFilter {
    /// Constructs a new `MarketRegimeFilter`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        trend_threshold: Decimal,
        range_threshold: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            trend_threshold,
            range_threshold,
            closes: VecDeque::with_capacity(period + 1),
            last_er: None,
        })
    }

    /// Returns the most recently computed Efficiency Ratio `[0, 1]`, or `None`.
    pub fn efficiency_ratio(&self) -> Option<Decimal> {
        self.last_er
    }

    /// Returns `true` when the last ER exceeds the trend threshold.
    pub fn is_trending(&self) -> bool {
        self.last_er.map_or(false, |er| er > self.trend_threshold)
    }

    /// Returns `true` when the last ER is below the range threshold.
    pub fn is_ranging(&self) -> bool {
        self.last_er.map_or(false, |er| er < self.range_threshold)
    }
}

impl Signal for MarketRegimeFilter {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let net_move = (self.closes[self.period] - self.closes[0]).abs();
        let path: Decimal = self.closes.iter().zip(self.closes.iter().skip(1))
            .map(|(a, b)| (*b - *a).abs())
            .sum();

        let er = if path.is_zero() {
            Decimal::ZERO
        } else {
            net_move.checked_div(path).unwrap_or(Decimal::ZERO)
        };

        self.last_er = Some(er);

        let regime = if er > self.trend_threshold {
            Decimal::ONE
        } else if er < self.range_threshold {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };

        Ok(SignalValue::Scalar(regime))
    }

    fn is_ready(&self) -> bool {
        self.last_er.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.last_er = None;
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
    fn test_regime_period_zero_fails() {
        assert!(MarketRegimeFilter::new("r", 0, dec!(0.6), dec!(0.3)).is_err());
    }

    #[test]
    fn test_regime_unavailable_before_period() {
        let mut m = MarketRegimeFilter::new("r", 3, dec!(0.6), dec!(0.3)).unwrap();
        for _ in 0..3 {
            assert_eq!(m.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!m.is_ready());
    }

    #[test]
    fn test_regime_trending_market() {
        let mut m = MarketRegimeFilter::new("r", 5, dec!(0.6), dec!(0.3)).unwrap();
        // Strongly trending: straight line up → ER = 1.0
        for i in 0..6u32 {
            m.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        let v = m.update_bar(&bar("106")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
        assert!(m.is_trending());
        assert!(!m.is_ranging());
    }

    #[test]
    fn test_regime_ranging_market() {
        let mut m = MarketRegimeFilter::new("r", 4, dec!(0.6), dec!(0.3)).unwrap();
        // Oscillating market: net move ≈ 0, large path
        let prices = ["100", "110", "90", "110", "100"];
        for p in &prices {
            m.update_bar(&bar(p)).unwrap();
        }
        let v = m.update_bar(&bar("100")).unwrap();
        // Net move ~0, path is large → ER near 0 → ranging
        assert!(matches!(v, SignalValue::Scalar(_)));
        assert!(m.is_ranging());
    }

    #[test]
    fn test_regime_reset() {
        let mut m = MarketRegimeFilter::new("r", 3, dec!(0.6), dec!(0.3)).unwrap();
        for i in 0..4u32 {
            m.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
        assert!(m.efficiency_ratio().is_none());
    }

    #[test]
    fn test_regime_er_accessible() {
        let mut m = MarketRegimeFilter::new("r", 2, dec!(0.6), dec!(0.3)).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("110")).unwrap();
        m.update_bar(&bar("120")).unwrap();
        // Straight up → ER = 1.0
        let er = m.efficiency_ratio().unwrap();
        assert_eq!(er, dec!(1));
    }
}
