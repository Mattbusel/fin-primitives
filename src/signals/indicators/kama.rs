//! Kaufman Adaptive Moving Average (KAMA).
//!
//! Adapts its smoothing speed to market noise using the Efficiency Ratio.
//! In trending markets it tracks price closely; in choppy markets it barely moves.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Kaufman Adaptive Moving Average.
///
/// - `ER = |close[n] - close[0]| / Σ|close[i] - close[i-1]|`
/// - `SC = (ER × (fast_sc − slow_sc) + slow_sc)²`
/// - `KAMA[t] = KAMA[t-1] + SC × (close − KAMA[t-1])`
///
/// Uses fast period = 2 and slow period = 30 by default.
/// Returns [`crate::signals::SignalValue::Unavailable`] until `period + 1` bars are seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Kama;
/// use fin_primitives::signals::Signal;
/// let k = Kama::new("kama10", 10).unwrap();
/// assert_eq!(k.period(), 10);
/// assert!(!k.is_ready());
/// ```
pub struct Kama {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    kama: Option<Decimal>,
    fast_sc: Decimal,
    slow_sc: Decimal,
}

impl Kama {
    /// Constructs a new `Kama` with the given name and efficiency-ratio period.
    ///
    /// Uses fast smoothing constant `2/(2+1)` and slow `2/(30+1)`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let fast_sc = Decimal::TWO / Decimal::from(3u32);   // 2/(2+1)
        let slow_sc = Decimal::TWO / Decimal::from(31u32);  // 2/(30+1)
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
            kama: None,
            fast_sc,
            slow_sc,
        })
    }
}

impl Signal for Kama {
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

        // Efficiency ratio
        let first = *self.closes.front().unwrap();
        let last = *self.closes.back().unwrap();
        let direction = (last - first).abs();
        let path: Decimal = self.closes.make_contiguous().windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum();

        let er = if path.is_zero() {
            Decimal::ZERO
        } else {
            direction / path
        };

        // Smoothing constant
        let sc_raw = er * (self.fast_sc - self.slow_sc) + self.slow_sc;
        let sc = sc_raw * sc_raw; // squared

        let kama = match self.kama {
            None => {
                let v = last;
                self.kama = Some(v);
                v
            }
            Some(prev) => {
                let v = prev + sc * (last - prev);
                self.kama = Some(v);
                v
            }
        };
        Ok(SignalValue::Scalar(kama))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.kama = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> BarInput {
        BarInput::from_close(close.parse().unwrap())
    }

    #[test]
    fn test_kama_invalid_period() {
        assert!(Kama::new("k", 0).is_err());
    }

    #[test]
    fn test_kama_unavailable_before_warmup() {
        let mut k = Kama::new("k", 3).unwrap();
        assert!(!k.is_ready());
        k.update(&bar("100")).unwrap();
        k.update(&bar("102")).unwrap();
        let sv = k.update(&bar("104")).unwrap();
        assert!(!k.is_ready());
        assert_eq!(sv, SignalValue::Unavailable);
    }

    #[test]
    fn test_kama_ready_after_period_plus_one_bars() {
        let mut k = Kama::new("k", 3).unwrap();
        for i in 0..4 {
            let p = format!("{}", 100 + i);
            k.update(&bar(&p)).unwrap();
        }
        assert!(k.is_ready());
    }

    #[test]
    fn test_kama_flat_prices_stays_near_price() {
        let mut k = Kama::new("k", 5).unwrap();
        for _ in 0..6 {
            k.update(&bar("100")).unwrap();
        }
        assert!(k.is_ready());
        let sv = k.update(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = sv {
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_kama_reset_clears_state() {
        let mut k = Kama::new("k", 3).unwrap();
        for i in 0..4 {
            let p = format!("{}", 100 + i);
            k.update(&bar(&p)).unwrap();
        }
        assert!(k.is_ready());
        k.reset();
        assert!(!k.is_ready());
    }

    #[test]
    fn test_kama_period_and_name() {
        let k = Kama::new("my_kama", 10).unwrap();
        assert_eq!(k.period(), 10);
        assert_eq!(k.name(), "my_kama");
    }

    #[test]
    fn test_kama_trending_prices_tracks_price() {
        // In a strong trend, ER ≈ 1, so KAMA should closely follow price
        let mut k = Kama::new("k", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..20u32 {
            let p = format!("{}", 100 + i);
            last = k.update(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            // KAMA should be close to the current price (119)
            assert!(v > dec!(100), "kama should have moved up: {}", v);
        } else {
            panic!("expected scalar");
        }
    }
}
