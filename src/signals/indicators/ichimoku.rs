//! Ichimoku Kinkō Hyō cloud indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Ichimoku cloud components returned as a named tuple via `SignalValue::Scalar`
/// for `Tenkan-sen` (the primary scalar output).
///
/// The full Ichimoku system has 5 lines:
/// - **Tenkan-sen** (conversion line): midpoint of last `tenkan` bars
/// - **Kijun-sen** (base line): midpoint of last `kijun` bars
/// - **Senkou Span A** (leading span A): average of Tenkan + Kijun, projected forward
/// - **Senkou Span B** (leading span B): midpoint of last `senkou_b` bars, projected forward
/// - **Chikou span**: close projected `kijun` bars back
///
/// This implementation exposes all lines via separate accessor methods.
/// The `Signal::update()` method returns `Tenkan-sen` as the primary scalar;
/// use the typed accessor [`Ichimoku::lines()`] for the full snapshot.
///
/// Defaults: `tenkan=9`, `kijun=26`, `senkou_b=52`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Ichimoku;
/// use fin_primitives::signals::Signal;
///
/// let ichi = Ichimoku::new("ichi", 9, 26, 52).unwrap();
/// assert_eq!(ichi.period(), 9);
/// ```
pub struct Ichimoku {
    name: String,
    tenkan: usize,
    kijun: usize,
    senkou_b: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    // Store last `senkou_b` bars of highs/lows for Senkou B
    sb_highs: VecDeque<Decimal>,
    sb_lows: VecDeque<Decimal>,
    last_tenkan: Option<Decimal>,
    last_kijun: Option<Decimal>,
    last_span_a: Option<Decimal>,
    last_span_b: Option<Decimal>,
    last_close: Option<Decimal>,
}

/// Snapshot of all Ichimoku lines for a given bar.
#[derive(Debug, Clone)]
pub struct IchimokuLines {
    /// Tenkan-sen (conversion line)
    pub tenkan: Option<Decimal>,
    /// Kijun-sen (base line)
    pub kijun: Option<Decimal>,
    /// Senkou Span A (leading span A)
    pub span_a: Option<Decimal>,
    /// Senkou Span B (leading span B)
    pub span_b: Option<Decimal>,
    /// Close price (Chikou span is close shifted back by kijun bars)
    pub close: Option<Decimal>,
}

impl Ichimoku {
    /// Constructs a new `Ichimoku` with custom periods.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is `0`.
    pub fn new(
        name: impl Into<String>,
        tenkan: usize,
        kijun: usize,
        senkou_b: usize,
    ) -> Result<Self, FinError> {
        if tenkan == 0 { return Err(FinError::InvalidPeriod(tenkan)); }
        if kijun == 0 { return Err(FinError::InvalidPeriod(kijun)); }
        if senkou_b == 0 { return Err(FinError::InvalidPeriod(senkou_b)); }
        let max_window = tenkan.max(kijun).max(senkou_b);
        Ok(Self {
            name: name.into(),
            tenkan,
            kijun,
            senkou_b,
            highs: VecDeque::with_capacity(max_window),
            lows: VecDeque::with_capacity(max_window),
            sb_highs: VecDeque::with_capacity(senkou_b),
            sb_lows: VecDeque::with_capacity(senkou_b),
            last_tenkan: None,
            last_kijun: None,
            last_span_a: None,
            last_span_b: None,
            last_close: None,
        })
    }

    /// Returns the latest Ichimoku line snapshot.
    pub fn lines(&self) -> IchimokuLines {
        IchimokuLines {
            tenkan: self.last_tenkan,
            kijun: self.last_kijun,
            span_a: self.last_span_a,
            span_b: self.last_span_b,
            close: self.last_close,
        }
    }

    fn midpoint(highs: &VecDeque<Decimal>, lows: &VecDeque<Decimal>, n: usize) -> Option<Decimal> {
        if highs.len() < n || lows.len() < n {
            return None;
        }
        let h = highs.iter().rev().take(n).copied().fold(Decimal::MIN, Decimal::max);
        let l = lows.iter().rev().take(n).copied().fold(Decimal::MAX, Decimal::min);
        Some((h + l) / Decimal::TWO)
    }
}

impl Signal for Ichimoku {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Maintain rolling windows
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        let max_window = self.tenkan.max(self.kijun).max(self.senkou_b);
        if self.highs.len() > max_window {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        self.sb_highs.push_back(bar.high);
        self.sb_lows.push_back(bar.low);
        if self.sb_highs.len() > self.senkou_b {
            self.sb_highs.pop_front();
            self.sb_lows.pop_front();
        }

        self.last_close = Some(bar.close);
        self.last_tenkan = Self::midpoint(&self.highs, &self.lows, self.tenkan);
        self.last_kijun = Self::midpoint(&self.highs, &self.lows, self.kijun);
        self.last_span_a = match (self.last_tenkan, self.last_kijun) {
            (Some(t), Some(k)) => Some((t + k) / Decimal::TWO),
            _ => None,
        };
        self.last_span_b = Self::midpoint(&self.sb_highs, &self.sb_lows, self.senkou_b);

        match self.last_tenkan {
            Some(t) => Ok(SignalValue::Scalar(t)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.last_tenkan.is_some()
    }

    fn period(&self) -> usize {
        self.tenkan
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.sb_highs.clear();
        self.sb_lows.clear();
        self.last_tenkan = None;
        self.last_kijun = None;
        self.last_span_a = None;
        self.last_span_b = None;
        self.last_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ichimoku_period_0_fails() {
        assert!(Ichimoku::new("i", 0, 26, 52).is_err());
        assert!(Ichimoku::new("i", 9, 0, 52).is_err());
        assert!(Ichimoku::new("i", 9, 26, 0).is_err());
    }

    #[test]
    fn test_ichimoku_tenkan_flat_price_equals_price() {
        // Flat series: highest high == lowest low → midpoint == price
        let mut ichi = Ichimoku::new("ichi", 3, 5, 7).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = ichi.update_bar(&bar("100", "100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_ichimoku_unavailable_before_tenkan_bars() {
        let mut ichi = Ichimoku::new("ichi", 3, 5, 7).unwrap();
        assert_eq!(ichi.update_bar(&bar("100", "90", "95")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ichi.update_bar(&bar("100", "90", "95")).unwrap(), SignalValue::Unavailable);
        assert!(!ichi.is_ready());
    }

    #[test]
    fn test_ichimoku_lines_accessor() {
        let mut ichi = Ichimoku::new("ichi", 3, 5, 7).unwrap();
        for _ in 0..10 {
            ichi.update_bar(&bar("100", "100", "100")).unwrap();
        }
        let lines = ichi.lines();
        assert!(lines.tenkan.is_some());
        assert!(lines.kijun.is_some());
        assert!(lines.span_a.is_some());
        assert!(lines.span_b.is_some());
    }

    #[test]
    fn test_ichimoku_reset() {
        let mut ichi = Ichimoku::new("ichi", 3, 5, 7).unwrap();
        for _ in 0..10 {
            ichi.update_bar(&bar("100", "100", "100")).unwrap();
        }
        assert!(ichi.is_ready());
        ichi.reset();
        assert!(!ichi.is_ready());
    }
}
