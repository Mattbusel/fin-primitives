//! # Module: python
//!
//! ## Responsibility
//! PyO3 Python bindings for core fin-primitives types and indicators.
//!
//! Enabled via the `python` Cargo feature. When the feature is active, this module
//! exposes a `_fin_primitives` Python extension module containing:
//! - `PyPrice`, `PyQuantity`, `PyOHLCV` — validated newtype wrappers
//! - `PyOrderBook` — L2 order book with Python-friendly methods
//! - `PySMA`, `PyEMA`, `PyRSI` — streaming indicators
//!
//! ## Build
//! Requires `maturin`. See the `python/` directory for a `pyproject.toml`
//! and `maturin develop` instructions.
//!
//! ## NOT Responsible For
//! - GIL management beyond pyo3 defaults
//! - Numpy interop (callers can convert scalars as needed)

#![cfg(feature = "python")]

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use rust_decimal::prelude::FromStr;
use rust_decimal::Decimal;

use crate::orderbook::{BookDelta, DeltaAction, OrderBook};
use crate::signals::indicators::{ema::Ema, rsi::Rsi, sma::Sma};
use crate::signals::{BarInput, Signal};
use crate::types::{Price, Quantity, Side, Symbol};

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

#[pymethods]
impl PyPrice {
    /// Construct a `PyPrice` from a float or string.
    #[new]
    pub fn new(value: f64) -> PyResult<Self> {
        let d = Decimal::from_f64_retain(value)
            .ok_or_else(|| PyValueError::new_err(format!("cannot convert {value} to Decimal")))?;
        let p = Price::new(d).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: p })
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

#[pymethods]
impl PyQuantity {
    #[new]
    pub fn new(value: f64) -> PyResult<Self> {
        let d = Decimal::from_f64_retain(value)
            .ok_or_else(|| PyValueError::new_err(format!("cannot convert {value} to Decimal")))?;
        let q = Quantity::new(d).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: q })
    }

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
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[pymethods]
impl PyOHLCV {
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

    #[getter]
    pub fn open(&self) -> f64 { self.open }
    #[getter]
    pub fn high(&self) -> f64 { self.high }
    #[getter]
    pub fn low(&self) -> f64 { self.low }
    #[getter]
    pub fn close(&self) -> f64 { self.close }
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
    pub fn apply_delta(
        &mut self,
        side: &str,
        price: f64,
        qty: f64,
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
        let pd = Decimal::from_str(&price.to_string())
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let qd = Decimal::from_str(&qty.to_string())
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
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
    #[new]
    pub fn new(name: &str, period: usize) -> PyResult<Self> {
        let inner = Sma::new(name, period).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Feed one bar's close price and return the SMA value, or `None` if not ready.
    pub fn update(&mut self, close: f64) -> PyResult<Option<f64>> {
        use rust_decimal::prelude::ToPrimitive;
        let d = Decimal::from_f64_retain(close)
            .ok_or_else(|| PyValueError::new_err("cannot convert close to Decimal"))?;
        let bar = BarInput::from_close(d);
        let sv = self.inner.update(&bar).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(sv.as_decimal().and_then(|v| v.to_f64()))
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
    #[new]
    pub fn new(name: &str, period: usize) -> PyResult<Self> {
        let inner = Ema::new(name, period).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Feed one bar's close price and return the EMA value, or `None` if not ready.
    pub fn update(&mut self, close: f64) -> PyResult<Option<f64>> {
        use rust_decimal::prelude::ToPrimitive;
        let d = Decimal::from_f64_retain(close)
            .ok_or_else(|| PyValueError::new_err("cannot convert close to Decimal"))?;
        let bar = BarInput::from_close(d);
        let sv = self.inner.update(&bar).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(sv.as_decimal().and_then(|v| v.to_f64()))
    }

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
    #[new]
    pub fn new(name: &str, period: usize) -> PyResult<Self> {
        let inner = Rsi::new(name, period).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Feed one bar's close price and return the RSI value (0–100), or `None` if not ready.
    pub fn update(&mut self, close: f64) -> PyResult<Option<f64>> {
        use rust_decimal::prelude::ToPrimitive;
        let d = Decimal::from_f64_retain(close)
            .ok_or_else(|| PyValueError::new_err("cannot convert close to Decimal"))?;
        let bar = BarInput::from_close(d);
        let sv = self.inner.update(&bar).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(sv.as_decimal().and_then(|v| v.to_f64()))
    }

    #[getter]
    pub fn name(&self) -> &str {
        self.inner.name()
    }

    fn __repr__(&self) -> String {
        format!("PyRSI(name={})", self.inner.name())
    }
}

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
    Ok(())
}
