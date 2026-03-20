//! TD Sequential Setup Count indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// TD Sequential Setup Count — simplified DeMark TD Sequential phase 1 (Setup).
///
/// The Setup phase counts consecutive bars where `close > close[4]` (buy setup)
/// or `close < close[4]` (sell setup). A completed 9-bar setup signals a potential
/// exhaustion in the prior trend.
///
/// Outputs:
/// - `+n` where n ∈ 1..=9: bar `n` of a **buy** setup (consecutive close > close[-4])
/// - `-n` where n ∈ 1..=9: bar `n` of a **sell** setup (consecutive close < close[-4])
/// - `0`: no active setup
/// - `+9`: a complete buy setup (9 consecutive closes above close[-4])
/// - `-9`: a complete sell setup
///
/// Returns [`SignalValue::Unavailable`] until 5 bars have been accumulated
/// (requires close[4] for comparison).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TdSequential;
/// use fin_primitives::signals::Signal;
///
/// let td = TdSequential::new("td").unwrap();
/// assert_eq!(td.period(), 5);
/// ```
pub struct TdSequential {
    name: String,
    closes: VecDeque<Decimal>,
    buy_count: i32,
    sell_count: i32,
}

impl TdSequential {
    /// Constructs a new `TdSequential`.
    ///
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            closes: VecDeque::with_capacity(5),
            buy_count: 0,
            sell_count: 0,
        })
    }

    /// Returns the current buy setup count (1-9), or 0 if no active buy setup.
    pub fn buy_count(&self) -> i32 {
        self.buy_count
    }

    /// Returns the current sell setup count (1-9), or 0 if no active sell setup.
    pub fn sell_count(&self) -> i32 {
        self.sell_count
    }

    /// Returns `true` if a complete 9-bar buy setup has been achieved.
    pub fn is_buy_setup_complete(&self) -> bool {
        self.buy_count >= 9
    }

    /// Returns `true` if a complete 9-bar sell setup has been achieved.
    pub fn is_sell_setup_complete(&self) -> bool {
        self.sell_count >= 9
    }
}

impl Signal for TdSequential {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        5
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= 5
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > 5 {
            self.closes.pop_front();
        }
        if self.closes.len() < 5 {
            return Ok(SignalValue::Unavailable);
        }

        let current = self.closes[4];
        let prior4  = self.closes[0]; // close 4 bars ago

        if current > prior4 {
            self.buy_count = (self.buy_count + 1).min(9);
            self.sell_count = 0;
            Ok(SignalValue::Scalar(Decimal::from(self.buy_count)))
        } else if current < prior4 {
            self.sell_count = (self.sell_count + 1).min(9);
            self.buy_count = 0;
            Ok(SignalValue::Scalar(-Decimal::from(self.sell_count)))
        } else {
            // Equal close — reset both counts
            self.buy_count  = 0;
            self.sell_count = 0;
            Ok(SignalValue::Scalar(Decimal::ZERO))
        }
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.buy_count  = 0;
        self.sell_count = 0;
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
    fn test_td_unavailable_before_five_bars() {
        let mut td = TdSequential::new("td").unwrap();
        for _ in 0..4 {
            assert_eq!(td.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!td.is_ready());
    }

    #[test]
    fn test_td_buy_setup_counts_up() {
        let mut td = TdSequential::new("td").unwrap();
        // Seed 4 bars at 100
        for _ in 0..4 {
            td.update_bar(&bar("100")).unwrap();
        }
        // Use incrementing prices so current > close[4] on every bar:
        // bar5=101>100, bar6=102>100, bar7=103>100, bar8=104>100,
        // bar9=105>101, bar10=106>102, bar11=107>103, bar12=108>104, bar13=109>105
        let prices = ["101","102","103","104","105","106","107","108","109"];
        for (i, p) in prices.iter().enumerate() {
            let v = td.update_bar(&bar(p)).unwrap();
            let expected = Decimal::from((i + 1) as u32);
            assert_eq!(v, SignalValue::Scalar(expected), "buy count should be {}", i + 1);
        }
        assert!(td.is_buy_setup_complete());
    }

    #[test]
    fn test_td_sell_setup_counts_down() {
        let mut td = TdSequential::new("td").unwrap();
        for _ in 0..4 {
            td.update_bar(&bar("100")).unwrap();
        }
        // Decrementing prices so current < close[4] on every bar
        let prices = ["99","98","97","96","95","94","93","92","91"];
        for (i, p) in prices.iter().enumerate() {
            let v = td.update_bar(&bar(p)).unwrap();
            let expected = -Decimal::from((i + 1) as u32);
            assert_eq!(v, SignalValue::Scalar(expected), "sell count should be -{}", i + 1);
        }
        assert!(td.is_sell_setup_complete());
    }

    #[test]
    fn test_td_buy_count_capped_at_9() {
        let mut td = TdSequential::new("td").unwrap();
        for _ in 0..4 { td.update_bar(&bar("100")).unwrap(); }
        // Push 12 incrementing bars — count caps at 9
        for i in 0u32..12 {
            td.update_bar(&bar(&(101 + i).to_string())).unwrap();
        }
        assert_eq!(td.buy_count(), 9);
    }

    #[test]
    fn test_td_reset() {
        let mut td = TdSequential::new("td").unwrap();
        for _ in 0..10 { td.update_bar(&bar("101")).unwrap(); }
        td.reset();
        assert!(!td.is_ready());
        assert_eq!(td.buy_count(), 0);
        assert_eq!(td.sell_count(), 0);
    }
}
