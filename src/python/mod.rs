//! PyO3 Python bindings (enabled by the `python` feature).
//!
//! ## Responsibility
//! The native half of the `fin-primitives` Python package. When the feature is active,
//! this module builds the `fin_primitives._fin_primitives` extension module containing:
//! - `_Indicator`: any of the 743 indicators in [`crate::signals::indicators`], chosen
//!   by key (`"rsi"`, `"macd"`, `"bollinger_b"`, ...), with exact (`decimal.Decimal`) and
//!   float streaming updates and batch updates; the catalogue is generated from the Rust
//!   sources by `python/tools/gen_registry.py`
//! - `catalogue()`, `aliases()`, `resolve()`: introspection of that catalogue
//! - `PyPrice`, `PyQuantity`, `PyOHLCV`: validated value types
//! - `PyOrderBook`: L2 order book
//! - `PySMA`, `PyEMA`, `PyRSI`: the original float-returning streaming indicators
//!
//! Prices and parameters accept `str`, `int`, `float` or `decimal.Decimal`. A float is
//! read as the shortest decimal that prints as the same float, so `0.1` is exactly `0.1`.
//!
//! The pure-Python package in `python/src/fin_primitives/` wraps these with keyword
//! arguments, `mode="exact" | "float"` and numpy / pandas / polars batch helpers.
//! See `docs/python.md` for the Python API.
//!
//! ## Build
//! ```text
//! cd python
//! maturin build      # abi3 wheel (CPython 3.9+) in ../target/wheels
//! maturin develop    # or install into the active virtualenv
//! ```
//! No `--release`: that flag selects the crate's `release` profile, which has
//! `panic = "abort"`, so a Rust panic would kill the interpreter. Without it,
//! `pyproject.toml` selects the `release-python` profile (optimised, `panic = "unwind"`),
//! and a panic becomes a Python exception. The `pyproject.toml` also enables this feature
//! and `pyo3/abi3-py39`; the default Rust build does not depend on pyo3.
//!
//! ## NOT Responsible For
//! - numpy / pandas / polars types (handled in Python; no hard dependency)


mod args;
mod indicator;
mod registry;

pub use indicator::PyIndicator;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use rust_decimal::Decimal;

use args::{decimal_to_py, to_decimal};

use crate::orderbook::{BookDelta, DeltaAction, OrderBook};
use crate::signals::indicators::{ema::Ema, rsi::Rsi, sma::Sma};
use crate::signals::{BarInput, Signal};
use crate::types::{Price, Quantity, Side, Symbol};

/// Convert a Python float to the decimal it prints as (`0.1` becomes exactly `0.1`).
///
/// `Decimal::from_f64_retain`, used before 2.15, kept the binary rounding error, so
/// `PyPrice(0.1)` held `0.1000000000000000055511151231`.
fn py_float_to_decimal(value: f64) -> PyResult<Decimal> {
    args::f64_to_decimal(value)
}

/// Python-visible `Price` newtype.
///
/// Wraps a validated, strictly-positive `Decimal`.
///
/// # Example (Python)
/// ```python
/// from _fin_primitives import PyPrice
/// p = PyPrice(100.50)
/// print(p.value)  # "100.50"
/// ```
#[pyclass(name = "PyPrice")]
pub struct PyPrice {
    inner: Price,
}

impl PyPrice {
    /// Construct a `PyPrice` from a float (Rust-side constructor).
    pub fn new(value: f64) -> PyResult<Self> {
        let d = py_float_to_decimal(value)?;
        let p = Price::new(d).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: p })
    }
}

#[pymethods]
impl PyPrice {
    /// Construct a `PyPrice` from a `str`, `int`, `float` or `decimal.Decimal`.
    #[new]
    fn py_new(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let p = Price::new(to_decimal(value)?).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: p })
    }

    /// The exact price as a `decimal.Decimal`.
    #[getter]
    fn decimal<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        decimal_to_py(py, self.inner.value())
    }

    /// The price as a Python float.
    #[getter]
    pub fn value(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.inner.value().to_f64().unwrap_or(f64::NAN)
    }

    /// String representation with full decimal precision.
    fn __repr__(&self) -> String {
        format!("PyPrice({})", self.inner.value())
    }

    fn __str__(&self) -> String {
        self.inner.value().to_string()
    }
}

/// Python-visible `Quantity` newtype.
///
/// Non-negative decimal quantity.
///
/// # Example (Python)
/// ```python
/// from _fin_primitives import PyQuantity
/// q = PyQuantity(100.0)
/// print(q.value)  # 100.0
/// ```
#[pyclass(name = "PyQuantity")]
pub struct PyQuantity {
    inner: Quantity,
}

impl PyQuantity {
    /// Create a new `PyQuantity` from a float (Rust-side constructor).
    pub fn new(value: f64) -> PyResult<Self> {
        let d = py_float_to_decimal(value)?;
        let q = Quantity::new(d).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: q })
    }
}

#[pymethods]
impl PyQuantity {
    /// Create a new `PyQuantity` from a `str`, `int`, `float` or `decimal.Decimal`;
    /// raises `ValueError` on a negative quantity.
    #[new]
    fn py_new(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let q = Quantity::new(to_decimal(value)?).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: q })
    }

    /// The exact quantity as a `decimal.Decimal`.
    #[getter]
    fn decimal<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        decimal_to_py(py, self.inner.value())
    }

    /// The `value` value.
    #[getter]
    pub fn value(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.inner.value().to_f64().unwrap_or(f64::NAN)
    }

    fn __repr__(&self) -> String {
        format!("PyQuantity({})", self.inner.value())
    }

    fn __str__(&self) -> String {
        self.inner.value().to_string()
    }
}

/// Python-visible OHLCV bar.
///
/// # Example (Python)
/// ```python
/// from _fin_primitives import PyOHLCV
/// bar = PyOHLCV(open=100.0, high=105.0, low=99.0, close=103.0, volume=1000.0)
/// print(bar.close)  # 103.0
/// ```
#[pyclass(name = "PyOHLCV")]
pub struct PyOHLCV {
    /// Bar open.
    pub open: f64,
    /// Bar high.
    pub high: f64,
    /// Bar low.
    pub low: f64,
    /// Bar close.
    pub close: f64,
    /// Bar volume.
    pub volume: f64,
}

#[pymethods]
impl PyOHLCV {
    /// Create a new `PyOHLCV`; raises `ValueError` on invalid input.
    #[new]
    #[pyo3(signature = (open, high, low, close, volume))]
    pub fn new(open: f64, high: f64, low: f64, close: f64, volume: f64) -> PyResult<Self> {
        if high < open || high < close || low > open || low > close || high < low {
            return Err(PyValueError::new_err(
                "OHLCV invariant violated: check that high >= open/close and low <= open/close",
            ));
        }
        if volume < 0.0 {
            return Err(PyValueError::new_err("volume must be non-negative"));
        }
        Ok(Self { open, high, low, close, volume })
    }

    /// The `open` value.
    #[getter]
    pub fn open(&self) -> f64 { self.open }
    /// The `high` value.
    #[getter]
    pub fn high(&self) -> f64 { self.high }
    /// The `low` value.
    #[getter]
    pub fn low(&self) -> f64 { self.low }
    /// The `close` value.
    #[getter]
    pub fn close(&self) -> f64 { self.close }
    /// The `volume` value.
    #[getter]
    pub fn volume(&self) -> f64 { self.volume }

    /// Typical price: (high + low + close) / 3.
    pub fn typical_price(&self) -> f64 {
        (self.high + self.low + self.close) / 3.0
    }

    fn __repr__(&self) -> String {
        format!(
            "PyOHLCV(open={}, high={}, low={}, close={}, volume={})",
            self.open, self.high, self.low, self.close, self.volume
        )
    }
}

/// Python-visible L2 order book.
///
/// # Example (Python)
/// ```python
/// from _fin_primitives import PyOrderBook
/// book = PyOrderBook("AAPL")
/// print(book.mid_price())   # None (empty)
/// ```
#[pyclass(name = "PyOrderBook")]
pub struct PyOrderBook {
    inner: OrderBook,
}

#[pymethods]
impl PyOrderBook {
    /// Create a new empty order book for `symbol`.
    #[new]
    pub fn new(symbol: &str) -> PyResult<Self> {
        let sym = Symbol::new(symbol).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: OrderBook::new(sym) })
    }

    /// Best bid price, or `None` if empty.
    pub fn best_bid(&self) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        self.inner.best_bid().and_then(|level| level.price.value().to_f64())
    }

    /// Best ask price, or `None` if empty.
    pub fn best_ask(&self) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        self.inner.best_ask().and_then(|level| level.price.value().to_f64())
    }

    /// Bid-ask spread (ask - bid), or `None` if either side is empty.
    pub fn bid_ask_spread(&self) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        let bid = self.inner.best_bid()?.price.value();
        let ask = self.inner.best_ask()?.price.value();
        (ask - bid).to_f64()
    }

    /// Mid-price: (bid + ask) / 2, or `None` if either side is empty.
    pub fn mid_price(&self) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        let bid = self.inner.best_bid()?.price.value();
        let ask = self.inner.best_ask()?.price.value();
        ((bid + ask) / Decimal::TWO).to_f64()
    }

    /// Order book imbalance: (bid_qty - ask_qty) / (bid_qty + ask_qty).
    ///
    /// Returns `None` if either side is empty or total quantity is zero.
    pub fn imbalance(&self) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        let bid_qty: Decimal = self.inner.top_bids(usize::MAX).iter().map(|l| l.quantity.value()).sum();
        let ask_qty: Decimal = self.inner.top_asks(usize::MAX).iter().map(|l| l.quantity.value()).sum();
        let total = bid_qty + ask_qty;
        if total.is_zero() {
            return None;
        }
        ((bid_qty - ask_qty) / total).to_f64()
    }

    /// Apply a delta: `(side, price, qty, action_set, sequence)`.
    ///
    /// `side` must be `"bid"` or `"ask"`. `action_set` is `True` for Set, `False` for Remove.
    /// `price` and `qty` accept `str`, `int`, `float` or `decimal.Decimal`.
    #[pyo3(name = "apply_delta")]
    fn py_apply_delta(
        &mut self,
        side: &str,
        price: &Bound<'_, PyAny>,
        qty: &Bound<'_, PyAny>,
        action_set: bool,
        sequence: u64,
    ) -> PyResult<()> {
        self.apply_decimal_delta(side, to_decimal(price)?, to_decimal(qty)?, action_set, sequence)
    }

    /// Current sequence number.
    pub fn sequence(&self) -> u64 {
        self.inner.sequence()
    }

    fn __repr__(&self) -> String {
        format!(
            "PyOrderBook(symbol={}, seq={}, bid={:?}, ask={:?})",
            self.inner.symbol.as_str(),
            self.inner.sequence(),
            self.best_bid(),
            self.best_ask(),
        )
    }
}

impl PyOrderBook {
    /// Apply a delta with float price and quantity (Rust-side API, kept from 2.15).
    ///
    /// `side` must be `"bid"` or `"ask"`. `action_set` is `true` for Set, `false` for Remove.
    pub fn apply_delta(
        &mut self,
        side: &str,
        price: f64,
        qty: f64,
        action_set: bool,
        sequence: u64,
    ) -> PyResult<()> {
        let (pd, qd) = (py_float_to_decimal(price)?, py_float_to_decimal(qty)?);
        self.apply_decimal_delta(side, pd, qd, action_set, sequence)
    }

    fn apply_decimal_delta(
        &mut self,
        side: &str,
        pd: Decimal,
        qd: Decimal,
        action_set: bool,
        sequence: u64,
    ) -> PyResult<()> {
        let s = match side.to_lowercase().as_str() {
            "bid" => Side::Bid,
            "ask" => Side::Ask,
            other => {
                return Err(PyValueError::new_err(format!("side must be 'bid' or 'ask', got '{other}'")));
            }
        };
        let price_t = crate::types::Price::new(pd).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let qty_t = crate::types::Quantity::new(qd).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let delta = BookDelta {
            side: s,
            price: price_t,
            quantity: qty_t,
            action: if action_set { DeltaAction::Set } else { DeltaAction::Remove },
            sequence,
        };
        self.inner.apply_delta(delta).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

/// Python-visible Simple Moving Average.
///
/// # Example (Python)
/// ```python
/// from _fin_primitives import PySMA
/// sma = PySMA("sma_20", 20)
/// result = sma.update(close=105.0)
/// print(result)  # None until 20 bars seen
/// ```
#[pyclass(name = "PySMA")]
pub struct PySMA {
    inner: Sma,
}

#[pymethods]
impl PySMA {
    /// Create a new `PySMA`; raises `ValueError` on invalid input.
    #[new]
    pub fn new(name: &str, period: usize) -> PyResult<Self> {
        let inner = Sma::new(name, period).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Feed one bar's close price and return the SMA value, or `None` if not ready.
    ///
    /// `close` accepts `str`, `int`, `float` or `decimal.Decimal`.
    #[pyo3(name = "update")]
    fn py_update(&mut self, close: &Bound<'_, PyAny>) -> PyResult<Option<f64>> {
        self.update_decimal(to_decimal(close)?)
    }

    /// Name of this indicator.
    #[getter]
    pub fn name(&self) -> &str {
        self.inner.name()
    }

    fn __repr__(&self) -> String {
        format!("PySMA(name={})", self.inner.name())
    }
}

/// Python-visible Exponential Moving Average.
///
/// # Example (Python)
/// ```python
/// from _fin_primitives import PyEMA
/// ema = PyEMA("ema_12", 12)
/// result = ema.update(close=100.0)
/// ```
#[pyclass(name = "PyEMA")]
pub struct PyEMA {
    inner: Ema,
}

#[pymethods]
impl PyEMA {
    /// Create a new `PyEMA`; raises `ValueError` on invalid input.
    #[new]
    pub fn new(name: &str, period: usize) -> PyResult<Self> {
        let inner = Ema::new(name, period).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Feed one bar's close price and return the EMA value, or `None` if not ready.
    ///
    /// `close` accepts `str`, `int`, `float` or `decimal.Decimal`.
    #[pyo3(name = "update")]
    fn py_update(&mut self, close: &Bound<'_, PyAny>) -> PyResult<Option<f64>> {
        self.update_decimal(to_decimal(close)?)
    }

    /// The `name` value.
    #[getter]
    pub fn name(&self) -> &str {
        self.inner.name()
    }

    fn __repr__(&self) -> String {
        format!("PyEMA(name={})", self.inner.name())
    }
}

/// Python-visible Relative Strength Index.
///
/// # Example (Python)
/// ```python
/// from _fin_primitives import PyRSI
/// rsi = PyRSI("rsi_14", 14)
/// result = rsi.update(close=50.0)
/// ```
#[pyclass(name = "PyRSI")]
pub struct PyRSI {
    inner: Rsi,
}

#[pymethods]
impl PyRSI {
    /// Create a new `PyRSI`; raises `ValueError` on invalid input.
    #[new]
    pub fn new(name: &str, period: usize) -> PyResult<Self> {
        let inner = Rsi::new(name, period).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Feed one bar's close price and return the RSI value (0–100), or `None` if not ready.
    ///
    /// `close` accepts `str`, `int`, `float` or `decimal.Decimal`.
    #[pyo3(name = "update")]
    fn py_update(&mut self, close: &Bound<'_, PyAny>) -> PyResult<Option<f64>> {
        self.update_decimal(to_decimal(close)?)
    }

    /// The `name` value.
    #[getter]
    pub fn name(&self) -> &str {
        self.inner.name()
    }

    fn __repr__(&self) -> String {
        format!("PyRSI(name={})", self.inner.name())
    }
}

macro_rules! float_update {
    ($($ty:ty),*) => {$(
        impl $ty {
            /// Feed one bar's close price and return the value, or `None` if not ready
            /// (Rust-side API, kept from 2.15).
            pub fn update(&mut self, close: f64) -> PyResult<Option<f64>> {
                self.update_decimal(py_float_to_decimal(close)?)
            }

            fn update_decimal(&mut self, close: Decimal) -> PyResult<Option<f64>> {
                use rust_decimal::prelude::ToPrimitive;
                let bar = BarInput::from_close(close);
                let sv = self.inner.update(&bar).map_err(|e| PyValueError::new_err(e.to_string()))?;
                Ok(sv.as_decimal().and_then(|v| v.to_f64()))
            }
        }
    )*};
}

float_update!(PySMA, PyEMA, PyRSI);

/// Registers all Python-visible types and creates the `_fin_primitives` module.
#[pymodule]
pub fn _fin_primitives(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyPrice>()?;
    m.add_class::<PyQuantity>()?;
    m.add_class::<PyOHLCV>()?;
    m.add_class::<PyOrderBook>()?;
    m.add_class::<PySMA>()?;
    m.add_class::<PyEMA>()?;
    m.add_class::<PyRSI>()?;
    m.add_class::<PyIndicator>()?;
    m.add_function(wrap_pyfunction!(indicator::catalogue, m)?)?;
    m.add_function(wrap_pyfunction!(indicator::aliases, m)?)?;
    m.add_function(wrap_pyfunction!(indicator::resolve, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
