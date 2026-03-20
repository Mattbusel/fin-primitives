//! Gap Fill Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling proportion of bars that filled their opening gap.
///
/// An opening gap is filled when:
/// - Bullish gap: open > prev_close AND current low <= prev_close
/// - Bearish gap: open < prev_close AND current high >= prev_close
///
/// Returns fraction of gap bars (relative to total gap bars in window).
/// Returns 0 when no gaps occurred in the window.
pub struct GapFillRatio {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<(u8, u8)>, // (had_gap, filled_gap)
    gap_count: usize,
    fill_count: usize,
}

impl GapFillRatio {
    /// Creates a new `GapFillRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            gap_count: 0,
            fill_count: 0,
        })
    }
}

impl Signal for GapFillRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (had_gap, filled_gap) = if let Some(pc) = self.prev_close {
            let bullish_gap = bar.open > pc;
            let bearish_gap = bar.open < pc;
            if bullish_gap {
                let filled = if bar.low <= pc { 1u8 } else { 0u8 };
                (1u8, filled)
            } else if bearish_gap {
                let filled = if bar.high >= pc { 1u8 } else { 0u8 };
                (1u8, filled)
            } else {
                (0u8, 0u8)
            }
        } else {
            (0u8, 0u8)
        };
        self.prev_close = Some(bar.close);

        self.window.push_back((had_gap, filled_gap));
        self.gap_count += had_gap as usize;
        self.fill_count += filled_gap as usize;
        if self.window.len() > self.period {
            if let Some((og, of_)) = self.window.pop_front() {
                self.gap_count -= og as usize;
                self.fill_count -= of_ as usize;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.gap_count == 0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let ratio = Decimal::from(self.fill_count as u32) / Decimal::from(self.gap_count as u32);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.gap_count = 0;
        self.fill_count = 0;
    }
    fn name(&self) -> &str { "GapFillRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_gfr_gap_filled() {
        // prev_close=100, open=105 (gap up), low=99 (fills) → 1/1 = 1
        let mut sig = GapFillRatio::new(2).unwrap();
        sig.update(&bar("100", "100", "100", "100")).unwrap(); // sets prev_close=100
        sig.update(&bar("105", "107", "99", "103")).unwrap(); // gap up, filled
        let v = sig.update(&bar("103", "105", "102", "104")).unwrap(); // no gap
        // window=[filled_gap, no_gap], gap_count=1, fill_count=1 → 1.0
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_gfr_no_gaps() {
        // No gaps → ratio = 0
        let mut sig = GapFillRatio::new(2).unwrap();
        sig.update(&bar("100", "105", "98", "100")).unwrap();
        sig.update(&bar("100", "103", "98", "100")).unwrap(); // no gap (open==prev_close)
        let v = sig.update(&bar("100", "102", "98", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
