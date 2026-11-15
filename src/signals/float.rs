//! The `f64` fast path: native floating-point versions of the most used indicators.
//!
//! The indicators in [`signals::indicators`](crate::signals::indicators) compute in exact
//! [`Decimal`](rust_decimal::Decimal) arithmetic. That is the right default for prices,
//! but a codebase that already stores bars as `f64` would have to convert every value in
//! and every result out. The types here take and return plain `f64` and never touch
//! `Decimal`, so they drop into float code directly and run at float speed.
//!
//! Each type runs the same algorithm as its `Decimal` counterpart (same warm-up, same
//! seeding, same edge-case values), and the main indicators have a constructor that
//! returns it, so switching is one call:
//!
//! | `Decimal` indicator | `f64` constructor | returns |
//! |---|---|---|
//! | [`Sma`] | [`Sma::new_f64`] | [`SmaF64`] |
//! | [`Ema`] | [`Ema::new_f64`], [`Ema::wilder_f64`] | [`EmaF64`] |
//! | [`Rsi`] | [`Rsi::new_f64`] | [`RsiF64`] |
//! | [`Macd`] | [`Macd::new_f64`] | [`MacdF64`] |
//! | [`BollingerB`] | [`BollingerB::new_f64`] | [`BollingerF64`] |
//! | [`Atr`] | [`Atr::new_f64`], [`Atr::wilder_f64`] | [`AtrF64`] |
//! | [`StdDev`] | [`StdDev::new_f64`] | [`StdDevF64`] |
//! | [`StochasticK`] | [`StochasticK::new_f64`] | [`StochasticF64`] |
//! | [`Roc`] | [`Roc::new_f64`] | [`RocF64`] |
//! | [`Obv`] | [`Obv::new_f64`] | [`ObvF64`] |
//! | [`Mfi`] | [`Mfi::new_f64`] | [`MfiF64`] |
//! | [`Cci`] | [`Cci::new_f64`] | [`CciF64`] |
//! | [`EfficiencyRatio`] | [`EfficiencyRatio::new_f64`] | [`EfficiencyRatioF64`] |
//! | [`Ppo`] | [`Ppo::new_f64`] | [`PpoF64`] |
//! | [`RollingMAD`] | [`RollingMAD::new_f64`] | [`MeanAbsDevF64`] |
//!
//! [`TrueRangeF64`], [`MaxF64`] and [`MinF64`] are building blocks with no `Decimal`
//! twin. [`MacdF64::talib`] follows TA-Lib where [`Macd`] does something else: TA-Lib
//! seeds MACD's fast EMA on the `fast` closes that end where the slow EMA's seed ends.
//! [`PpoF64::talib`] takes TA-Lib's parameter rules (swapped or equal periods); its
//! values are the same as [`PpoF64::new`], because TA-Lib's `PPO` seeds both EMAs from
//! the first bar.
//!
//! ## Conventions
//!
//! - `update` returns `None` during warm-up and `Some(value)` afterwards (indicators
//!   that are ready from the first bar, such as [`ObvF64`] and [`TrueRangeF64`], return
//!   a bare `f64`).
//! - Inputs are not validated: a NaN or infinite input propagates into the outputs that
//!   depend on it, as it would in any float code. Rolling sums recover once the bad
//!   value leaves the window.
//! - Results agree with the `Decimal` indicators to float precision (the tests check
//!   1e-9 relative), not digit for digit. When you need exact decimal results, use the
//!   `Decimal` types.
//!
//! # Example
//! ```rust
//! use fin_primitives::signals::indicators::Rsi;
//!
//! let mut rsi = Rsi::new_f64(14).unwrap();
//! let closes = [44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42,
//!               45.84, 46.08, 45.89, 46.03, 45.61, 46.28, 46.28];
//! let mut last = None;
//! for c in closes {
//!     last = rsi.update(c);
//! }
//! let v = last.unwrap();
//! assert!((v - 70.464).abs() < 1e-3);
//! ```

use crate::error::FinError;
use crate::signals::indicators::{
    Atr, BollingerB, Cci, EfficiencyRatio, Ema, Macd, Mfi, Obv, Ppo, RollingMAD, Roc, Rsi, Sma,
    StdDev, StochasticK,
};
use std::collections::VecDeque;

fn check_period(period: usize) -> Result<(), FinError> {
    if period == 0 {
        Err(FinError::InvalidPeriod(period))
    } else {
        Ok(())
    }
}

#[allow(clippy::cast_precision_loss)]
fn as_f64(n: usize) -> f64 {
    n as f64
}

/// A fixed-length window with a running sum that is rebuilt whenever it stops being
/// finite, so one NaN or infinity does not poison the sum after it leaves the window.
#[derive(Debug, Clone)]
struct SumWindow {
    period: usize,
    values: VecDeque<f64>,
    sum: f64,
}

impl SumWindow {
    fn new(period: usize) -> Self {
        Self { period, values: VecDeque::with_capacity(period + 1), sum: 0.0 }
    }

    fn push(&mut self, value: f64) {
        self.values.push_back(value);
        self.sum += value;
        if self.values.len() > self.period {
            if let Some(old) = self.values.pop_front() {
                self.sum -= old;
            }
            if !self.sum.is_finite() {
                self.sum = self.values.iter().sum();
            }
        }
    }

    fn is_full(&self) -> bool {
        self.values.len() >= self.period
    }

    fn mean(&self) -> f64 {
        self.sum / as_f64(self.period)
    }

    fn clear(&mut self) {
        self.values.clear();
        self.sum = 0.0;
    }
}

// ── SMA ──────────────────────────────────────────────────────────────────────

/// Simple moving average of the last `period` values, in `f64`.
///
/// The `f64` twin of [`Sma`]. `None` until `period` values have been seen.
#[derive(Debug, Clone)]
pub struct SmaF64 {
    window: SumWindow,
}

impl SmaF64 {
    /// Creates an SMA over `period` values.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { window: SumWindow::new(period) })
    }

    /// Feeds one value and returns the average once the window is full.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        self.window.push(value);
        self.window.is_full().then(|| self.window.mean())
    }

    /// `true` once `period` values have been seen.
    pub fn is_ready(&self) -> bool {
        self.window.is_full()
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.window.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.window.clear();
    }
}

// ── EMA ──────────────────────────────────────────────────────────────────────

/// Exponential moving average in `f64`, seeded with the SMA of the first `period`
/// values (the TA-Lib convention).
///
/// The `f64` twin of [`Ema`]: [`EmaF64::new`] uses `k = 2 / (period + 1)` and
/// [`EmaF64::wilder`] uses Wilder's `k = 1 / period`. `None` until `period` values
/// have been seen.
#[derive(Debug, Clone)]
pub struct EmaF64 {
    period: usize,
    k: f64,
    count: usize,
    seed_sum: f64,
    value: Option<f64>,
}

impl EmaF64 {
    fn with_k(period: usize, k: f64) -> Self {
        Self { period, k, count: 0, seed_sum: 0.0, value: None }
    }

    /// Creates an EMA with the standard smoothing factor `2 / (period + 1)`.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self::with_k(period, 2.0 / as_f64(period + 1)))
    }

    /// Creates an EMA with Wilder's smoothing factor `1 / period` (the smoothing in
    /// RSI, ATR and ADX).
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn wilder(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self::with_k(period, 1.0 / as_f64(period)))
    }

    /// Feeds one value and returns the EMA once seeded.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        match self.value {
            None => {
                self.seed_sum += value;
                self.count += 1;
                if self.count >= self.period {
                    self.value = Some(self.seed_sum / as_f64(self.period));
                }
            }
            Some(prev) => self.value = Some((value - prev) * self.k + prev),
        }
        self.value
    }

    /// The current value, if seeded.
    pub fn value(&self) -> Option<f64> {
        self.value
    }

    /// The smoothing factor `k`.
    pub fn k(&self) -> f64 {
        self.k
    }

    /// `true` once the SMA seed is complete.
    pub fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    /// The period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.count = 0;
        self.seed_sum = 0.0;
        self.value = None;
    }
}

// ── RSI ──────────────────────────────────────────────────────────────────────

/// Wilder's Relative Strength Index in `f64`.
///
/// The `f64` twin of [`Rsi`]: the first bar sets the previous close, the next
/// `period` changes seed the average gain and loss with a simple mean, and later bars
/// apply Wilder smoothing. This matches TA-Lib's `RSI`. `None` until `period + 1`
/// closes have been seen. When the average loss is zero the RSI is 100 (as in
/// [`Rsi`]; TA-Lib returns 0 when both averages are zero).
#[derive(Debug, Clone)]
pub struct RsiF64 {
    period: usize,
    prev: Option<f64>,
    count: usize,
    gain_sum: f64,
    loss_sum: f64,
    avg: Option<(f64, f64)>,
}

impl RsiF64 {
    /// Creates an RSI over `period` changes.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { period, prev: None, count: 0, gain_sum: 0.0, loss_sum: 0.0, avg: None })
    }

    fn rsi(avg_gain: f64, avg_loss: f64) -> f64 {
        if avg_loss == 0.0 {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
        }
    }

    /// Feeds one close and returns the RSI once seeded.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        let prev = self.prev.replace(close)?;
        let change = close - prev;
        let gain = if change > 0.0 { change } else { 0.0 };
        let loss = if change < 0.0 { -change } else { 0.0 };
        let p = as_f64(self.period);
        let (ag, al) = match self.avg {
            None => {
                self.gain_sum += gain;
                self.loss_sum += loss;
                self.count += 1;
                if self.count < self.period {
                    return None;
                }
                (self.gain_sum / p, self.loss_sum / p)
            }
            Some((ag, al)) => ((ag * (p - 1.0) + gain) / p, (al * (p - 1.0) + loss) / p),
        };
        self.avg = Some((ag, al));
        Some(Self::rsi(ag, al))
    }

    /// `true` once `period + 1` closes have been seen.
    pub fn is_ready(&self) -> bool {
        self.avg.is_some()
    }

    /// The period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.prev = None;
        self.count = 0;
        self.gain_sum = 0.0;
        self.loss_sum = 0.0;
        self.avg = None;
    }
}

// ── MACD ─────────────────────────────────────────────────────────────────────

/// The three MACD lines for one bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MacdOutputF64 {
    /// Fast EMA minus slow EMA.
    pub macd: f64,
    /// EMA of the MACD line over the signal period.
    pub signal: f64,
    /// MACD line minus signal line.
    pub histogram: f64,
}

/// MACD in `f64`: `EMA(fast) - EMA(slow)`, its signal EMA, and the histogram.
///
/// [`MacdF64::new`] is the `f64` twin of [`Macd`] (both EMAs seeded from the first
/// bar). [`MacdF64::talib`] reproduces TA-Lib's `MACD`, which seeds the fast EMA on the
/// `fast` closes that end where the slow EMA's seed ends, so both lines start on the
/// same bar from comparable windows. Both return `None` until the signal line is
/// seeded (`slow + signal - 1` bars); [`MacdF64::macd_line`] has the MACD line from bar
/// `slow` on.
#[derive(Debug, Clone)]
pub struct MacdF64 {
    fast: EmaF64,
    slow: EmaF64,
    signal: EmaF64,
    fast_delay: usize,
    seen: usize,
    line: Option<f64>,
}

impl MacdF64 {
    /// The `f64` twin of [`Macd::new`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if any period is zero or `fast >= slow`.
    pub fn new(fast: usize, slow: usize, signal: usize) -> Result<Self, FinError> {
        check_period(fast)?;
        check_period(slow)?;
        check_period(signal)?;
        if fast >= slow {
            return Err(FinError::InvalidPeriod(fast));
        }
        Ok(Self::build(fast, slow, signal, 0))
    }

    /// TA-Lib's `MACD` seeding. As in TA-Lib, the periods are swapped when
    /// `fast > slow`, and `fast == slow` is allowed (the MACD line is then zero).
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if any period is zero.
    pub fn talib(fast: usize, slow: usize, signal: usize) -> Result<Self, FinError> {
        check_period(fast)?;
        check_period(slow)?;
        check_period(signal)?;
        let (fast, slow) = if fast > slow { (slow, fast) } else { (fast, slow) };
        Ok(Self::build(fast, slow, signal, slow - fast))
    }

    fn build(fast: usize, slow: usize, signal: usize, fast_delay: usize) -> Self {
        Self {
            fast: EmaF64::with_k(fast, 2.0 / as_f64(fast + 1)),
            slow: EmaF64::with_k(slow, 2.0 / as_f64(slow + 1)),
            signal: EmaF64::with_k(signal, 2.0 / as_f64(signal + 1)),
            fast_delay,
            seen: 0,
            line: None,
        }
    }

    /// Feeds one close and returns all three lines once the signal line is seeded.
    pub fn update(&mut self, close: f64) -> Option<MacdOutputF64> {
        let index = self.seen;
        self.seen += 1;
        let fast = if index >= self.fast_delay { self.fast.update(close) } else { None };
        let slow = self.slow.update(close);
        let line = fast? - slow?;
        self.line = Some(line);
        let signal = self.signal.update(line)?;
        Some(MacdOutputF64 { macd: line, signal, histogram: line - signal })
    }

    /// The latest MACD line, available before the signal line is seeded.
    pub fn macd_line(&self) -> Option<f64> {
        self.line
    }

    /// `true` once the signal line is seeded.
    pub fn is_ready(&self) -> bool {
        self.signal.is_ready()
    }

    /// The `(fast, slow, signal)` periods.
    pub fn periods(&self) -> (usize, usize, usize) {
        (self.fast.period, self.slow.period, self.signal.period)
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.fast.reset();
        self.slow.reset();
        self.signal.reset();
        self.seen = 0;
        self.line = None;
    }
}

// ── Rolling standard deviation and Bollinger Bands ───────────────────────────

/// Population variance of a window, two-pass (mean first) for accuracy.
fn population_variance(values: &VecDeque<f64>, mean: f64) -> f64 {
    let n = as_f64(values.len());
    values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n
}

/// Rolling population standard deviation in `f64`.
///
/// The `f64` twin of [`StdDev`] and TA-Lib's `STDDEV` with `nbdev = 1`. `None` until
/// `period` values have been seen.
#[derive(Debug, Clone)]
pub struct StdDevF64 {
    window: SumWindow,
}

impl StdDevF64 {
    /// Creates a standard deviation over `period` values.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { window: SumWindow::new(period) })
    }

    /// Feeds one value and returns the standard deviation once the window is full.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        self.window.push(value);
        if !self.window.is_full() {
            return None;
        }
        Some(population_variance(&self.window.values, self.window.mean()).sqrt())
    }

    /// The mean of the current window, once full.
    pub fn mean(&self) -> Option<f64> {
        self.window.is_full().then(|| self.window.mean())
    }

    /// `true` once `period` values have been seen.
    pub fn is_ready(&self) -> bool {
        self.window.is_full()
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.window.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.window.clear();
    }
}

/// Bollinger Bands for one bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BollingerOutputF64 {
    /// `middle + multiplier * stddev`.
    pub upper: f64,
    /// The simple moving average.
    pub middle: f64,
    /// `middle - multiplier * stddev`.
    pub lower: f64,
    /// `(close - lower) / (upper - lower)`, or 0.5 when the bands are flat (the value
    /// [`BollingerB`] returns).
    pub percent_b: f64,
}

/// Bollinger Bands in `f64`: SMA middle band, population standard deviation.
///
/// The `f64` twin of [`BollingerB`] (which returns `percent_b`), and TA-Lib's `BBANDS`
/// with an SMA and equal up and down multipliers. `None` until `period` closes have
/// been seen.
#[derive(Debug, Clone)]
pub struct BollingerF64 {
    window: SumWindow,
    multiplier: f64,
}

impl BollingerF64 {
    /// Creates Bollinger Bands over `period` closes, `multiplier` standard deviations
    /// wide.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`; [`FinError::InvalidInput`] if
    /// `multiplier` is not finite.
    pub fn new(period: usize, multiplier: f64) -> Result<Self, FinError> {
        check_period(period)?;
        if !multiplier.is_finite() {
            return Err(FinError::InvalidInput(format!(
                "Bollinger multiplier must be finite, got {multiplier}"
            )));
        }
        Ok(Self { window: SumWindow::new(period), multiplier })
    }

    /// Feeds one close and returns the bands once the window is full.
    pub fn update(&mut self, close: f64) -> Option<BollingerOutputF64> {
        self.window.push(close);
        if !self.window.is_full() {
            return None;
        }
        let middle = self.window.mean();
        let width = self.multiplier * population_variance(&self.window.values, middle).sqrt();
        let upper = middle + width;
        let lower = middle - width;
        let range = upper - lower;
        let percent_b = if range == 0.0 { 0.5 } else { (close - lower) / range };
        Some(BollingerOutputF64 { upper, middle, lower, percent_b })
    }

    /// The band multiplier.
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// `true` once `period` closes have been seen.
    pub fn is_ready(&self) -> bool {
        self.window.is_full()
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.window.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.window.clear();
    }
}

// ── True range and ATR ───────────────────────────────────────────────────────

/// True range in `f64`: `max(high - low, |high - prev_close|, |low - prev_close|)`.
///
/// The first bar has no previous close, so its true range is `high - low` (the same
/// rule as [`BarInput::true_range`](crate::signals::BarInput::true_range)).
#[derive(Debug, Clone, Default)]
pub struct TrueRangeF64 {
    prev_close: Option<f64>,
}

impl TrueRangeF64 {
    /// Creates a true range tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one bar and returns its true range.
    pub fn update(&mut self, high: f64, low: f64, close: f64) -> f64 {
        let hl = high - low;
        let tr = match self.prev_close {
            None => hl,
            Some(pc) => hl.max((high - pc).abs()).max((low - pc).abs()),
        };
        self.prev_close = Some(close);
        tr
    }

    /// The previous bar's close, if any bar has been seen.
    pub fn prev_close(&self) -> Option<f64> {
        self.prev_close
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.prev_close = None;
    }
}

#[derive(Debug, Clone)]
enum AtrSmoothingF64 {
    Simple(SmaF64),
    Wilder(EmaF64),
}

/// Average true range in `f64`.
///
/// Both constructors skip the first bar (it has no previous close) and average the
/// true ranges of the next `period` bars, so the first value arrives on bar
/// `period + 1`.
///
/// - [`AtrF64::new`] keeps a simple moving average of the true range: the `f64` twin of
///   [`Atr::new`].
/// - [`AtrF64::wilder`] continues with Wilder smoothing after the SMA seed: the `f64`
///   twin of [`Atr::wilder`] and TA-Lib's `ATR`.
#[derive(Debug, Clone)]
pub struct AtrF64 {
    prev_close: Option<f64>,
    smoothing: AtrSmoothingF64,
}

impl AtrF64 {
    /// Simple-average ATR (the twin of [`Atr::new`]).
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        Ok(Self { prev_close: None, smoothing: AtrSmoothingF64::Simple(SmaF64::new(period)?) })
    }

    /// Wilder ATR (the twin of [`Atr::wilder`] and TA-Lib's `ATR`).
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn wilder(period: usize) -> Result<Self, FinError> {
        Ok(Self { prev_close: None, smoothing: AtrSmoothingF64::Wilder(EmaF64::wilder(period)?) })
    }

    /// Feeds one bar and returns the ATR once seeded.
    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let prev = self.prev_close.replace(close)?;
        let tr = (high - low).max((high - prev).abs()).max((low - prev).abs());
        match &mut self.smoothing {
            AtrSmoothingF64::Simple(s) => s.update(tr),
            AtrSmoothingF64::Wilder(e) => e.update(tr),
        }
    }

    /// `true` once seeded.
    pub fn is_ready(&self) -> bool {
        match &self.smoothing {
            AtrSmoothingF64::Simple(s) => s.is_ready(),
            AtrSmoothingF64::Wilder(e) => e.is_ready(),
        }
    }

    /// The period.
    pub fn period(&self) -> usize {
        match &self.smoothing {
            AtrSmoothingF64::Simple(s) => s.period(),
            AtrSmoothingF64::Wilder(e) => e.period(),
        }
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.prev_close = None;
        match &mut self.smoothing {
            AtrSmoothingF64::Simple(s) => s.reset(),
            AtrSmoothingF64::Wilder(e) => e.reset(),
        }
    }
}

// ── Rolling max / min ────────────────────────────────────────────────────────

/// Monotonic deque for a rolling extreme in amortized O(1) per value.
#[derive(Debug, Clone)]
struct RollingExtreme {
    period: usize,
    seen: usize,
    /// (index, value), values strictly decreasing (max) or increasing (min) from the front.
    deque: VecDeque<(usize, f64)>,
    want_max: bool,
}

impl RollingExtreme {
    fn new(period: usize, want_max: bool) -> Self {
        Self { period, seen: 0, deque: VecDeque::with_capacity(period), want_max }
    }

    fn update(&mut self, value: f64) -> Option<f64> {
        let index = self.seen;
        self.seen += 1;
        while self.deque.front().is_some_and(|&(i, _)| i + self.period <= index) {
            self.deque.pop_front();
        }
        // NaN never enters the deque, so it cannot hide a later extreme.
        if !value.is_nan() {
            while self.deque.back().is_some_and(|&(_, v)| {
                if self.want_max { v <= value } else { v >= value }
            }) {
                self.deque.pop_back();
            }
            self.deque.push_back((index, value));
        }
        if self.seen < self.period {
            return None;
        }
        Some(self.deque.front().map_or(f64::NAN, |&(_, v)| v))
    }

    fn reset(&mut self) {
        self.seen = 0;
        self.deque.clear();
    }
}

/// Highest value over the last `period` values, in `f64` (TA-Lib's `MAX`).
///
/// `None` until `period` values have been seen. NaN inputs are skipped; a window that
/// holds only NaN returns NaN.
#[derive(Debug, Clone)]
pub struct MaxF64 {
    inner: RollingExtreme,
}

impl MaxF64 {
    /// Creates a rolling maximum over `period` values.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { inner: RollingExtreme::new(period, true) })
    }

    /// Feeds one value and returns the window maximum once full.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        self.inner.update(value)
    }

    /// `true` once `period` values have been seen.
    pub fn is_ready(&self) -> bool {
        self.inner.seen >= self.inner.period
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.inner.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

/// Lowest value over the last `period` values, in `f64` (TA-Lib's `MIN`).
///
/// `None` until `period` values have been seen. NaN inputs are skipped; a window that
/// holds only NaN returns NaN.
#[derive(Debug, Clone)]
pub struct MinF64 {
    inner: RollingExtreme,
}

impl MinF64 {
    /// Creates a rolling minimum over `period` values.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { inner: RollingExtreme::new(period, false) })
    }

    /// Feeds one value and returns the window minimum once full.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        self.inner.update(value)
    }

    /// `true` once `period` values have been seen.
    pub fn is_ready(&self) -> bool {
        self.inner.seen >= self.inner.period
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.inner.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

// ── Stochastic %K ────────────────────────────────────────────────────────────

/// Fast stochastic %K in `f64`:
/// `(close - lowest_low) / (highest_high - lowest_low) * 100` over `period` bars.
///
/// The `f64` twin of [`StochasticK`] and TA-Lib's `STOCHF` fast-K. A flat window
/// (highest high equals lowest low) returns 50, as [`StochasticK`] does; TA-Lib returns
/// 0 there. `None` until `period` bars have been seen.
#[derive(Debug, Clone)]
pub struct StochasticF64 {
    highest: MaxF64,
    lowest: MinF64,
}

impl StochasticF64 {
    /// Creates a %K over `period` bars.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        Ok(Self { highest: MaxF64::new(period)?, lowest: MinF64::new(period)? })
    }

    /// Feeds one bar and returns %K once the window is full.
    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let hh = self.highest.update(high);
        let ll = self.lowest.update(low);
        let (hh, ll) = (hh?, ll?);
        let range = hh - ll;
        Some(if range == 0.0 { 50.0 } else { (close - ll) / range * 100.0 })
    }

    /// `true` once `period` bars have been seen.
    pub fn is_ready(&self) -> bool {
        self.highest.is_ready()
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.highest.period()
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.highest.reset();
        self.lowest.reset();
    }
}

// ── ROC ──────────────────────────────────────────────────────────────────────

/// Rate of change in percent: `(close - close[period]) / close[period] * 100`.
///
/// The `f64` twin of [`Roc`] and TA-Lib's `ROC`; a zero reference close gives 0.
/// `None` until `period + 1` closes have been seen.
#[derive(Debug, Clone)]
pub struct RocF64 {
    period: usize,
    history: VecDeque<f64>,
}

impl RocF64 {
    /// Creates a rate of change over `period` bars.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { period, history: VecDeque::with_capacity(period + 2) })
    }

    /// Feeds one close and returns the rate of change once `period + 1` closes are in.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.history.push_back(close);
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }
        if self.history.len() <= self.period {
            return None;
        }
        let old = *self.history.front()?;
        Some(if old == 0.0 { 0.0 } else { (close - old) / old * 100.0 })
    }

    /// `true` once `period + 1` closes have been seen.
    pub fn is_ready(&self) -> bool {
        self.history.len() > self.period
    }

    /// The period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.history.clear();
    }
}

// ── OBV ──────────────────────────────────────────────────────────────────────

/// On-balance volume in `f64`.
///
/// The `f64` twin of [`Obv`]: starts at 0 on the first bar, then adds the volume on an
/// up close and subtracts it on a down close. (TA-Lib's `OBV` starts at the first bar's
/// volume instead, so its values are this plus a constant.)
#[derive(Debug, Clone, Default)]
pub struct ObvF64 {
    prev_close: Option<f64>,
    obv: f64,
}

impl ObvF64 {
    /// Creates an OBV accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one bar and returns the running OBV.
    pub fn update(&mut self, close: f64, volume: f64) -> f64 {
        if let Some(prev) = self.prev_close {
            if close > prev {
                self.obv += volume;
            } else if close < prev {
                self.obv -= volume;
            }
        }
        self.prev_close = Some(close);
        self.obv
    }

    /// The current OBV.
    pub fn value(&self) -> f64 {
        self.obv
    }

    /// `true` once a bar has been seen.
    pub fn is_ready(&self) -> bool {
        self.prev_close.is_some()
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.prev_close = None;
        self.obv = 0.0;
    }
}

// ── MFI ──────────────────────────────────────────────────────────────────────

/// Money flow index in `f64`.
///
/// The `f64` twin of [`Mfi`] and TA-Lib's `MFI`: typical price times volume, split into
/// positive and negative flow by whether the typical price rose or fell, summed over
/// `period` changes. No negative flow gives 100. `None` until `period + 1` bars have
/// been seen.
#[derive(Debug, Clone)]
pub struct MfiF64 {
    period: usize,
    prev_tp: Option<f64>,
    flows: VecDeque<(f64, f64)>,
}

impl MfiF64 {
    /// Creates an MFI over `period` bars.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { period, prev_tp: None, flows: VecDeque::with_capacity(period + 1) })
    }

    /// Feeds one bar and returns the MFI once `period` flows are in.
    pub fn update(&mut self, high: f64, low: f64, close: f64, volume: f64) -> Option<f64> {
        let tp = (high + low + close) / 3.0;
        let prev = self.prev_tp.replace(tp)?;
        let raw = tp * volume;
        let flow = if tp > prev {
            (raw, 0.0)
        } else if tp < prev {
            (0.0, raw)
        } else {
            (0.0, 0.0)
        };
        self.flows.push_back(flow);
        if self.flows.len() > self.period {
            self.flows.pop_front();
        }
        if self.flows.len() < self.period {
            return None;
        }
        let (pos, neg) = self.flows.iter().fold((0.0, 0.0), |(p, n), (fp, fn_)| (p + fp, n + fn_));
        Some(if neg == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + pos / neg) })
    }

    /// `true` once `period + 1` bars have been seen.
    pub fn is_ready(&self) -> bool {
        self.flows.len() >= self.period
    }

    /// The period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.prev_tp = None;
        self.flows.clear();
    }
}

// ── CCI ──────────────────────────────────────────────────────────────────────

/// Commodity channel index in `f64`:
/// `(tp - SMA(tp)) / (0.015 * mean_deviation(tp))` with `tp = (high + low + close) / 3`.
///
/// The `f64` twin of [`Cci`] and TA-Lib's `CCI`; a zero mean deviation gives 0. `None`
/// until `period` bars have been seen.
#[derive(Debug, Clone)]
pub struct CciF64 {
    window: SumWindow,
}

impl CciF64 {
    /// Creates a CCI over `period` bars.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { window: SumWindow::new(period) })
    }

    /// Feeds one bar and returns the CCI once the window is full.
    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let tp = (high + low + close) / 3.0;
        self.window.push(tp);
        if !self.window.is_full() {
            return None;
        }
        let mean = self.window.mean();
        let md = self.window.values.iter().map(|v| (v - mean).abs()).sum::<f64>()
            / as_f64(self.window.period);
        Some(if md == 0.0 { 0.0 } else { (tp - mean) / (0.015 * md) })
    }

    /// `true` once `period` bars have been seen.
    pub fn is_ready(&self) -> bool {
        self.window.is_full()
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.window.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.window.clear();
    }
}

// ── Efficiency ratio ─────────────────────────────────────────────────────────

/// Kaufman efficiency ratio in `f64`: `|close - close[period]|` divided by the sum of
/// the `period` absolute close-to-close changes.
///
/// The `f64` twin of [`EfficiencyRatio`]; a window with no movement gives 0. `None`
/// until `period + 1` closes have been seen.
#[derive(Debug, Clone)]
pub struct EfficiencyRatioF64 {
    period: usize,
    closes: VecDeque<f64>,
}

impl EfficiencyRatioF64 {
    /// Creates an efficiency ratio over `period` changes.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { period, closes: VecDeque::with_capacity(period + 2) })
    }

    /// Feeds one close and returns the ratio once `period + 1` closes are in.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.closes.push_back(close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return None;
        }
        let first = *self.closes.front()?;
        let path: f64 = self
            .closes
            .iter()
            .zip(self.closes.iter().skip(1))
            .map(|(a, b)| (b - a).abs())
            .sum();
        Some(if path == 0.0 { 0.0 } else { (close - first).abs() / path })
    }

    /// `true` once `period + 1` closes have been seen.
    pub fn is_ready(&self) -> bool {
        self.closes.len() > self.period
    }

    /// The period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.closes.clear();
    }
}

// ── PPO ──────────────────────────────────────────────────────────────────────

/// Percentage price oscillator in `f64`: `(EMA(fast) - EMA(slow)) / EMA(slow) * 100`.
///
/// Both EMAs are seeded from the first bar, which is what [`Ppo`] and TA-Lib's `PPO`
/// with `matype = EMA` do (unlike TA-Lib's MACD, see [`MacdF64::talib`]). A zero slow
/// EMA gives `None`. `None` until the slow EMA is seeded.
#[derive(Debug, Clone)]
pub struct PpoF64 {
    fast: EmaF64,
    slow: EmaF64,
}

impl PpoF64 {
    /// The `f64` twin of [`Ppo::new`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if either period is zero or `fast >= slow`.
    pub fn new(fast: usize, slow: usize) -> Result<Self, FinError> {
        check_period(fast)?;
        check_period(slow)?;
        if fast >= slow {
            return Err(FinError::InvalidPeriod(fast));
        }
        Ok(Self { fast: EmaF64::new(fast)?, slow: EmaF64::new(slow)? })
    }

    /// TA-Lib's `PPO` parameter rules: the periods are swapped when `fast > slow`, and
    /// `fast == slow` is allowed (the PPO is then zero). Values are the same as
    /// [`PpoF64::new`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if either period is zero.
    pub fn talib(fast: usize, slow: usize) -> Result<Self, FinError> {
        check_period(fast)?;
        check_period(slow)?;
        let (fast, slow) = if fast > slow { (slow, fast) } else { (fast, slow) };
        Ok(Self { fast: EmaF64::new(fast)?, slow: EmaF64::new(slow)? })
    }

    /// Feeds one close and returns the PPO once the slow EMA is seeded.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        let fast = self.fast.update(close);
        let slow = self.slow.update(close);
        let (fast, slow) = (fast?, slow?);
        if slow == 0.0 {
            return None;
        }
        Some((fast - slow) / slow * 100.0)
    }

    /// `true` once the slow EMA is seeded.
    pub fn is_ready(&self) -> bool {
        self.slow.is_ready() && self.fast.is_ready()
    }

    /// The `(fast, slow)` periods.
    pub fn periods(&self) -> (usize, usize) {
        (self.fast.period, self.slow.period)
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.fast.reset();
        self.slow.reset();
    }
}

// ── Mean absolute deviation ──────────────────────────────────────────────────

/// Rolling mean absolute deviation in `f64`: `mean(|x - mean(x)|)` over `period` values.
///
/// The `f64` twin of [`RollingMAD`]. `None` until `period` values have been seen.
#[derive(Debug, Clone)]
pub struct MeanAbsDevF64 {
    window: SumWindow,
}

impl MeanAbsDevF64 {
    /// Creates a mean absolute deviation over `period` values.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self, FinError> {
        check_period(period)?;
        Ok(Self { window: SumWindow::new(period) })
    }

    /// Feeds one value and returns the deviation once the window is full.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        self.window.push(value);
        if !self.window.is_full() {
            return None;
        }
        let mean = self.window.mean();
        Some(
            self.window.values.iter().map(|v| (v - mean).abs()).sum::<f64>()
                / as_f64(self.window.period),
        )
    }

    /// `true` once `period` values have been seen.
    pub fn is_ready(&self) -> bool {
        self.window.is_full()
    }

    /// The window length.
    pub fn period(&self) -> usize {
        self.window.period
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.window.clear();
    }
}

// ── f64 constructors on the Decimal indicators ───────────────────────────────

impl Sma {
    /// The `f64` fast path: an [`SmaF64`] over `period` values.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<SmaF64, FinError> {
        SmaF64::new(period)
    }
}

impl Ema {
    /// The `f64` fast path: an [`EmaF64`] with `k = 2 / (period + 1)`.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<EmaF64, FinError> {
        EmaF64::new(period)
    }

    /// The `f64` fast path of [`Ema::wilder`]: an [`EmaF64`] with `k = 1 / period`.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn wilder_f64(period: usize) -> Result<EmaF64, FinError> {
        EmaF64::wilder(period)
    }
}

impl Rsi {
    /// The `f64` fast path: a Wilder [`RsiF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<RsiF64, FinError> {
        RsiF64::new(period)
    }
}

impl Macd {
    /// The `f64` fast path: a [`MacdF64`] with the same seeding as [`Macd::new`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if any period is zero or `fast >= slow`.
    pub fn new_f64(fast: usize, slow: usize, signal: usize) -> Result<MacdF64, FinError> {
        MacdF64::new(fast, slow, signal)
    }
}

impl BollingerB {
    /// The `f64` fast path: [`BollingerF64`] bands (with `%B` in
    /// [`BollingerOutputF64::percent_b`]).
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`; [`FinError::InvalidInput`] if
    /// `multiplier` is not finite.
    pub fn new_f64(period: usize, multiplier: f64) -> Result<BollingerF64, FinError> {
        BollingerF64::new(period, multiplier)
    }
}

impl Atr {
    /// The `f64` fast path of [`Atr::new`]: a simple-average [`AtrF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<AtrF64, FinError> {
        AtrF64::new(period)
    }

    /// The `f64` fast path of [`Atr::wilder`]: a Wilder [`AtrF64`] (TA-Lib's `ATR`).
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn wilder_f64(period: usize) -> Result<AtrF64, FinError> {
        AtrF64::wilder(period)
    }
}

impl StdDev {
    /// The `f64` fast path: a population [`StdDevF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<StdDevF64, FinError> {
        StdDevF64::new(period)
    }
}

impl StochasticK {
    /// The `f64` fast path: a [`StochasticF64`] %K.
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<StochasticF64, FinError> {
        StochasticF64::new(period)
    }
}

impl Roc {
    /// The `f64` fast path: a [`RocF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<RocF64, FinError> {
        RocF64::new(period)
    }
}

impl Obv {
    /// The `f64` fast path: an [`ObvF64`].
    pub fn new_f64() -> ObvF64 {
        ObvF64::new()
    }
}

impl Mfi {
    /// The `f64` fast path: an [`MfiF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<MfiF64, FinError> {
        MfiF64::new(period)
    }
}

impl Cci {
    /// The `f64` fast path: a [`CciF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<CciF64, FinError> {
        CciF64::new(period)
    }
}

impl EfficiencyRatio {
    /// The `f64` fast path: an [`EfficiencyRatioF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<EfficiencyRatioF64, FinError> {
        EfficiencyRatioF64::new(period)
    }
}

impl Ppo {
    /// The `f64` fast path: a [`PpoF64`] with the same seeding as [`Ppo::new`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if either period is zero or `fast >= slow`.
    pub fn new_f64(fast: usize, slow: usize) -> Result<PpoF64, FinError> {
        PpoF64::new(fast, slow)
    }
}

impl RollingMAD {
    /// The `f64` fast path: a [`MeanAbsDevF64`].
    ///
    /// # Errors
    /// [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new_f64(period: usize) -> Result<MeanAbsDevF64, FinError> {
        MeanAbsDevF64::new(period)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_periods_are_rejected() {
        assert!(SmaF64::new(0).is_err());
        assert!(EmaF64::new(0).is_err());
        assert!(EmaF64::wilder(0).is_err());
        assert!(RsiF64::new(0).is_err());
        assert!(MacdF64::new(0, 26, 9).is_err());
        assert!(MacdF64::new(26, 12, 9).is_err());
        assert!(MacdF64::talib(12, 26, 0).is_err());
        assert!(BollingerF64::new(0, 2.0).is_err());
        assert!(BollingerF64::new(20, f64::NAN).is_err());
        assert!(AtrF64::new(0).is_err());
        assert!(AtrF64::wilder(0).is_err());
        assert!(StdDevF64::new(0).is_err());
        assert!(MaxF64::new(0).is_err());
        assert!(MinF64::new(0).is_err());
        assert!(StochasticF64::new(0).is_err());
        assert!(RocF64::new(0).is_err());
        assert!(MfiF64::new(0).is_err());
        assert!(CciF64::new(0).is_err());
        assert!(EfficiencyRatioF64::new(0).is_err());
        assert!(PpoF64::new(3, 3).is_err());
        assert!(PpoF64::talib(0, 3).is_err());
        assert!(MeanAbsDevF64::new(0).is_err());
    }

    #[test]
    fn sma_recovers_after_nan_leaves_window() {
        let mut s = SmaF64::new(2).unwrap();
        s.update(1.0);
        assert!(s.update(f64::NAN).unwrap().is_nan());
        assert!(s.update(3.0).unwrap().is_nan());
        assert_eq!(s.update(5.0), Some(4.0));
    }

    #[test]
    fn rolling_max_min_skip_nan() {
        let mut mx = MaxF64::new(3).unwrap();
        let mut mn = MinF64::new(3).unwrap();
        let xs = [5.0, f64::NAN, 7.0, 6.0, 1.0];
        let maxes: Vec<_> = xs.iter().map(|&x| mx.update(x)).collect();
        let mins: Vec<_> = xs.iter().map(|&x| mn.update(x)).collect();
        assert_eq!(maxes, vec![None, None, Some(7.0), Some(7.0), Some(7.0)]);
        assert_eq!(mins, vec![None, None, Some(5.0), Some(6.0), Some(1.0)]);
    }

    #[test]
    fn talib_macd_swaps_inverted_periods() {
        let m = MacdF64::talib(26, 12, 9).unwrap();
        assert_eq!(m.periods(), (12, 26, 9));
    }

    #[test]
    fn reset_restores_initial_behaviour() {
        let mut rsi = RsiF64::new(3).unwrap();
        let xs = [1.0, 2.0, 1.5, 3.0, 2.0];
        let a: Vec<_> = xs.iter().map(|&x| rsi.update(x)).collect();
        rsi.reset();
        let b: Vec<_> = xs.iter().map(|&x| rsi.update(x)).collect();
        assert_eq!(a, b);
        assert!(rsi.is_ready());
    }
}
