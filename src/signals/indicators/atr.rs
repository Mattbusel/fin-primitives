//! Average True Range (ATR) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Average True Range over `period` bars: Wilder's ATR, as TA-Lib's `ATR` computes it.
///
/// True Range for each bar is: `max(high - low, |high - prev_close|, |low - prev_close|)`.
/// The first bar only sets the previous close. The true ranges of the next `period` bars
/// are averaged to seed the ATR, and every later bar applies Wilder smoothing:
/// `atr = (prev_atr * (period - 1) + true_range) / period`.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been seen.
///
/// Since 4.0 [`Atr::new`] is Wilder's ATR. Before 4.0 it was a simple moving average of
/// the true range, which never converges to the ATR that TA-Lib, pandas-ta and charting
/// platforms show; that average is still available as [`Atr::sma`].
///
/// ATR is commonly used for volatility measurement, position sizing, and stop-loss placement.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Atr;
/// use fin_primitives::signals::Signal;
/// let atr = Atr::new("atr_14", 14).unwrap();
/// assert_eq!(atr.period(), 14);
/// assert!(atr.is_wilder());
/// ```
pub struct Atr {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    tr_values: VecDeque<Decimal>,
    /// `Some` for Wilder smoothing: the current smoothed value once seeded.
    wilder: Option<Option<Decimal>>,
}

impl Atr {
    /// Constructs Wilder's ATR (TA-Lib's `ATR`) with the given name and period.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        let mut atr = Self::sma(name, period)?;
        atr.wilder = Some(None);
        Ok(atr)
    }

    /// Constructs a simple moving average of the true range over `period` bars.
    ///
    /// This was [`Atr::new`] before 4.0. It is not Wilder's ATR: it does not converge to
    /// the value TA-Lib or a charting platform shows, and it jumps when a large bar leaves
    /// the window. Use it only to reproduce an existing system that averages this way.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn sma(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            tr_values: VecDeque::with_capacity(period),
            wilder: None,
        })
    }

    /// Constructs Wilder's ATR, which matches TA-Lib's `ATR`. The same as [`Atr::new`]
    /// since 4.0; kept so code written for 3.x keeps compiling.
    ///
    /// The first bar only sets the previous close. The true ranges of the next
    /// `period` bars are averaged to seed the ATR (so the first value arrives on bar
    /// `period + 1`), and every later bar applies Wilder
    /// smoothing: `atr = (prev_atr * (period - 1) + true_range) / period`.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    ///
    /// # Example
    /// ```rust
    /// use fin_primitives::signals::indicators::Atr;
    /// use fin_primitives::signals::{BarInput, Signal};
    /// use rust_decimal_macros::dec;
    ///
    /// let mut atr = Atr::wilder("atr", 2).unwrap();
    /// let bar = |h, l, c| BarInput::new(c, h, l, c, dec!(0));
    /// atr.update(&bar(dec!(10), dec!(8), dec!(9))).unwrap();   // sets prev close
    /// atr.update(&bar(dec!(11), dec!(9), dec!(10))).unwrap();  // TR 2
    /// let seeded = atr.update(&bar(dec!(12), dec!(8), dec!(11))).unwrap(); // TR 4
    /// assert_eq!(seeded.as_decimal(), Some(dec!(3)));
    /// let next = atr.update(&bar(dec!(12), dec!(11), dec!(11))).unwrap(); // TR 1
    /// assert_eq!(next.as_decimal(), Some(dec!(2)));
    /// ```
    pub fn wilder(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        Self::new(name, period)
    }

    /// `true` if this ATR uses Wilder smoothing (every constructor except [`Atr::sma`]).
    pub fn is_wilder(&self) -> bool {
        self.wilder.is_some()
    }
}

impl Signal for Atr {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let true_range = match self.prev_close {
            None => {
                // First bar: no previous close, use high - low only.
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(prev) => bar.true_range(Some(prev)),
        };

        self.prev_close = Some(bar.close);

        #[allow(clippy::cast_possible_truncation)]
        let period_d = Decimal::from(self.period as u32);
        if let Some(Some(prev_atr)) = self.wilder {
            let atr = prev_atr
                .checked_mul(period_d - Decimal::ONE)
                .and_then(|v| v.checked_add(true_range))
                .and_then(|v| v.checked_div(period_d))
                .ok_or(FinError::ArithmeticOverflow)?;
            self.wilder = Some(Some(atr));
            return Ok(SignalValue::Scalar(atr));
        }

        self.tr_values.push_back(true_range);
        if self.tr_values.len() > self.period {
            self.tr_values.pop_front();
        }

        if self.tr_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.tr_values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let atr = sum
            .checked_div(period_d)
            .ok_or(FinError::ArithmeticOverflow)?;
        if self.wilder.is_some() {
            // Seeded: from here on only the smoothed value is needed.
            self.wilder = Some(Some(atr));
            self.tr_values.clear();
        }
        Ok(SignalValue::Scalar(atr))
    }

    fn is_ready(&self) -> bool {
        matches!(self.wilder, Some(Some(_))) || self.tr_values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.tr_values.clear();
        if self.wilder.is_some() {
            self.wilder = Some(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_atr_unavailable_on_first_bar() {
        let mut atr = Atr::new("atr1", 1).unwrap();
        let v = atr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!atr.is_ready());
    }

    #[test]
    fn test_atr_period_1_ready_after_two_bars() {
        let mut atr = Atr::new("atr1", 1).unwrap();
        atr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        let v = atr.update_bar(&bar("100", "115", "95", "110")).unwrap();
        // TR = max(115-95, |115-100|, |95-100|) = max(20, 15, 5) = 20
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
        assert!(atr.is_ready());
    }

    #[test]
    fn test_atr_uses_prev_close_for_gap() {
        // Simulate a gap: prev close = 100, new bar opens and trades much higher
        let mut atr = Atr::new("atr1", 1).unwrap();
        atr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        // Gap up: bar trades 120-115, but prev close = 100
        // TR = max(120-115, |120-100|, |115-100|) = max(5, 20, 15) = 20
        let v = atr.update_bar(&bar("118", "120", "115", "119")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_atr_period_0_fails() {
        assert!(Atr::new("atr0", 0).is_err());
    }

    #[test]
    fn test_atr_period_3_averages_true_ranges() {
        let mut atr = Atr::sma("atr3", 3).unwrap();
        // Bar 0: establishes prev_close = 100, no TR
        atr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        // Bar 1: TR = max(120-80, |120-100|, |80-100|) = max(40, 20, 20) = 40
        atr.update_bar(&bar("100", "120", "80", "110")).unwrap();
        // Bar 2: TR = max(115-105, |115-110|, |105-110|) = max(10, 5, 5) = 10
        atr.update_bar(&bar("112", "115", "105", "108")).unwrap();
        // Bar 3: TR = max(112-100, |112-108|, |100-108|) = max(12, 4, 8) = 12
        let v = atr.update_bar(&bar("110", "112", "100", "105")).unwrap();
        // ATR(3) = (40 + 10 + 12) / 3 = 20.666...
        if let SignalValue::Scalar(atr_val) = v {
            let expected = dec!(62) / dec!(3);
            assert_eq!(atr_val, expected);
        } else {
            panic!("expected Scalar after period bars");
        }
    }

    #[test]
    fn test_atr_new_is_wilder_and_sma_is_not() {
        let bars = [
            bar("100", "110", "90", "100"),
            bar("100", "120", "80", "110"),  // TR 40
            bar("112", "115", "105", "108"), // TR 10
            bar("110", "112", "100", "105"), // TR 12: seed (40 + 10 + 12) / 3
            bar("105", "109", "103", "108"), // TR 6
        ];
        let mut w = Atr::new("w", 3).unwrap();
        let mut s = Atr::sma("s", 3).unwrap();
        let (mut wv, mut sv) = (SignalValue::Unavailable, SignalValue::Unavailable);
        for b in &bars {
            wv = w.update_bar(b).unwrap();
            sv = s.update_bar(b).unwrap();
        }
        assert!(w.is_wilder() && !s.is_wilder());
        // Wilder: (62/3 * 2 + 6) / 3; simple average: (10 + 12 + 6) / 3.
        assert_eq!(
            wv,
            SignalValue::Scalar((dec!(62) / dec!(3) * dec!(2) + dec!(6)) / dec!(3))
        );
        assert_eq!(sv, SignalValue::Scalar(dec!(28) / dec!(3)));
    }

    #[test]
    fn test_atr_unavailable_before_period_filled() {
        let mut atr = Atr::new("atr3", 3).unwrap();
        atr.update_bar(&bar("100", "110", "90", "100")).unwrap(); // bar 0, no TR
        let v1 = atr.update_bar(&bar("100", "120", "80", "110")).unwrap(); // TR=40, 1 of 3
        let v2 = atr.update_bar(&bar("112", "115", "105", "108")).unwrap(); // TR=10, 2 of 3
        assert_eq!(v1, SignalValue::Unavailable);
        assert_eq!(v2, SignalValue::Unavailable);
        assert!(!atr.is_ready());
    }
}
