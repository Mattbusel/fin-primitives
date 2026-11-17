//! The indicator structs of `ta::indicators`, with the same names, constructors,
//! outputs, `Default`s and `Display` strings.
//!
//! Each one wraps an `f64` fast-path type from
//! [`signals::float`](crate::signals::float). Outputs are `f64::NAN` during warm-up;
//! see the [module docs](super#where-results-intentionally-differ-from-ta) for where
//! the values differ from `ta`.

use super::errors::{Result, TaError};
use super::{Close, High, Low, Next, Period, Reset, Volume};
use crate::error::FinError;
use crate::signals::float::{
    AtrF64, BollingerF64, CciF64, EfficiencyRatioF64, EmaF64, MacdF64, MaxF64, MeanAbsDevF64,
    MfiF64, MinF64, ObvF64, PpoF64, RocF64, RsiF64, SmaF64, StdDevF64, StochasticF64,
    TrueRangeF64,
};
use std::collections::VecDeque;
use std::fmt;

fn param<T>(r: std::result::Result<T, FinError>) -> Result<T> {
    r.map_err(|_| TaError::InvalidParameter)
}

/// Unwraps a constructor called with fixed, valid default parameters.
fn valid<T>(r: std::result::Result<T, FinError>) -> T {
    match r {
        Ok(v) => v,
        Err(_) => unreachable!("default indicator parameters are valid"),
    }
}

fn or_nan(v: Option<f64>) -> f64 {
    v.unwrap_or(f64::NAN)
}

/// Implements `Next<&T>` by forwarding the bar's close to `Next<f64>`.
macro_rules! next_on_close {
    ($ty:ty) => {
        impl<T: Close> Next<&T> for $ty {
            type Output = <$ty as Next<f64>>::Output;
            fn next(&mut self, input: &T) -> Self::Output {
                self.next(input.close())
            }
        }
    };
}

// ── SimpleMovingAverage ──────────────────────────────────────────────────────

/// Simple moving average (SMA), as `ta::indicators::SimpleMovingAverage`.
///
/// Backed by [`SmaF64`]. `NaN` until `period` inputs have been seen.
///
/// ```rust
/// use fin_primitives::ta_compat::indicators::SimpleMovingAverage;
/// use fin_primitives::ta_compat::Next;
///
/// let mut sma = SimpleMovingAverage::new(3).unwrap();
/// assert!(sma.next(10.0).is_nan());
/// assert!(sma.next(11.0).is_nan());
/// assert_eq!(sma.next(12.0), 11.0);
/// assert_eq!(sma.next(13.0), 12.0);
/// ```
#[doc(alias = "SMA")]
#[derive(Debug, Clone)]
pub struct SimpleMovingAverage {
    inner: SmaF64,
}

impl SimpleMovingAverage {
    /// An SMA over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(SmaF64::new(period))? })
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for SimpleMovingAverage {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for SimpleMovingAverage {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}
next_on_close!(SimpleMovingAverage);

impl Reset for SimpleMovingAverage {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for SimpleMovingAverage {
    fn default() -> Self {
        Self { inner: valid(SmaF64::new(9)) }
    }
}

impl fmt::Display for SimpleMovingAverage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SMA({})", self.inner.period())
    }
}

// ── ExponentialMovingAverage ─────────────────────────────────────────────────

/// Exponential moving average (EMA), as `ta::indicators::ExponentialMovingAverage`.
///
/// Backed by [`EmaF64`]: `k = 2 / (period + 1)`, seeded with the SMA of the first
/// `period` inputs (TA-Lib `EMA`; `ta` seeds with the first input). `NaN` until
/// `period` inputs have been seen.
#[doc(alias = "EMA")]
#[derive(Debug, Clone)]
pub struct ExponentialMovingAverage {
    inner: EmaF64,
}

impl ExponentialMovingAverage {
    /// An EMA over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(EmaF64::new(period))? })
    }

    /// `true` once seeded.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for ExponentialMovingAverage {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for ExponentialMovingAverage {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}
next_on_close!(ExponentialMovingAverage);

impl Reset for ExponentialMovingAverage {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for ExponentialMovingAverage {
    fn default() -> Self {
        Self { inner: valid(EmaF64::new(9)) }
    }
}

impl fmt::Display for ExponentialMovingAverage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EMA({})", self.inner.period())
    }
}

// ── StandardDeviation ────────────────────────────────────────────────────────

/// Rolling population standard deviation, as `ta::indicators::StandardDeviation`.
///
/// Backed by [`StdDevF64`] (TA-Lib `STDDEV`). `NaN` until `period` inputs have been
/// seen.
#[doc(alias = "SD")]
#[derive(Debug, Clone)]
pub struct StandardDeviation {
    inner: StdDevF64,
}

impl StandardDeviation {
    /// A standard deviation over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(StdDevF64::new(period))? })
    }

    /// The mean of the current window (`NaN` during warm-up).
    pub fn mean(&self) -> f64 {
        or_nan(self.inner.mean())
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for StandardDeviation {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for StandardDeviation {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}
next_on_close!(StandardDeviation);

impl Reset for StandardDeviation {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for StandardDeviation {
    fn default() -> Self {
        Self { inner: valid(StdDevF64::new(9)) }
    }
}

impl fmt::Display for StandardDeviation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SD({})", self.inner.period())
    }
}

// ── MeanAbsoluteDeviation ────────────────────────────────────────────────────

/// Rolling mean absolute deviation, as `ta::indicators::MeanAbsoluteDeviation`.
///
/// Backed by [`MeanAbsDevF64`]. `NaN` until `period` inputs have been seen.
#[derive(Debug, Clone)]
pub struct MeanAbsoluteDeviation {
    inner: MeanAbsDevF64,
}

impl MeanAbsoluteDeviation {
    /// A mean absolute deviation over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(MeanAbsDevF64::new(period))? })
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for MeanAbsoluteDeviation {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for MeanAbsoluteDeviation {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}
next_on_close!(MeanAbsoluteDeviation);

impl Reset for MeanAbsoluteDeviation {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for MeanAbsoluteDeviation {
    fn default() -> Self {
        Self { inner: valid(MeanAbsDevF64::new(9)) }
    }
}

impl fmt::Display for MeanAbsoluteDeviation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MAD({})", self.inner.period())
    }
}

// ── RelativeStrengthIndex ────────────────────────────────────────────────────

/// Wilder's relative strength index, as `ta::indicators::RelativeStrengthIndex`.
///
/// Backed by [`RsiF64`], which matches TA-Lib's `RSI` (`ta` uses a non-Wilder EMA; see
/// the [module docs](super#where-results-intentionally-differ-from-ta)). `NaN` until
/// `period + 1` inputs have been seen.
#[doc(alias = "RSI")]
#[derive(Debug, Clone)]
pub struct RelativeStrengthIndex {
    inner: RsiF64,
}

impl RelativeStrengthIndex {
    /// An RSI over `period` changes.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(RsiF64::new(period))? })
    }

    /// `true` once seeded.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for RelativeStrengthIndex {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for RelativeStrengthIndex {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}
next_on_close!(RelativeStrengthIndex);

impl Reset for RelativeStrengthIndex {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for RelativeStrengthIndex {
    fn default() -> Self {
        Self { inner: valid(RsiF64::new(14)) }
    }
}

impl fmt::Display for RelativeStrengthIndex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RSI({})", self.inner.period())
    }
}

// ── Maximum / Minimum ────────────────────────────────────────────────────────

/// Highest value over `period` inputs, as `ta::indicators::Maximum` (reads the high of
/// a bar).
///
/// Backed by [`MaxF64`] (TA-Lib `MAX`). `NaN` until `period` inputs have been seen.
#[derive(Debug, Clone)]
pub struct Maximum {
    inner: MaxF64,
}

impl Maximum {
    /// A rolling maximum over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(MaxF64::new(period))? })
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for Maximum {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for Maximum {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}

impl<T: High> Next<&T> for Maximum {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        self.next(input.high())
    }
}

impl Reset for Maximum {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for Maximum {
    fn default() -> Self {
        Self { inner: valid(MaxF64::new(14)) }
    }
}

impl fmt::Display for Maximum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MAX({})", self.inner.period())
    }
}

/// Lowest value over `period` inputs, as `ta::indicators::Minimum` (reads the low of a
/// bar).
///
/// Backed by [`MinF64`] (TA-Lib `MIN`). `NaN` until `period` inputs have been seen.
#[derive(Debug, Clone)]
pub struct Minimum {
    inner: MinF64,
}

impl Minimum {
    /// A rolling minimum over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(MinF64::new(period))? })
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for Minimum {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for Minimum {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}

impl<T: Low> Next<&T> for Minimum {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        self.next(input.low())
    }
}

impl Reset for Minimum {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for Minimum {
    fn default() -> Self {
        Self { inner: valid(MinF64::new(14)) }
    }
}

impl fmt::Display for Minimum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MIN({})", self.inner.period())
    }
}

// ── FastStochastic / SlowStochastic ──────────────────────────────────────────

/// Fast stochastic %K, as `ta::indicators::FastStochastic`.
///
/// Backed by [`StochasticF64`] (TA-Lib `STOCHF` fast-K; a flat window gives 50 as in
/// `ta`). `NaN` until `period` inputs have been seen. With `f64` input the value is
/// used as high, low and close.
#[derive(Debug, Clone)]
pub struct FastStochastic {
    inner: StochasticF64,
}

impl FastStochastic {
    /// A %K over `period` bars.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(StochasticF64::new(period))? })
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for FastStochastic {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for FastStochastic {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input, input, input))
    }
}

impl<T: High + Low + Close> Next<&T> for FastStochastic {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        or_nan(self.inner.update(input.high(), input.low(), input.close()))
    }
}

impl Reset for FastStochastic {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for FastStochastic {
    fn default() -> Self {
        Self { inner: valid(StochasticF64::new(14)) }
    }
}

impl fmt::Display for FastStochastic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FAST_STOCH({})", self.inner.period())
    }
}

/// Slow stochastic: an EMA of the fast %K, as `ta::indicators::SlowStochastic`.
///
/// The EMA is seeded with the SMA of the first `ema_period` %K values, which makes this
/// TA-Lib's `STOCH` slow-K with `slowk_matype = EMA`. `NaN` until
/// `stochastic_period + ema_period - 1` inputs have been seen.
#[derive(Debug, Clone)]
pub struct SlowStochastic {
    fast: StochasticF64,
    ema: EmaF64,
}

impl SlowStochastic {
    /// A slow stochastic: %K over `stochastic_period` bars, smoothed by an EMA over
    /// `ema_period`.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if either period is zero.
    pub fn new(stochastic_period: usize, ema_period: usize) -> Result<Self> {
        Ok(Self {
            fast: param(StochasticF64::new(stochastic_period))?,
            ema: param(EmaF64::new(ema_period))?,
        })
    }

    /// `true` once the EMA is seeded.
    pub fn is_ready(&self) -> bool {
        self.ema.is_ready()
    }

    fn step(&mut self, k: Option<f64>) -> f64 {
        match k {
            Some(k) => or_nan(self.ema.update(k)),
            None => f64::NAN,
        }
    }
}

impl Next<f64> for SlowStochastic {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        let k = self.fast.update(input, input, input);
        self.step(k)
    }
}

impl<T: High + Low + Close> Next<&T> for SlowStochastic {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        let k = self.fast.update(input.high(), input.low(), input.close());
        self.step(k)
    }
}

impl Reset for SlowStochastic {
    fn reset(&mut self) {
        self.fast.reset();
        self.ema.reset();
    }
}

impl Default for SlowStochastic {
    fn default() -> Self {
        Self {
            fast: valid(StochasticF64::new(14)),
            ema: valid(EmaF64::new(3)),
        }
    }
}

impl fmt::Display for SlowStochastic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SLOW_STOCH({}, {})", self.fast.period(), self.ema.period())
    }
}

// ── TrueRange / AverageTrueRange ─────────────────────────────────────────────

/// True range, as `ta::indicators::TrueRange`.
///
/// Backed by [`TrueRangeF64`]. Ready from the first bar (`high - low`, as in `ta`).
/// With `f64` input it is the absolute change from the previous input (0 on the first).
#[derive(Debug, Clone, Default)]
pub struct TrueRange {
    inner: TrueRangeF64,
}

impl TrueRange {
    /// A true range tracker.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Next<f64> for TrueRange {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        self.inner.update(input, input, input)
    }
}

impl<T: High + Low + Close> Next<&T> for TrueRange {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        self.inner.update(input.high(), input.low(), input.close())
    }
}

impl Reset for TrueRange {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl fmt::Display for TrueRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TRUE_RANGE()")
    }
}

/// Wilder's average true range, as `ta::indicators::AverageTrueRange`.
///
/// Backed by [`AtrF64::wilder`], which matches TA-Lib's `ATR` (`ta` uses a non-Wilder
/// EMA). `NaN` until `period + 1` bars have been seen. With `f64` input the value is
/// used as high, low and close.
#[doc(alias = "ATR")]
#[derive(Debug, Clone)]
pub struct AverageTrueRange {
    inner: AtrF64,
}

impl AverageTrueRange {
    /// A Wilder ATR over `period` bars.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(AtrF64::wilder(period))? })
    }

    /// `true` once seeded.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for AverageTrueRange {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for AverageTrueRange {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input, input, input))
    }
}

impl<T: High + Low + Close> Next<&T> for AverageTrueRange {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        or_nan(self.inner.update(input.high(), input.low(), input.close()))
    }
}

impl Reset for AverageTrueRange {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for AverageTrueRange {
    fn default() -> Self {
        Self { inner: valid(AtrF64::wilder(14)) }
    }
}

impl fmt::Display for AverageTrueRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ATR({})", self.inner.period())
    }
}

// ── MACD / PPO ───────────────────────────────────────────────────────────────

/// MACD output, as `ta::indicators::MovingAverageConvergenceDivergenceOutput`.
#[derive(Debug, Clone, PartialEq)]
pub struct MovingAverageConvergenceDivergenceOutput {
    /// Fast EMA minus slow EMA.
    pub macd: f64,
    /// EMA of the MACD line.
    pub signal: f64,
    /// MACD minus signal.
    pub histogram: f64,
}

impl From<MovingAverageConvergenceDivergenceOutput> for (f64, f64, f64) {
    fn from(mo: MovingAverageConvergenceDivergenceOutput) -> Self {
        (mo.macd, mo.signal, mo.histogram)
    }
}

/// MACD, as `ta::indicators::MovingAverageConvergenceDivergence`.
///
/// Backed by [`MacdF64::talib`], which matches TA-Lib's `MACD`. `macd` is `NaN` until
/// bar `slow`; `signal` and `histogram` until bar `slow + signal - 1`.
///
/// ```rust
/// use fin_primitives::ta_compat::indicators::MovingAverageConvergenceDivergence as Macd;
/// use fin_primitives::ta_compat::Next;
///
/// let mut macd = Macd::new(3, 6, 4).unwrap();
/// let out: Vec<_> = (0..12).map(|i| macd.next(100.0 + f64::from(i))).collect();
/// assert!(out[4].macd.is_nan());
/// assert!(!out[5].macd.is_nan() && out[5].signal.is_nan());
/// assert!(!out[8].signal.is_nan());
/// let (m, s, h) = out[11].clone().into();
/// assert!((m - s - h).abs() < 1e-12);
/// ```
#[doc(alias = "MACD")]
#[derive(Debug, Clone)]
pub struct MovingAverageConvergenceDivergence {
    inner: MacdF64,
}

impl MovingAverageConvergenceDivergence {
    /// A MACD with the given EMA periods. As in TA-Lib, `fast > slow` is swapped.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if a period is zero.
    pub fn new(fast_period: usize, slow_period: usize, signal_period: usize) -> Result<Self> {
        Ok(Self { inner: param(MacdF64::talib(fast_period, slow_period, signal_period))? })
    }

    /// `true` once the signal line is seeded.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Next<f64> for MovingAverageConvergenceDivergence {
    type Output = MovingAverageConvergenceDivergenceOutput;
    fn next(&mut self, input: f64) -> Self::Output {
        match self.inner.update(input) {
            Some(o) => MovingAverageConvergenceDivergenceOutput {
                macd: o.macd,
                signal: o.signal,
                histogram: o.histogram,
            },
            None => MovingAverageConvergenceDivergenceOutput {
                macd: or_nan(self.inner.macd_line()),
                signal: f64::NAN,
                histogram: f64::NAN,
            },
        }
    }
}
next_on_close!(MovingAverageConvergenceDivergence);

impl Reset for MovingAverageConvergenceDivergence {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for MovingAverageConvergenceDivergence {
    fn default() -> Self {
        Self { inner: valid(MacdF64::talib(12, 26, 9)) }
    }
}

impl fmt::Display for MovingAverageConvergenceDivergence {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (fast, slow, signal) = self.inner.periods();
        write!(f, "MACD({fast}, {slow}, {signal})")
    }
}

/// PPO output, as `ta::indicators::PercentagePriceOscillatorOutput`.
#[derive(Debug, Clone, PartialEq)]
pub struct PercentagePriceOscillatorOutput {
    /// `(fast EMA - slow EMA) / slow EMA * 100`.
    pub ppo: f64,
    /// EMA of the PPO line.
    pub signal: f64,
    /// PPO minus signal.
    pub histogram: f64,
}

impl From<PercentagePriceOscillatorOutput> for (f64, f64, f64) {
    fn from(po: PercentagePriceOscillatorOutput) -> Self {
        (po.ppo, po.signal, po.histogram)
    }
}

/// Percentage price oscillator, as `ta::indicators::PercentagePriceOscillator`.
///
/// The `ppo` line is [`PpoF64::talib`] (TA-Lib's `PPO` with EMAs); the signal line is an
/// SMA-seeded EMA of it. `ppo` is `NaN` until bar `slow` (or when the slow EMA is
/// zero); `signal` and `histogram` until bar `slow + signal - 1`.
#[doc(alias = "PPO")]
#[derive(Debug, Clone)]
pub struct PercentagePriceOscillator {
    ppo: PpoF64,
    signal: EmaF64,
}

impl PercentagePriceOscillator {
    /// A PPO with the given EMA periods. As in TA-Lib, `fast > slow` is swapped.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if a period is zero.
    pub fn new(fast_period: usize, slow_period: usize, signal_period: usize) -> Result<Self> {
        Ok(Self {
            ppo: param(PpoF64::talib(fast_period, slow_period))?,
            signal: param(EmaF64::new(signal_period))?,
        })
    }

    /// `true` once the signal line is seeded.
    pub fn is_ready(&self) -> bool {
        self.signal.is_ready()
    }
}

impl Next<f64> for PercentagePriceOscillator {
    type Output = PercentagePriceOscillatorOutput;
    fn next(&mut self, input: f64) -> Self::Output {
        let Some(ppo) = self.ppo.update(input) else {
            return PercentagePriceOscillatorOutput {
                ppo: f64::NAN,
                signal: f64::NAN,
                histogram: f64::NAN,
            };
        };
        let signal = or_nan(self.signal.update(ppo));
        PercentagePriceOscillatorOutput { ppo, signal, histogram: ppo - signal }
    }
}
next_on_close!(PercentagePriceOscillator);

impl Reset for PercentagePriceOscillator {
    fn reset(&mut self) {
        self.ppo.reset();
        self.signal.reset();
    }
}

impl Default for PercentagePriceOscillator {
    fn default() -> Self {
        Self {
            ppo: valid(PpoF64::talib(12, 26)),
            signal: valid(EmaF64::new(9)),
        }
    }
}

impl fmt::Display for PercentagePriceOscillator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (fast, slow) = self.ppo.periods();
        write!(f, "PPO({fast}, {slow}, {})", self.signal.period())
    }
}

// ── CommodityChannelIndex ────────────────────────────────────────────────────

/// Commodity channel index, as `ta::indicators::CommodityChannelIndex`.
///
/// Backed by [`CciF64`] (TA-Lib `CCI`): the mean deviation is taken of the typical
/// price, where `ta` takes it of the close. `NaN` until `period` bars have been seen.
#[derive(Debug, Clone)]
pub struct CommodityChannelIndex {
    inner: CciF64,
}

impl CommodityChannelIndex {
    /// A CCI over `period` bars.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(CciF64::new(period))? })
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for CommodityChannelIndex {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl<T: Close + High + Low> Next<&T> for CommodityChannelIndex {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        or_nan(self.inner.update(input.high(), input.low(), input.close()))
    }
}

impl Reset for CommodityChannelIndex {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for CommodityChannelIndex {
    fn default() -> Self {
        Self { inner: valid(CciF64::new(20)) }
    }
}

impl fmt::Display for CommodityChannelIndex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CCI({})", self.inner.period())
    }
}

// ── EfficiencyRatio ──────────────────────────────────────────────────────────

/// Kaufman efficiency ratio, as `ta::indicators::EfficiencyRatio`.
///
/// Backed by [`EfficiencyRatioF64`]. `NaN` until `period + 1` inputs have been seen; a
/// window without movement gives 0.
#[doc(alias = "ER")]
#[derive(Debug, Clone)]
pub struct EfficiencyRatio {
    inner: EfficiencyRatioF64,
}

impl EfficiencyRatio {
    /// An efficiency ratio over `period` changes.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(EfficiencyRatioF64::new(period))? })
    }

    /// `true` once `period + 1` inputs have been seen.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for EfficiencyRatio {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for EfficiencyRatio {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}
next_on_close!(EfficiencyRatio);

impl Reset for EfficiencyRatio {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for EfficiencyRatio {
    fn default() -> Self {
        Self { inner: valid(EfficiencyRatioF64::new(14)) }
    }
}

impl fmt::Display for EfficiencyRatio {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ER({})", self.inner.period())
    }
}

// ── BollingerBands ───────────────────────────────────────────────────────────

/// Bollinger Bands output, as `ta::indicators::BollingerBandsOutput`.
#[derive(Debug, Clone, PartialEq)]
pub struct BollingerBandsOutput {
    /// The SMA (middle band).
    pub average: f64,
    /// `average + multiplier * stddev`.
    pub upper: f64,
    /// `average - multiplier * stddev`.
    pub lower: f64,
}

/// Bollinger Bands, as `ta::indicators::BollingerBands`.
///
/// Backed by [`BollingerF64`] (TA-Lib `BBANDS` with an SMA and population standard
/// deviation). All fields are `NaN` until `period` inputs have been seen.
#[doc(alias = "BB")]
#[derive(Debug, Clone)]
pub struct BollingerBands {
    inner: BollingerF64,
}

impl BollingerBands {
    /// Bands over `period` inputs, `multiplier` standard deviations wide.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0` or `multiplier` is not finite.
    pub fn new(period: usize, multiplier: f64) -> Result<Self> {
        Ok(Self { inner: param(BollingerF64::new(period, multiplier))? })
    }

    /// The band multiplier.
    pub fn multiplier(&self) -> f64 {
        self.inner.multiplier()
    }

    /// `true` once the window is full.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for BollingerBands {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for BollingerBands {
    type Output = BollingerBandsOutput;
    fn next(&mut self, input: f64) -> Self::Output {
        match self.inner.update(input) {
            Some(o) => BollingerBandsOutput { average: o.middle, upper: o.upper, lower: o.lower },
            None => BollingerBandsOutput { average: f64::NAN, upper: f64::NAN, lower: f64::NAN },
        }
    }
}
next_on_close!(BollingerBands);

impl Reset for BollingerBands {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for BollingerBands {
    fn default() -> Self {
        Self { inner: valid(BollingerF64::new(9, 2.0)) }
    }
}

impl fmt::Display for BollingerBands {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BB({}, {})", self.inner.period(), self.inner.multiplier())
    }
}

// ── KeltnerChannel ───────────────────────────────────────────────────────────

/// Keltner Channel output, as `ta::indicators::KeltnerChannelOutput`.
#[derive(Debug, Clone, PartialEq)]
pub struct KeltnerChannelOutput {
    /// The EMA (middle line).
    pub average: f64,
    /// `average + multiplier * ATR`.
    pub upper: f64,
    /// `average - multiplier * ATR`.
    pub lower: f64,
}

/// Keltner Channel, as `ta::indicators::KeltnerChannel`: an EMA of the typical price
/// (the close for `f64` input) plus and minus a multiple of Wilder's ATR.
///
/// The EMA is [`EmaF64`] (SMA-seeded) and the ATR is [`AtrF64::wilder`]. `average` is
/// `NaN` until bar `period`, `upper` and `lower` until bar `period + 1`.
#[doc(alias = "KC")]
#[derive(Debug, Clone)]
pub struct KeltnerChannel {
    ema: EmaF64,
    atr: AtrF64,
    multiplier: f64,
}

impl KeltnerChannel {
    /// A channel over `period` bars, `multiplier` ATRs wide.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize, multiplier: f64) -> Result<Self> {
        Ok(Self {
            ema: param(EmaF64::new(period))?,
            atr: param(AtrF64::wilder(period))?,
            multiplier,
        })
    }

    /// The ATR multiplier.
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// `true` once both the EMA and the ATR are seeded.
    pub fn is_ready(&self) -> bool {
        self.ema.is_ready() && self.atr.is_ready()
    }

    fn output(&self, average: Option<f64>, atr: Option<f64>) -> KeltnerChannelOutput {
        let average = or_nan(average);
        let width = or_nan(atr) * self.multiplier;
        KeltnerChannelOutput { average, upper: average + width, lower: average - width }
    }
}

impl Period for KeltnerChannel {
    fn period(&self) -> usize {
        self.ema.period()
    }
}

impl Next<f64> for KeltnerChannel {
    type Output = KeltnerChannelOutput;
    fn next(&mut self, input: f64) -> Self::Output {
        let atr = self.atr.update(input, input, input);
        let average = self.ema.update(input);
        self.output(average, atr)
    }
}

impl<T: Close + High + Low> Next<&T> for KeltnerChannel {
    type Output = KeltnerChannelOutput;
    fn next(&mut self, input: &T) -> Self::Output {
        let typical_price = (input.close() + input.high() + input.low()) / 3.0;
        let average = self.ema.update(typical_price);
        let atr = self.atr.update(input.high(), input.low(), input.close());
        self.output(average, atr)
    }
}

impl Reset for KeltnerChannel {
    fn reset(&mut self) {
        self.ema.reset();
        self.atr.reset();
    }
}

impl Default for KeltnerChannel {
    fn default() -> Self {
        Self {
            ema: valid(EmaF64::new(10)),
            atr: valid(AtrF64::wilder(10)),
            multiplier: 2.0,
        }
    }
}

impl fmt::Display for KeltnerChannel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "KC({}, {})", self.ema.period(), self.multiplier)
    }
}

// ── ChandelierExit ───────────────────────────────────────────────────────────

/// Chandelier Exit output, as `ta::indicators::ChandelierExitOutput`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChandelierExitOutput {
    /// Highest high minus `multiplier * ATR`: the stop for a long position.
    pub long: f64,
    /// Lowest low plus `multiplier * ATR`: the stop for a short position.
    pub short: f64,
}

impl From<ChandelierExitOutput> for (f64, f64) {
    fn from(ce: ChandelierExitOutput) -> Self {
        (ce.long, ce.short)
    }
}

/// Chandelier Exit, as `ta::indicators::ChandelierExit`, with Wilder's ATR
/// ([`AtrF64::wilder`]). `NaN` until bar `period + 1`.
#[doc(alias = "CE")]
#[derive(Debug, Clone)]
pub struct ChandelierExit {
    atr: AtrF64,
    min: MinF64,
    max: MaxF64,
    multiplier: f64,
}

impl ChandelierExit {
    /// A Chandelier Exit over `period` bars, `multiplier` ATRs from the extreme.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize, multiplier: f64) -> Result<Self> {
        Ok(Self {
            atr: param(AtrF64::wilder(period))?,
            min: param(MinF64::new(period))?,
            max: param(MaxF64::new(period))?,
            multiplier,
        })
    }

    /// The ATR multiplier.
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// `true` once the ATR is seeded.
    pub fn is_ready(&self) -> bool {
        self.atr.is_ready()
    }
}

impl Period for ChandelierExit {
    fn period(&self) -> usize {
        self.atr.period()
    }
}

impl<T: Low + High + Close> Next<&T> for ChandelierExit {
    type Output = ChandelierExitOutput;
    fn next(&mut self, input: &T) -> Self::Output {
        let atr = or_nan(self.atr.update(input.high(), input.low(), input.close())) * self.multiplier;
        let min = or_nan(self.min.update(input.low()));
        let max = or_nan(self.max.update(input.high()));
        ChandelierExitOutput { long: max - atr, short: min + atr }
    }
}

impl Reset for ChandelierExit {
    fn reset(&mut self) {
        self.atr.reset();
        self.min.reset();
        self.max.reset();
    }
}

impl Default for ChandelierExit {
    fn default() -> Self {
        Self {
            atr: valid(AtrF64::wilder(22)),
            min: valid(MinF64::new(22)),
            max: valid(MaxF64::new(22)),
            multiplier: 3.0,
        }
    }
}

impl fmt::Display for ChandelierExit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CE({}, {})", self.atr.period(), self.multiplier)
    }
}

// ── RateOfChange ─────────────────────────────────────────────────────────────

/// Rate of change in percent, as `ta::indicators::RateOfChange`.
///
/// Backed by [`RocF64`] (TA-Lib `ROC`). `NaN` until `period + 1` inputs have been
/// seen; a zero reference value gives 0.
#[doc(alias = "ROC")]
#[derive(Debug, Clone)]
pub struct RateOfChange {
    inner: RocF64,
}

impl RateOfChange {
    /// A rate of change over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(RocF64::new(period))? })
    }

    /// `true` once `period + 1` inputs have been seen.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for RateOfChange {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl Next<f64> for RateOfChange {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.inner.update(input))
    }
}
next_on_close!(RateOfChange);

impl Reset for RateOfChange {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for RateOfChange {
    fn default() -> Self {
        Self { inner: valid(RocF64::new(9)) }
    }
}

impl fmt::Display for RateOfChange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ROC({})", self.inner.period())
    }
}

// ── MoneyFlowIndex ───────────────────────────────────────────────────────────

/// Money flow index, as `ta::indicators::MoneyFlowIndex`.
///
/// Backed by [`MfiF64`] (TA-Lib `MFI`). `NaN` until `period + 1` bars have been seen.
#[doc(alias = "MFI")]
#[derive(Debug, Clone)]
pub struct MoneyFlowIndex {
    inner: MfiF64,
}

impl MoneyFlowIndex {
    /// An MFI over `period` bars.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { inner: param(MfiF64::new(period))? })
    }

    /// `true` once `period + 1` bars have been seen.
    pub fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

impl Period for MoneyFlowIndex {
    fn period(&self) -> usize {
        self.inner.period()
    }
}

impl<T: High + Low + Close + Volume> Next<&T> for MoneyFlowIndex {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        or_nan(self.inner.update(input.high(), input.low(), input.close(), input.volume()))
    }
}

impl Reset for MoneyFlowIndex {
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for MoneyFlowIndex {
    fn default() -> Self {
        Self { inner: valid(MfiF64::new(14)) }
    }
}

impl fmt::Display for MoneyFlowIndex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MFI({})", self.inner.period())
    }
}

// ── OnBalanceVolume ──────────────────────────────────────────────────────────

/// On-balance volume, as `ta::indicators::OnBalanceVolume`.
///
/// Starts at the first bar's volume and then adds or subtracts each bar's volume on an
/// up or down close: TA-Lib's `OBV`, and the same values as `ta` for positive prices.
/// Built on [`ObvF64`] (which starts at 0).
#[doc(alias = "OBV")]
#[derive(Debug, Clone, Default)]
pub struct OnBalanceVolume {
    inner: ObvF64,
    first_volume: Option<f64>,
}

impl OnBalanceVolume {
    /// An OBV accumulator.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T: Close + Volume> Next<&T> for OnBalanceVolume {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        let first = *self.first_volume.get_or_insert(input.volume());
        first + self.inner.update(input.close(), input.volume())
    }
}

impl Reset for OnBalanceVolume {
    fn reset(&mut self) {
        self.inner.reset();
        self.first_volume = None;
    }
}

impl fmt::Display for OnBalanceVolume {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "OBV")
    }
}

// ── Indicators `ta` 0.5 does not have ────────────────────────────────────────
//
// `ta` users have asked for these (ADX/DX in greyblake/ta-rs#64, DEMA in #74, Hull in #36).
// They follow the same shape as the indicators above: `new(period)`, `Next<f64>` and/or
// `Next<&T>`, `Reset`, `Period`, `Default`, `Display`, `NaN` during warm-up, TA-Lib's numbers.

/// TA-Lib's test for a DI sum that is zero (the legs are ratios, so a fixed band is
/// scale-free).
fn di_sum_is_zero(v: f64) -> bool {
    v.abs() < 1e-14
}

// ── WeightedMovingAverage ────────────────────────────────────────────────────

/// Linearly weighted moving average (TA-Lib `WMA`): the newest input has weight
/// `period`, the oldest weight 1. `NaN` until `period` inputs have been seen.
///
/// ```rust
/// use fin_primitives::ta_compat::indicators::WeightedMovingAverage;
/// use fin_primitives::ta_compat::Next;
///
/// let mut wma = WeightedMovingAverage::new(3).unwrap();
/// assert!(wma.next(1.0).is_nan());
/// assert!(wma.next(2.0).is_nan());
/// assert_eq!(wma.next(3.0), (1.0 + 4.0 + 9.0) / 6.0);
/// ```
#[doc(alias = "WMA")]
#[derive(Debug, Clone)]
pub struct WeightedMovingAverage {
    period: usize,
    window: VecDeque<f64>,
}

impl WeightedMovingAverage {
    /// A WMA over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        if period == 0 {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }

    /// `true` once `period` inputs have been seen.
    pub fn is_ready(&self) -> bool {
        self.window.len() == self.period
    }

    fn update(&mut self, x: f64) -> Option<f64> {
        if self.window.len() == self.period {
            self.window.pop_front();
        }
        self.window.push_back(x);
        if self.window.len() < self.period {
            return None;
        }
        let mut sum = 0.0;
        let mut w = 0.0;
        for &v in &self.window {
            w += 1.0;
            sum += v * w;
        }
        Some(sum / (w * (w + 1.0) / 2.0))
    }
}

impl Period for WeightedMovingAverage {
    fn period(&self) -> usize {
        self.period
    }
}

impl Next<f64> for WeightedMovingAverage {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.update(input))
    }
}
next_on_close!(WeightedMovingAverage);

impl Reset for WeightedMovingAverage {
    fn reset(&mut self) {
        self.window.clear();
    }
}

impl Default for WeightedMovingAverage {
    fn default() -> Self {
        Self { period: 9, window: VecDeque::with_capacity(9) }
    }
}

impl fmt::Display for WeightedMovingAverage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WMA({})", self.period)
    }
}

// ── DoubleExponentialMovingAverage ───────────────────────────────────────────

/// Double exponential moving average (Mulloy's DEMA, TA-Lib `DEMA`):
/// `2 * EMA(x) - EMA(EMA(x))`, both EMAs seeded with an SMA as in TA-Lib. `NaN` for the
/// first `2 * (period - 1)` inputs.
///
/// ```rust
/// use fin_primitives::ta_compat::indicators::DoubleExponentialMovingAverage;
/// use fin_primitives::ta_compat::Next;
///
/// let mut dema = DoubleExponentialMovingAverage::new(3).unwrap();
/// let out: Vec<f64> = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0].iter().map(|&x| dema.next(x)).collect();
/// assert!(out[3].is_nan());
/// assert!((out[4] - 5.0).abs() < 1e-12); // a straight line: DEMA has no lag
/// ```
#[doc(alias = "DEMA")]
#[derive(Debug, Clone)]
pub struct DoubleExponentialMovingAverage {
    ema1: EmaF64,
    ema2: EmaF64,
}

impl DoubleExponentialMovingAverage {
    /// A DEMA over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { ema1: param(EmaF64::new(period))?, ema2: param(EmaF64::new(period))? })
    }

    /// `true` once both EMAs are seeded.
    pub fn is_ready(&self) -> bool {
        self.ema2.is_ready()
    }

    fn update(&mut self, x: f64) -> Option<f64> {
        let e1 = self.ema1.update(x)?;
        let e2 = self.ema2.update(e1)?;
        Some(2.0 * e1 - e2)
    }
}

impl Period for DoubleExponentialMovingAverage {
    fn period(&self) -> usize {
        self.ema1.period()
    }
}

impl Next<f64> for DoubleExponentialMovingAverage {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.update(input))
    }
}
next_on_close!(DoubleExponentialMovingAverage);

impl Reset for DoubleExponentialMovingAverage {
    fn reset(&mut self) {
        self.ema1.reset();
        self.ema2.reset();
    }
}

impl Default for DoubleExponentialMovingAverage {
    fn default() -> Self {
        Self { ema1: valid(EmaF64::new(9)), ema2: valid(EmaF64::new(9)) }
    }
}

impl fmt::Display for DoubleExponentialMovingAverage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DEMA({})", self.ema1.period())
    }
}

// ── HullMovingAverage ────────────────────────────────────────────────────────

/// Hull moving average (Alan Hull): `WMA(2 * WMA(x, n / 2) - WMA(x, n), sqrt(n))`, with
/// `n / 2` and `sqrt(n)` rounded down, as in pandas-ta and TradingView. TA-Lib has no HMA;
/// this one equals the same composition of TA-Lib's `WMA` to float rounding. `NaN` for the
/// first `n + floor(sqrt(n)) - 2` inputs.
///
/// ```rust
/// use fin_primitives::ta_compat::indicators::HullMovingAverage;
/// use fin_primitives::ta_compat::Next;
///
/// let mut hma = HullMovingAverage::new(9).unwrap();
/// let out: Vec<f64> = (1..=12).map(|x| hma.next(f64::from(x))).collect();
/// assert!(out[9].is_nan());
/// assert!((out[10] - 11.0).abs() < 1e-9); // a straight line: HMA has no lag
/// ```
#[doc(alias = "HMA")]
#[derive(Debug, Clone)]
pub struct HullMovingAverage {
    period: usize,
    half: WeightedMovingAverage,
    full: WeightedMovingAverage,
    smooth: WeightedMovingAverage,
}

/// Integer square root, rounded down.
fn isqrt(n: usize) -> usize {
    let mut r = 0usize;
    while (r + 1).saturating_mul(r + 1) <= n {
        r += 1;
    }
    r
}

impl HullMovingAverage {
    /// An HMA over `period` inputs.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period < 2` (`period / 2` would be 0).
    pub fn new(period: usize) -> Result<Self> {
        if period < 2 {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            period,
            half: WeightedMovingAverage::new(period / 2)?,
            full: WeightedMovingAverage::new(period)?,
            smooth: WeightedMovingAverage::new(isqrt(period))?,
        })
    }

    /// `true` once the first value has been produced.
    pub fn is_ready(&self) -> bool {
        self.smooth.is_ready()
    }

    fn update(&mut self, x: f64) -> Option<f64> {
        let half = self.half.update(x);
        let full = self.full.update(x)?;
        let half = half?;
        self.smooth.update(2.0 * half - full)
    }
}

impl Period for HullMovingAverage {
    fn period(&self) -> usize {
        self.period
    }
}

impl Next<f64> for HullMovingAverage {
    type Output = f64;
    fn next(&mut self, input: f64) -> f64 {
        or_nan(self.update(input))
    }
}
next_on_close!(HullMovingAverage);

impl Reset for HullMovingAverage {
    fn reset(&mut self) {
        self.half.reset();
        self.full.reset();
        self.smooth.reset();
    }
}

impl Default for HullMovingAverage {
    fn default() -> Self {
        Self {
            period: 9,
            half: WeightedMovingAverage { period: 4, window: VecDeque::with_capacity(4) },
            full: WeightedMovingAverage { period: 9, window: VecDeque::with_capacity(9) },
            smooth: WeightedMovingAverage { period: 3, window: VecDeque::with_capacity(3) },
        }
    }
}

impl fmt::Display for HullMovingAverage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HMA({})", self.period)
    }
}

// ── Directional movement (DX, ADX) ───────────────────────────────────────────

/// Wilder's directional movement state, computed the way TA-Lib's `PLUS_DI`, `MINUS_DI`,
/// `DX` and `ADX` compute it: the first sums cover `period - 1` bar-to-bar changes, later
/// bars use `sum - sum / period + new`.
#[derive(Debug, Clone)]
struct DirectionalMovement {
    period: usize,
    /// Bars seen so far.
    bars: usize,
    prev: Option<(f64, f64, f64)>,
    plus_dm: f64,
    minus_dm: f64,
    tr: f64,
    /// Last defined DX (TA-Lib repeats it when DX is undefined).
    last_dx: f64,
}

/// One bar of directional movement output.
#[derive(Debug, Clone, Copy)]
struct DmiStep {
    plus_di: f64,
    minus_di: f64,
    /// `None` when TA-Lib leaves DX undefined for this bar (zero true range or DI sum).
    dx: Option<f64>,
}

impl DirectionalMovement {
    fn new(period: usize) -> Result<Self> {
        if period == 0 {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self::empty(period))
    }

    fn empty(period: usize) -> Self {
        Self { period, bars: 0, prev: None, plus_dm: 0.0, minus_dm: 0.0, tr: 0.0, last_dx: 0.0 }
    }

    fn reset(&mut self) {
        *self = Self::empty(self.period);
    }

    /// Feeds one bar. Returns the DI and DX values from bar index `period` on, `None` before.
    fn update(&mut self, high: f64, low: f64, close: f64) -> Option<DmiStep> {
        self.bars += 1;
        let (ph, pl, pc) = self.prev.replace((high, low, close))?;
        let diff_p = high - ph;
        let diff_m = pl - low;
        let (mut pdm, mut mdm) = (0.0, 0.0);
        if diff_m > 0.0 && diff_p < diff_m {
            mdm = diff_m;
        } else if diff_p > 0.0 && diff_p > diff_m {
            pdm = diff_p;
        }
        let tr = (high - low).max((high - pc).abs()).max((low - pc).abs());
        // Bars 2..=period add plain sums of period - 1 changes; from bar period + 1 on the
        // sums are Wilder-smoothed and DI/DX are defined.
        if self.bars <= self.period {
            self.plus_dm += pdm;
            self.minus_dm += mdm;
            self.tr += tr;
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        let n = self.period as f64;
        self.plus_dm = self.plus_dm - self.plus_dm / n + pdm;
        self.minus_dm = self.minus_dm - self.minus_dm / n + mdm;
        self.tr = self.tr - self.tr / n + tr;
        // The true-range sum is tested exactly, as current TA-Lib does (TA-Lib issue #253:
        // the old fixed `TA_IS_ZERO` band zeroed the index for instruments quoted in tiny
        // units). It is a sum of non-negative ranges, so it is zero only when every range
        // in the window is.
        if self.tr <= 0.0 {
            return Some(DmiStep { plus_di: 0.0, minus_di: 0.0, dx: None });
        }
        let plus_di = 100.0 * (self.plus_dm / self.tr);
        let minus_di = 100.0 * (self.minus_dm / self.tr);
        let sum = plus_di + minus_di;
        let dx = if di_sum_is_zero(sum) { None } else { Some(100.0 * ((minus_di - plus_di).abs() / sum)) };
        if let Some(d) = dx {
            self.last_dx = d;
        }
        Some(DmiStep { plus_di, minus_di, dx })
    }
}

/// Directional movement index output: the +DI and -DI lines and DX, as TA-Lib's
/// `PLUS_DI`, `MINUS_DI` and `DX`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalIndexOutput {
    /// +DI, 0 to 100.
    pub plus_di: f64,
    /// -DI, 0 to 100.
    pub minus_di: f64,
    /// DX = `100 * |+DI - -DI| / (+DI + -DI)`.
    pub dx: f64,
}

/// Wilder's directional movement index (TA-Lib `DX`, `PLUS_DI` and `MINUS_DI`). Takes
/// bars (`High + Low + Close`). Every field is `NaN` for the first `period` bars.
///
/// Where the true range or the DI sum is zero, DX repeats its previous value, as in
/// TA-Lib (0 before there is one).
#[doc(alias = "DX")]
#[doc(alias = "DMI")]
#[derive(Debug, Clone)]
pub struct DirectionalIndex {
    dm: DirectionalMovement,
}

impl DirectionalIndex {
    /// A DX over `period` bars.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { dm: DirectionalMovement::new(period)? })
    }

    /// `true` once the first value has been produced.
    pub fn is_ready(&self) -> bool {
        self.dm.bars > self.dm.period
    }
}

impl Period for DirectionalIndex {
    fn period(&self) -> usize {
        self.dm.period
    }
}

impl<T: High + Low + Close> Next<&T> for DirectionalIndex {
    type Output = DirectionalIndexOutput;
    fn next(&mut self, input: &T) -> DirectionalIndexOutput {
        match self.dm.update(input.high(), input.low(), input.close()) {
            None => DirectionalIndexOutput { plus_di: f64::NAN, minus_di: f64::NAN, dx: f64::NAN },
            Some(s) => DirectionalIndexOutput {
                plus_di: s.plus_di,
                minus_di: s.minus_di,
                dx: s.dx.unwrap_or(self.dm.last_dx),
            },
        }
    }
}

impl Reset for DirectionalIndex {
    fn reset(&mut self) {
        self.dm.reset();
    }
}

impl Default for DirectionalIndex {
    fn default() -> Self {
        Self { dm: DirectionalMovement::empty(14) }
    }
}

impl fmt::Display for DirectionalIndex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DX({})", self.dm.period)
    }
}

/// Wilder's average directional index (TA-Lib `ADX`): the Wilder average of DX. Takes bars
/// (`High + Low + Close`). `NaN` for the first `2 * period - 1` bars.
///
/// ```rust
/// use fin_primitives::ta_compat::indicators::AverageDirectionalIndex;
/// use fin_primitives::ta_compat::{DataItem, Next};
///
/// let mut adx = AverageDirectionalIndex::new(3).unwrap();
/// let mut last = f64::NAN;
/// for i in 0..10 {
///     let x = 100.0 + f64::from(i); // a steady uptrend
///     let bar = DataItem::builder()
///         .open(x).high(x + 1.0).low(x - 0.5).close(x + 0.5).volume(1.0)
///         .build()
///         .unwrap();
///     last = adx.next(&bar);
/// }
/// assert!((last - 100.0).abs() < 1e-9); // only +DM: the trend is as strong as it gets
/// ```
#[doc(alias = "ADX")]
#[derive(Debug, Clone)]
pub struct AverageDirectionalIndex {
    dm: DirectionalMovement,
    dx_sum: f64,
    dx_count: usize,
    adx: Option<f64>,
}

impl AverageDirectionalIndex {
    /// An ADX over `period` bars.
    ///
    /// # Errors
    /// [`TaError::InvalidParameter`] if `period == 0`.
    pub fn new(period: usize) -> Result<Self> {
        Ok(Self { dm: DirectionalMovement::new(period)?, dx_sum: 0.0, dx_count: 0, adx: None })
    }

    /// `true` once the first value has been produced.
    pub fn is_ready(&self) -> bool {
        self.adx.is_some()
    }

    fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let step = self.dm.update(high, low, close)?;
        #[allow(clippy::cast_precision_loss)]
        let n = self.dm.period as f64;
        match self.adx {
            None => {
                // TA-Lib averages the first `period` DX values, leaving undefined ones out
                // of the sum but still dividing by `period`.
                if let Some(dx) = step.dx {
                    self.dx_sum += dx;
                }
                self.dx_count += 1;
                if self.dx_count == self.dm.period {
                    self.adx = Some(self.dx_sum / n);
                }
            }
            Some(prev) => {
                if let Some(dx) = step.dx {
                    self.adx = Some((prev * (n - 1.0) + dx) / n);
                }
            }
        }
        self.adx
    }
}

impl Period for AverageDirectionalIndex {
    fn period(&self) -> usize {
        self.dm.period
    }
}

impl<T: High + Low + Close> Next<&T> for AverageDirectionalIndex {
    type Output = f64;
    fn next(&mut self, input: &T) -> f64 {
        or_nan(self.update(input.high(), input.low(), input.close()))
    }
}

impl Reset for AverageDirectionalIndex {
    fn reset(&mut self) {
        self.dm.reset();
        self.dx_sum = 0.0;
        self.dx_count = 0;
        self.adx = None;
    }
}

impl Default for AverageDirectionalIndex {
    fn default() -> Self {
        Self { dm: DirectionalMovement::empty(14), dx_sum: 0.0, dx_count: 0, adx: None }
    }
}

impl fmt::Display for AverageDirectionalIndex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ADX({})", self.dm.period)
    }
}
