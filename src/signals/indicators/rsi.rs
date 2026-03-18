//! Relative Strength Index (RSI) indicator.

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::{Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Strength Index over `period` bars using Wilder's smoothing.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been processed
/// (one extra bar is needed to compute the first price change).
///
/// Wilder's algorithm:
/// 1. Seed: simple average of the first `period` gains/losses.
/// 2. Every subsequent bar: `avg = (avg * (period - 1) + new_value) / period`.
///
/// The previous implementation used a rolling SMA window recomputed each bar,
/// which produces different values than the Wilder EMA used by every real RSI
/// implementation (e.g. TradingView, Bloomberg, MetaTrader).
///
/// Result is always in `[0, 100]`.
pub struct Rsi {
    name: String,
    period: usize,
    /// Seed accumulator — collects raw gains for the first `period` bars.
    seed_gains: VecDeque<Decimal>,
    /// Seed accumulator — collects raw losses for the first `period` bars.
    seed_losses: VecDeque<Decimal>,
    /// Wilder smoothed average gain (None until seed phase complete).
    avg_gain: Option<Decimal>,
    /// Wilder smoothed average loss (None until seed phase complete).
    avg_loss: Option<Decimal>,
    prev_close: Option<Decimal>,
    count: usize,
}

impl Rsi {
    /// Constructs a new `Rsi` with the given name and period.
    pub fn new(name: impl Into<String>, period: usize) -> Self {
        Self {
            name: name.into(),
            period,
            seed_gains: VecDeque::with_capacity(period),
            seed_losses: VecDeque::with_capacity(period),
            avg_gain: None,
            avg_loss: None,
            prev_close: None,
            count: 0,
        }
    }
}

impl Signal for Rsi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError> {
        let close = bar.close.value();

        if let Some(prev) = self.prev_close {
            let change = close - prev;
            let (gain, loss) = if change >= Decimal::ZERO {
                (change, Decimal::ZERO)
            } else {
                (Decimal::ZERO, change.abs())
            };

            let period_dec = Decimal::from(self.period as u32);

            if self.count < self.period {
                // Seed phase: accumulate raw values.
                self.seed_gains.push_back(gain);
                self.seed_losses.push_back(loss);
                self.count += 1;

                if self.count == self.period {
                    // Initialise with simple average of seed window.
                    let sum_g: Decimal = self.seed_gains.iter().copied().sum();
                    let sum_l: Decimal = self.seed_losses.iter().copied().sum();
                    self.avg_gain = Some(
                        sum_g.checked_div(period_dec).ok_or(FinError::ArithmeticOverflow)?,
                    );
                    self.avg_loss = Some(
                        sum_l.checked_div(period_dec).ok_or(FinError::ArithmeticOverflow)?,
                    );
                }
            } else {
                // Smoothing phase: Wilder EMA — avg = (avg*(period-1) + new) / period.
                let prev_gain = self.avg_gain.ok_or(FinError::ArithmeticOverflow)?;
                let prev_loss = self.avg_loss.ok_or(FinError::ArithmeticOverflow)?;
                let period_m1 = Decimal::from((self.period - 1) as u32);
                self.avg_gain = Some(
                    (prev_gain * period_m1 + gain)
                        .checked_div(period_dec)
                        .ok_or(FinError::ArithmeticOverflow)?,
                );
                self.avg_loss = Some(
                    (prev_loss * period_m1 + loss)
                        .checked_div(period_dec)
                        .ok_or(FinError::ArithmeticOverflow)?,
                );
                self.count += 1;
            }
        }

        self.prev_close = Some(close);

        let (avg_gain, avg_loss) = match (self.avg_gain, self.avg_loss) {
            (Some(g), Some(l)) => (g, l),
            _ => return Ok(SignalValue::Unavailable),
        };

        if avg_loss == Decimal::ZERO {
            // All gains, no losses → RSI = 100
            return Ok(SignalValue::Scalar(Decimal::ONE_HUNDRED));
        }

        let rs = avg_gain
            .checked_div(avg_loss)
            .ok_or(FinError::ArithmeticOverflow)?;
        let rsi = Decimal::ONE_HUNDRED
            - Decimal::ONE_HUNDRED
                .checked_div(Decimal::ONE + rs)
                .ok_or(FinError::ArithmeticOverflow)?;

        // Clamp to [0, 100] to guard against precision edge cases.
        let rsi = rsi.max(Decimal::ZERO).min(Decimal::ONE_HUNDRED);
        Ok(SignalValue::Scalar(rsi))
    }

    fn is_ready(&self) -> bool {
        self.avg_gain.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rsi_not_ready_before_period() {
        let mut rsi = Rsi::new("rsi3", 3);
        rsi.update(&bar("100")).unwrap();
        let v = rsi.update(&bar("105")).unwrap();
        assert!(matches!(v, SignalValue::Unavailable));
        assert!(!rsi.is_ready());
    }

    #[test]
    fn test_rsi_value_in_range_0_to_100() {
        let mut rsi = Rsi::new("rsi3", 3);
        let prices = ["100", "102", "101", "103", "105"];
        let mut last_val = Decimal::ZERO;
        for p in &prices {
            if let SignalValue::Scalar(v) = rsi.update(&bar(p)).unwrap() {
                last_val = v;
            }
        }
        assert!(last_val >= Decimal::ZERO);
        assert!(last_val <= Decimal::ONE_HUNDRED);
    }

    #[test]
    fn test_rsi_all_gains_returns_100() {
        let mut rsi = Rsi::new("rsi3", 3);
        // Monotonically increasing → RSI should be 100.
        rsi.update(&bar("100")).unwrap();
        rsi.update(&bar("110")).unwrap();
        rsi.update(&bar("120")).unwrap();
        let v = rsi.update(&bar("130")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rsi_is_ready_after_period_plus_one() {
        let mut rsi = Rsi::new("rsi3", 3);
        rsi.update(&bar("100")).unwrap();
        rsi.update(&bar("101")).unwrap();
        rsi.update(&bar("102")).unwrap();
        assert!(!rsi.is_ready());
        rsi.update(&bar("103")).unwrap();
        assert!(rsi.is_ready());
    }

    #[test]
    fn test_rsi_overbought_at_70() {
        // Feed a period-14 RSI with 14 consecutive up-moves of equal size.
        // All changes are gains → avg_loss == 0 → RSI == 100, which is >= 70.
        let mut rsi = Rsi::new("rsi14", 14);
        // 15 bars: one to set prev_close, then 14 up moves filling the seed window.
        // After bar 15 the seed average is computed (all gains) → RSI = 100.
        for i in 0u32..=14 {
            rsi.update(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(rsi.is_ready(), "RSI should be ready after period+1 bars");
        let v = rsi.update(&bar("115")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val >= dec!(70), "all-up RSI should be >= 70, got {val}");
        } else {
            panic!("expected Scalar, got Unavailable");
        }
    }

    #[test]
    fn test_ema_faster_than_sma() {
        // After a sharp price spike, EMA should be closer to the spike than SMA
        // because EMA weights recent values more heavily.
        use crate::signals::indicators::Sma;

        let period = 5;
        let mut ema = Ema::new("ema5", period);
        let mut sma = Sma::new("sma5", period);

        // Seed both with stable prices at 100.
        for _ in 0..period {
            ema.update(&bar("100")).unwrap();
            sma.update(&bar("100")).unwrap();
        }

        // Feed a large spike.
        let spike_bar = bar("200");
        let ema_val = match ema.update(&spike_bar).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("EMA should be ready"),
        };
        let sma_val = match sma.update(&spike_bar).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("SMA should be ready"),
        };

        // EMA gives more weight to the new value, so it should be higher than SMA.
        assert!(
            ema_val > sma_val,
            "EMA ({ema_val}) should be higher than SMA ({sma_val}) immediately after a spike"
        );
    }

    #[test]
    fn test_rsi_mixed_values_bounded() {
        let mut rsi = Rsi::new("rsi14", 14);
        let prices = [
            "44.34", "44.09", "44.15", "43.61", "44.33", "44.83", "45.10", "45.15",
            "43.61", "44.33", "44.83", "45.10", "45.15", "43.61", "44.33",
        ];
        let mut val = SignalValue::Unavailable;
        for p in &prices {
            val = rsi.update(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = val {
            assert!(v >= Decimal::ZERO, "RSI below 0: {v}");
            assert!(v <= Decimal::ONE_HUNDRED, "RSI above 100: {v}");
        }
    }
}
