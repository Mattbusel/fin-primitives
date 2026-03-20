//! Relative Vigor Index (RVI) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Vigor Index over `period` bars.
///
/// RVI measures trend conviction by comparing close-to-open movement against
/// the high-to-low range, both smoothed by a symmetric 4-bar weighted average:
///
/// ```text
/// numerator   = WMA4(close - open)
/// denominator = WMA4(high - low)
/// RVI         = SMA(numerator, period) / SMA(denominator, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until enough bars are accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Rvi;
/// use fin_primitives::signals::Signal;
///
/// let mut rvi = Rvi::new("rvi10", 10).unwrap();
/// assert_eq!(rvi.period(), 10);
/// ```
pub struct Rvi {
    name: String,
    period: usize,
    bars: VecDeque<BarInput>,
    num_sma: VecDeque<Decimal>,
    den_sma: VecDeque<Decimal>,
}

impl Rvi {
    /// Constructs a new `Rvi`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 4`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 4 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            bars: VecDeque::with_capacity(4),
            num_sma: VecDeque::with_capacity(period),
            den_sma: VecDeque::with_capacity(period),
        })
    }

    fn wma4(a: Decimal, b: Decimal, c: Decimal, d: Decimal) -> Decimal {
        // weights: 1,2,3,4 (d is newest)
        (a + b * Decimal::TWO + c * Decimal::from(3u32) + d * Decimal::from(4u32))
            / Decimal::TEN
    }
}

impl Signal for Rvi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars.push_back(*bar);
        if self.bars.len() > 4 {
            self.bars.pop_front();
        }
        if self.bars.len() < 4 {
            return Ok(SignalValue::Unavailable);
        }

        let b: Vec<&BarInput> = self.bars.iter().collect();
        let num = Self::wma4(
            b[0].close - b[0].open,
            b[1].close - b[1].open,
            b[2].close - b[2].open,
            b[3].close - b[3].open,
        );
        let den = Self::wma4(
            b[0].high - b[0].low,
            b[1].high - b[1].low,
            b[2].high - b[2].low,
            b[3].high - b[3].low,
        );

        self.num_sma.push_back(num);
        self.den_sma.push_back(den);
        if self.num_sma.len() > self.period {
            self.num_sma.pop_front();
            self.den_sma.pop_front();
        }
        if self.num_sma.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let num_avg: Decimal = self.num_sma.iter().sum::<Decimal>();
        let den_avg: Decimal = self.den_sma.iter().sum::<Decimal>();
        #[allow(clippy::cast_possible_truncation)]
        let denom = Decimal::from(self.period as u32);
        let den_avg = den_avg / denom;
        let num_avg = num_avg / denom;

        if den_avg == Decimal::ZERO {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(num_avg / den_avg))
    }

    fn is_ready(&self) -> bool {
        self.num_sma.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bars.clear();
        self.num_sma.clear();
        self.den_sma.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rvi_period_too_small() {
        assert!(Rvi::new("r", 3).is_err());
    }

    #[test]
    fn test_rvi_unavailable_before_warmup() {
        let mut rvi = Rvi::new("rvi4", 4).unwrap();
        for _ in 0..6 {
            assert_eq!(rvi.update_bar(&bar("100","110","90","105")).unwrap(), SignalValue::Unavailable);
        }
        // needs 4+4-1 = 7 bars
        assert!(rvi.update_bar(&bar("100","110","90","105")).unwrap().is_scalar());
    }

    #[test]
    fn test_rvi_bullish_bars_positive() {
        let mut rvi = Rvi::new("rvi4", 4).unwrap();
        for _ in 0..10 {
            rvi.update_bar(&bar("90","110","88","108")).unwrap();
        }
        match rvi.update_bar(&bar("90","110","88","108")).unwrap() {
            SignalValue::Scalar(v) => assert!(v > Decimal::ZERO),
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_rvi_reset() {
        let mut rvi = Rvi::new("rvi4", 4).unwrap();
        for _ in 0..15 { rvi.update_bar(&bar("90","110","88","108")).unwrap(); }
        assert!(rvi.is_ready());
        rvi.reset();
        assert!(!rvi.is_ready());
    }
}
