//! Elder's Force Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Elder's Force Index.
///
/// Measures the force (power) behind a price move by combining price change with volume:
///
/// ```text
/// raw_fi     = (close - prev_close) × volume
/// ForceIndex = EMA(raw_fi, period)
/// ```
///
/// Positive values indicate buying pressure; negative values indicate selling pressure.
/// Large values signal strong directional conviction.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ForceIndex;
/// use fin_primitives::signals::Signal;
/// let fi = ForceIndex::new("fi13", 13).unwrap();
/// assert_eq!(fi.period(), 13);
/// ```
pub struct ForceIndex {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    ema: Option<Decimal>,
    multiplier: Decimal,
    bar_count: usize,
}

impl ForceIndex {
    /// Constructs a new `ForceIndex` indicator.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO
            .checked_div(Decimal::from(period as u32) + Decimal::ONE)
            .unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            ema: None,
            multiplier: k,
            bar_count: 0,
        })
    }
}

impl Signal for ForceIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let volume = bar.volume;

        if let Some(prev) = self.prev_close {
            let raw_fi = (close - prev) * volume;
            self.bar_count += 1;

            self.ema = Some(match self.ema {
                None => raw_fi,
                Some(prev_ema) => {
                    let diff = raw_fi - prev_ema;
                    prev_ema
                        + self
                            .multiplier
                            .checked_mul(diff)
                            .ok_or(FinError::ArithmeticOverflow)?
                }
            });
        }
        self.prev_close = Some(close);

        if self.bar_count >= self.period {
            Ok(SignalValue::Scalar(self.ema.unwrap()))
        } else {
            Ok(SignalValue::Unavailable)
        }
    }

    fn is_ready(&self) -> bool {
        self.bar_count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.ema = None;
        self.bar_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str, volume: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        let v = Quantity::new(volume.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_force_index_period_zero_fails() {
        assert!(ForceIndex::new("fi", 0).is_err());
    }

    #[test]
    fn test_force_index_unavailable_before_warmup() {
        let mut fi = ForceIndex::new("fi2", 2).unwrap();
        // First bar — no prev_close yet
        assert_eq!(fi.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
        // Second bar — bar_count=1, period=2, still not ready
        assert_eq!(fi.update_bar(&bar("101", "1000")).unwrap(), SignalValue::Unavailable);
        assert!(!fi.is_ready());
    }

    #[test]
    fn test_force_index_ready_after_warmup() {
        let mut fi = ForceIndex::new("fi1", 1).unwrap();
        fi.update_bar(&bar("100", "1000")).unwrap();
        let v = fi.update_bar(&bar("102", "1000")).unwrap();
        // raw_fi = (102-100)*1000 = 2000; EMA(1) = 2000
        assert_eq!(v, SignalValue::Scalar(dec!(2000)));
        assert!(fi.is_ready());
    }

    #[test]
    fn test_force_index_negative_on_price_drop() {
        let mut fi = ForceIndex::new("fi1", 1).unwrap();
        fi.update_bar(&bar("100", "500")).unwrap();
        let v = fi.update_bar(&bar("98", "500")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val < Decimal::ZERO, "price drop should produce negative force index");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_force_index_reset_clears_state() {
        let mut fi = ForceIndex::new("fi1", 1).unwrap();
        fi.update_bar(&bar("100", "1000")).unwrap();
        fi.update_bar(&bar("105", "1000")).unwrap();
        assert!(fi.is_ready());
        fi.reset();
        assert!(!fi.is_ready());
        assert_eq!(fi.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_force_index_zero_volume_gives_zero() {
        let mut fi = ForceIndex::new("fi1", 1).unwrap();
        fi.update_bar(&bar("100", "1000")).unwrap();
        let v = fi.update_bar(&bar("110", "0")).unwrap();
        // raw_fi = (110-100)*0 = 0; EMA becomes 0
        assert_eq!(v, SignalValue::Scalar(Decimal::ZERO));
    }
}
