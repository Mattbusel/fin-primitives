//! A drop-in replacement for the [`ta`](https://docs.rs/ta) crate's API (feature
//! `ta-compat`), computed by fin-primitives' Wilder and TA-Lib-exact indicators.
//!
//! `ta` 0.5 has not had a release since 2021. This module copies its public shape (the
//! [`Next`], [`Reset`] and [`Period`] traits, the [`Open`]/[`High`]/[`Low`]/[`Close`]/
//! [`Volume`] input traits, [`DataItem`], [`errors::TaError`] and the indicator structs
//! in [`indicators`] with the same names, constructors, outputs, `Default`s and
//! `Display` strings), so code written against `ta` compiles after changing its `use`
//! lines. The numbers come from the `f64` fast path in
//! [`signals::float`](crate::signals::float), which follows TA-Lib: an RSI or ATR from
//! this module is the RSI or ATR that TA-Lib, most charting platforms and Wilder's book
//! give, which `ta`'s are not.
//!
//! Nothing here depends on the `ta` crate. With the `ta` feature on as well, `ta`'s own
//! `DataItem` implements this module's input traits, so bars you already build for `ta`
//! work unchanged during a migration.
//!
//! # Migration guide
//!
//! 1. Swap the dependency:
//!
//!    ```toml
//!    [dependencies]
//!    # ta = "0.5"
//!    fin-primitives = { version = "3.2", features = ["ta-compat"] }
//!    ```
//!
//!    (`default-features = false, features = ["ta-compat"]` is enough if you only want
//!    this module.)
//!
//! 2. Point the imports at this module. Either replace `ta::` with
//!    `fin_primitives::ta_compat::` in `use` lines, or alias it once per file and keep
//!    every `ta::...` path as it is:
//!
//!    ```rust
//!    use fin_primitives::ta_compat as ta;
//!    use ta::indicators::{ExponentialMovingAverage, RelativeStrengthIndex};
//!    use ta::{DataItem, Next, Reset};
//!
//!    let mut ema = ExponentialMovingAverage::new(3).unwrap();
//!    let mut rsi = RelativeStrengthIndex::new(14).unwrap();
//!    let bar = DataItem::builder()
//!        .open(10.0).high(11.0).low(9.5).close(10.5).volume(1_000.0)
//!        .build()
//!        .unwrap();
//!    let _ = ema.next(&bar);
//!    let _ = rsi.next(10.5);
//!    ema.reset();
//!    ```
//!
//! 3. Handle the warm-up. Every indicator returns `f64::NAN` until it has a full window
//!    (TA-Lib's lookback), where `ta` returns a number computed from whatever it had
//!    seen. Comparisons with NaN are `false`, so `if rsi.next(x) > 70.0` simply does not
//!    fire during warm-up; to test explicitly, use `value.is_nan()` or the indicator's
//!    `is_ready()`. Output structs (MACD, Bollinger, Keltner, ...) carry NaN in the
//!    fields that are not ready yet.
//!
//! 4. Re-check thresholds tuned on `ta`'s RSI, ATR, Keltner Channel, EMA warm-up or
//!    slow stochastic: those values change (see below). Strategies tuned against
//!    TA-Lib, TradingView or other Wilder-based tools now match them.
//!
//! fin-primitives bars ([`OhlcvBar`](crate::ohlcv::OhlcvBar) and
//! [`BarInput`](crate::signals::BarInput)) implement the input traits too, so
//! `indicator.next(&bar)` works on them directly.
//!
//! # Indicators `ta` does not have
//!
//! `ta` users asked for these and `ta` never shipped them (greyblake/ta-rs issues 36, 64 and
//! 74). They have the same shape as the rest of the module and TA-Lib's numbers:
//!
//! | indicator | TA-Lib | first value at bar (0-based) |
//! |---|---|---|
//! | [`AverageDirectionalIndex`](indicators::AverageDirectionalIndex) | `ADX` | `2 * period - 1` |
//! | [`DirectionalIndex`](indicators::DirectionalIndex) (+DI, -DI, DX) | `PLUS_DI`, `MINUS_DI`, `DX` | `period` |
//! | [`DoubleExponentialMovingAverage`](indicators::DoubleExponentialMovingAverage) | `DEMA` | `2 * (period - 1)` |
//! | [`WeightedMovingAverage`](indicators::WeightedMovingAverage) | `WMA` | `period - 1` |
//! | [`HullMovingAverage`](indicators::HullMovingAverage) | `WMA(2 WMA(n/2) - WMA(n), sqrt n)` | `n + floor(sqrt n) - 2` |
//!
//! `tests/ta_compat.rs` checks them against TA-Lib's Python wrapper on the reference bars and
//! `tests/ta_compat_talib_long.rs` against the `ta-lib` crate (a native port of the C library)
//! on 20,000 bars, both to 1e-9. A zero true-range sum is tested exactly, as current TA-Lib
//! does (TA-Lib issue 253), not against the old fixed `TA_IS_ZERO` band. `examples/ta_migration.rs`
//! shows the migration end to end.
//!
//! # Where results intentionally differ from `ta`
//!
//! | indicator | `ta` 0.5 | here (TA-Lib) |
//! |---|---|---|
//! | all | a value from the first input, computed on a partial window | `NaN` until the window is full; first value on bar `period` (bar `period + 1` for those that need a previous close) |
//! | `ExponentialMovingAverage` | seeded with the first input | seeded with the SMA of the first `period` inputs |
//! | `RelativeStrengthIndex` | EMA of gains and losses with `k = 2 / (period + 1)`, both seeded with 0.1 | Wilder: simple mean of the first `period` changes, then `k = 1 / period` (TA-Lib `RSI`); 100 when there are no losses in the window |
//! | `AverageTrueRange` | EMA (`k = 2 / (period + 1)`) of the true range, first bar's `high - low` included | Wilder: the first bar is skipped, the next `period` true ranges are averaged, then `k = 1 / period` (TA-Lib `ATR`) |
//! | `MovingAverageConvergenceDivergence` | three EMAs seeded with the first input | TA-Lib `MACD` seeding; `macd` from bar `slow`, `signal` and `histogram` from bar `slow + signal - 1` |
//! | `PercentagePriceOscillator` | three EMAs seeded with the first input | TA-Lib `PPO` (EMA) for the `ppo` line, an SMA-seeded EMA signal line; `ppo` from bar `slow`, `signal` and `histogram` from bar `slow + signal - 1` |
//! | `CommodityChannelIndex` | divides by the mean deviation of the close | divides by the mean deviation of the typical price (Lambert's definition, TA-Lib `CCI`) |
//! | `KeltnerChannel` | EMA of the typical price ± multiplier × `ta`'s ATR | SMA-seeded EMA of the typical price ± multiplier × Wilder ATR |
//! | `SlowStochastic` | EMA of %K seeded with the first %K | TA-Lib `STOCH` slow-K with an EMA (`slowk_matype = 1`) |
//! | `ChandelierExit` | uses `ta`'s ATR | uses the Wilder ATR |
//! | `RateOfChange` | warm-up values compare with the first input; a zero reference gives ±inf or NaN | NaN for `period` bars; a zero reference gives 0 (TA-Lib) |
//! | `EfficiencyRatio` | a window without movement gives NaN (0 / 0) | 0 |
//! | `MoneyFlowIndex` | 50 on the first bar, partial windows after | NaN until `period + 1` bars; 100 when the window has no negative flow |
//! | `BollingerBands` | accepts any multiplier | a non-finite multiplier is `TaError::InvalidParameter` |
//!
//! Unchanged once warmed up: `SimpleMovingAverage`, `StandardDeviation` (population),
//! `MeanAbsoluteDeviation`, `BollingerBands`, `Maximum`, `Minimum`, `FastStochastic`
//! (50 on a flat window, as in `ta`; TA-Lib gives 0), `RateOfChange`,
//! `EfficiencyRatio`, and
//! `TrueRange` and `OnBalanceVolume` (which have no warm-up: `TrueRange` gives
//! `high - low` on the first bar and `OnBalanceVolume` starts at the first bar's volume,
//! as both `ta` and TA-Lib do for positive prices).
//!
//! The structs do not derive serde's traits (`ta` does behind its `serde` feature).
//!
//! # Example
//! ```rust
//! use fin_primitives::ta_compat::indicators::{AverageTrueRange, RelativeStrengthIndex};
//! use fin_primitives::ta_compat::{DataItem, Next};
//!
//! let mut rsi = RelativeStrengthIndex::new(14).unwrap();
//! let closes = [44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42,
//!               45.84, 46.08, 45.89, 46.03, 45.61, 46.28, 46.28];
//! let values: Vec<f64> = closes.iter().map(|&c| rsi.next(c)).collect();
//! assert!(values[..14].iter().all(|v| v.is_nan()));   // warm-up
//! assert!((values[14] - 70.464_135_021).abs() < 1e-6); // TA-Lib RSI(14)
//!
//! let mut atr = AverageTrueRange::new(2).unwrap();
//! for (h, l, c) in [(10.0, 8.0, 9.0), (11.0, 9.0, 10.0), (12.0, 8.0, 11.0)] {
//!     let bar = DataItem::builder().open(c).high(h).low(l).close(c).volume(0.0).build().unwrap();
//!     let _ = atr.next(&bar);
//! }
//! assert!(atr.is_ready());
//! ```

pub mod indicators;

/// `ta`'s error type and `Result` alias.
pub mod errors {
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    /// `Result` with [`TaError`], as in `ta::errors::Result`.
    pub type Result<T> = std::result::Result<T, TaError>;

    /// The errors `ta`'s constructors and [`DataItem`](super::DataItem) builder return.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TaError {
        /// A period was zero or a multiplier was not finite.
        InvalidParameter,
        /// A [`DataItem`](super::DataItem) builder was missing a field.
        DataItemIncomplete,
        /// A [`DataItem`](super::DataItem) builder had inconsistent prices or a
        /// negative volume.
        DataItemInvalid,
    }

    impl Display for TaError {
        fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
            match *self {
                TaError::InvalidParameter => write!(f, "invalid parameter"),
                TaError::DataItemIncomplete => write!(f, "data item is incomplete"),
                TaError::DataItemInvalid => write!(f, "data item is invalid"),
            }
        }
    }

    impl Error for TaError {}

    impl From<TaError> for crate::error::FinError {
        fn from(e: TaError) -> Self {
            crate::error::FinError::InvalidInput(e.to_string())
        }
    }
}

use crate::interop::decimal_to_f64;
pub use errors::TaError;

/// Resets an indicator to its initial state, as `ta::Reset`.
pub trait Reset {
    /// Clears all state.
    fn reset(&mut self);
}

/// The look-back period of an indicator, as `ta::Period`.
pub trait Period {
    /// The period.
    fn period(&self) -> usize;
}

/// Feeds one input to an indicator and returns its output, as `ta::Next`.
///
/// Indicators implement `Next<f64>` (a close price or other single value) and/or
/// `Next<&T>` for bars that implement the input traits they need.
pub trait Next<T> {
    /// The indicator's output: `f64` or an output struct.
    type Output;
    /// Feeds `input` and returns the new output (`NaN` fields during warm-up).
    fn next(&mut self, input: T) -> Self::Output;
}

/// A bar's open price, as `ta::Open`.
pub trait Open {
    /// The open price.
    fn open(&self) -> f64;
}

/// A bar's close price, as `ta::Close`.
pub trait Close {
    /// The close price.
    fn close(&self) -> f64;
}

/// A bar's low price, as `ta::Low`.
pub trait Low {
    /// The low price.
    fn low(&self) -> f64;
}

/// A bar's high price, as `ta::High`.
pub trait High {
    /// The high price.
    fn high(&self) -> f64;
}

/// A bar's volume, as `ta::Volume`.
pub trait Volume {
    /// The volume.
    fn volume(&self) -> f64;
}

/// An OHLCV bar of `f64`s, as `ta::DataItem`.
///
/// Built with [`DataItem::builder`], which checks the same things `ta` does: all five
/// fields set, `low <= open, close <= high`, `low >= 0` and `volume >= 0`.
///
/// ```rust
/// use fin_primitives::ta_compat::{Close, DataItem, TaError};
///
/// let bar = DataItem::builder().open(10.0).high(12.0).low(9.0).close(11.0).volume(500.0).build().unwrap();
/// assert_eq!(bar.close(), 11.0);
/// let bad = DataItem::builder().open(10.0).high(9.0).low(9.5).close(9.8).volume(1.0).build();
/// assert_eq!(bad.unwrap_err(), TaError::DataItemInvalid);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct DataItem {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl DataItem {
    /// Starts a [`DataItemBuilder`].
    pub fn builder() -> DataItemBuilder {
        DataItemBuilder::new()
    }
}

impl Open for DataItem {
    fn open(&self) -> f64 {
        self.open
    }
}

impl High for DataItem {
    fn high(&self) -> f64 {
        self.high
    }
}

impl Low for DataItem {
    fn low(&self) -> f64 {
        self.low
    }
}

impl Close for DataItem {
    fn close(&self) -> f64 {
        self.close
    }
}

impl Volume for DataItem {
    fn volume(&self) -> f64 {
        self.volume
    }
}

/// Builder for [`DataItem`], as `ta::DataItemBuilder` (reachable as
/// `ta::data_item::DataItemBuilder` there; here it is at the module root).
#[derive(Debug, Clone, Default)]
pub struct DataItemBuilder {
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
    volume: Option<f64>,
}

impl DataItemBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the open price.
    #[must_use]
    pub fn open(mut self, val: f64) -> Self {
        self.open = Some(val);
        self
    }

    /// Sets the high price.
    #[must_use]
    pub fn high(mut self, val: f64) -> Self {
        self.high = Some(val);
        self
    }

    /// Sets the low price.
    #[must_use]
    pub fn low(mut self, val: f64) -> Self {
        self.low = Some(val);
        self
    }

    /// Sets the close price.
    #[must_use]
    pub fn close(mut self, val: f64) -> Self {
        self.close = Some(val);
        self
    }

    /// Sets the volume.
    #[must_use]
    pub fn volume(mut self, val: f64) -> Self {
        self.volume = Some(val);
        self
    }

    /// Builds the bar.
    ///
    /// # Errors
    /// [`TaError::DataItemIncomplete`] if a field is missing;
    /// [`TaError::DataItemInvalid`] unless `low <= open, close <= high`, `low >= 0` and
    /// `volume >= 0` (NaN fails these checks).
    pub fn build(self) -> errors::Result<DataItem> {
        let (Some(open), Some(high), Some(low), Some(close), Some(volume)) =
            (self.open, self.high, self.low, self.close, self.volume)
        else {
            return Err(TaError::DataItemIncomplete);
        };
        if low <= open
            && low <= close
            && low <= high
            && high >= open
            && high >= close
            && volume >= 0.0
            && low >= 0.0
        {
            Ok(DataItem { open, high, low, close, volume })
        } else {
            Err(TaError::DataItemInvalid)
        }
    }
}

macro_rules! impl_input_traits {
    ($ty:ty, $open:expr, $high:expr, $low:expr, $close:expr, $volume:expr) => {
        impl Open for $ty {
            fn open(&self) -> f64 {
                let f: fn(&$ty) -> f64 = $open;
                f(self)
            }
        }
        impl High for $ty {
            fn high(&self) -> f64 {
                let f: fn(&$ty) -> f64 = $high;
                f(self)
            }
        }
        impl Low for $ty {
            fn low(&self) -> f64 {
                let f: fn(&$ty) -> f64 = $low;
                f(self)
            }
        }
        impl Close for $ty {
            fn close(&self) -> f64 {
                let f: fn(&$ty) -> f64 = $close;
                f(self)
            }
        }
        impl Volume for $ty {
            fn volume(&self) -> f64 {
                let f: fn(&$ty) -> f64 = $volume;
                f(self)
            }
        }
    };
}

impl_input_traits!(
    crate::ohlcv::OhlcvBar,
    |b| decimal_to_f64(b.open.value()),
    |b| decimal_to_f64(b.high.value()),
    |b| decimal_to_f64(b.low.value()),
    |b| decimal_to_f64(b.close.value()),
    |b| decimal_to_f64(b.volume.value())
);

impl_input_traits!(
    crate::signals::BarInput,
    |b| decimal_to_f64(b.open),
    |b| decimal_to_f64(b.high),
    |b| decimal_to_f64(b.low),
    |b| decimal_to_f64(b.close),
    |b| decimal_to_f64(b.volume)
);

#[cfg(feature = "ta")]
impl_input_traits!(
    ::ta::DataItem,
    ::ta::Open::open,
    ::ta::High::high,
    ::ta::Low::low,
    ::ta::Close::close,
    ::ta::Volume::volume
);
