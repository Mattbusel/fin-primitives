//! The generic `Indicator` binding over the whole catalogue in [`super::registry`].
//!
//! Python code normally uses the `fin_primitives.Indicator` wrapper, which adds keyword
//! arguments, the `mode` switch and numpy / pandas / polars batch helpers on top of this class.

use std::sync::Mutex;

use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple};
use rust_decimal::Decimal;

use super::args::{decimal_to_f64, decimal_to_py, f64_to_decimal, f64_to_decimal_opt, fin_err, to_decimal};
use super::registry::{Entry, ALIASES, ENTRIES};
use crate::signals::{BarInput, Signal, SignalValue};

/// Normalises a user-supplied key: lower case, `-` and spaces become `_`.
fn normalise(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

/// Finds a catalogue entry by key or alias.
pub(crate) fn lookup(kind: &str) -> PyResult<&'static Entry> {
    let k = normalise(kind);
    let key = ALIASES.iter().find(|(a, _)| *a == k).map_or(k.as_str(), |(_, key)| *key);
    if let Ok(i) = ENTRIES.binary_search_by(|e| e.key.cmp(key)) {
        return Ok(&ENTRIES[i]);
    }
    // Suggestions: keys starting with the input first, then keys containing it.
    let keys = || ENTRIES.iter().map(|e| e.key);
    let close: Vec<&str> = keys()
        .filter(|e| e.starts_with(key))
        .chain(keys().filter(|e| !e.starts_with(key) && e.contains(key)))
        .take(8)
        .collect();
    let hint = if close.is_empty() {
        String::from("; fin_primitives.catalogue() lists every key")
    } else {
        format!("; did you mean one of: {}", close.join(", "))
    };
    Err(PyKeyError::new_err(format!("unknown indicator {kind:?}{hint}")))
}

fn value_or_none(v: SignalValue) -> Option<Decimal> {
    match v {
        SignalValue::Scalar(d) => Some(d),
        SignalValue::Unavailable => None,
    }
}

/// Reads native-endian f64s from a bytes buffer.
fn read_f64s(buf: &[u8], what: &str) -> PyResult<Vec<f64>> {
    if buf.len() % 8 != 0 {
        return Err(PyValueError::new_err(format!("{what}: buffer length is not a multiple of 8")));
    }
    Ok(buf
        .chunks_exact(8)
        .map(|c| f64::from_ne_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect())
}

/// A streaming indicator from the catalogue, chosen by key.
///
/// Exact methods (`update`, `update_bar`, `batch`) take `str`, `int`, `float` or
/// `decimal.Decimal` and return `decimal.Decimal` (or `None` while warming up). The `_f64`
/// methods take and return floats; the arithmetic inside is the same exact decimal code.
#[pyclass(name = "_Indicator", module = "fin_primitives._fin_primitives")]
pub struct PyIndicator {
    inner: Mutex<Box<dyn Signal>>,
    entry: &'static Entry,
    params: String,
}

impl PyIndicator {
    fn signal(&mut self) -> &mut Box<dyn Signal> {
        self.inner.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn step(&mut self, bar: &BarInput) -> PyResult<Option<Decimal>> {
        self.signal().update(bar).map(value_or_none).map_err(fin_err)
    }
}

fn bar_from(
    close: Decimal,
    high: Option<Decimal>,
    low: Option<Decimal>,
    open: Option<Decimal>,
    volume: Option<Decimal>,
) -> BarInput {
    BarInput::new(
        close,
        high.unwrap_or(close),
        low.unwrap_or(close),
        open.unwrap_or(close),
        volume.unwrap_or(Decimal::ZERO),
    )
}

fn opt_decimal(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Decimal>> {
    match obj {
        Some(o) if !o.is_none() => to_decimal(o).map(Some),
        _ => Ok(None),
    }
}

fn collect_column(obj: Option<&Bound<'_, PyAny>>, what: &str, n: Option<usize>) -> PyResult<Option<Vec<Decimal>>> {
    let Some(o) = obj else { return Ok(None) };
    if o.is_none() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(n.unwrap_or(0));
    for (i, item) in o.try_iter()?.enumerate() {
        let item = item?;
        let d = to_decimal(&item).map_err(|e| {
            let py = o.py();
            PyErr::from_type(e.get_type(py), format!("{what}[{i}]: {}", e.value(py)))
        })?;
        out.push(d);
    }
    if let Some(n) = n {
        if out.len() != n {
            return Err(PyValueError::new_err(format!("{what} has {} values, close has {n}", out.len())));
        }
    }
    Ok(Some(out))
}

#[pymethods]
impl PyIndicator {
    /// Builds the indicator `kind` with positional constructor parameters.
    #[new]
    #[pyo3(signature = (kind, *params))]
    fn new(kind: &str, params: &Bound<'_, PyTuple>) -> PyResult<Self> {
        let entry = lookup(kind)?;
        let args: Vec<Bound<'_, PyAny>> = params.iter().collect();
        let signal = (entry.build)(entry.key, &args).map_err(|e| {
            let py = params.py();
            let names: Vec<&str> = entry.params.iter().map(|(n, _)| *n).collect();
            PyErr::from_type(
                e.get_type(py),
                format!("{}({}): {}", entry.key, names.join(", "), e.value(py)),
            )
        })?;
        let shown: Vec<String> = args
            .iter()
            .map(|a| a.repr().map(|r| r.to_string()).unwrap_or_else(|_| String::from("?")))
            .collect();
        Ok(Self { inner: Mutex::new(signal), entry, params: shown.join(", ") })
    }

    /// Catalogue key, e.g. `"rsi"`.
    #[getter]
    fn key(&self) -> &'static str {
        self.entry.key
    }

    /// Rust type name, e.g. `"Rsi"`.
    #[getter]
    fn type_name(&self) -> &'static str {
        self.entry.type_name
    }

    /// Name reported by the Rust indicator.
    #[getter]
    fn name(&mut self) -> String {
        self.signal().name().to_owned()
    }

    /// Look-back period reported by the Rust indicator.
    #[getter]
    fn period(&mut self) -> usize {
        self.signal().period()
    }

    /// `True` once the indicator has produced (or can produce) a value.
    #[getter]
    fn is_ready(&mut self) -> bool {
        self.signal().is_ready()
    }

    /// Clears all state, as if newly constructed.
    fn reset(&mut self) {
        self.signal().reset();
    }

    /// Feeds one close price (exact). Returns `decimal.Decimal` or `None` while warming up.
    fn update<'py>(&mut self, close: &Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let py = close.py();
        let bar = BarInput::from_close(to_decimal(close)?);
        self.step(&bar)?.map(|d| decimal_to_py(py, d)).transpose()
    }

    /// Feeds one bar (exact). Missing `high`/`low`/`open` default to `close`, `volume` to 0.
    #[pyo3(signature = (close, high=None, low=None, open=None, volume=None))]
    fn update_bar<'py>(
        &mut self,
        close: &Bound<'py, PyAny>,
        high: Option<&Bound<'py, PyAny>>,
        low: Option<&Bound<'py, PyAny>>,
        open: Option<&Bound<'py, PyAny>>,
        volume: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let py = close.py();
        let bar = bar_from(
            to_decimal(close)?,
            opt_decimal(high)?,
            opt_decimal(low)?,
            opt_decimal(open)?,
            opt_decimal(volume)?,
        );
        self.step(&bar)?.map(|d| decimal_to_py(py, d)).transpose()
    }

    /// Feeds one close price as a float. Returns a float or `None` while warming up.
    fn update_f64(&mut self, close: f64) -> PyResult<Option<f64>> {
        let bar = BarInput::from_close(f64_to_decimal(close)?);
        Ok(self.step(&bar)?.map(decimal_to_f64))
    }

    /// Feeds one bar as floats. Returns a float or `None` while warming up.
    #[pyo3(signature = (close, high=None, low=None, open=None, volume=None))]
    fn update_bar_f64(
        &mut self,
        close: f64,
        high: Option<f64>,
        low: Option<f64>,
        open: Option<f64>,
        volume: Option<f64>,
    ) -> PyResult<Option<f64>> {
        let conv = |v: Option<f64>| v.map(f64_to_decimal).transpose();
        let bar = bar_from(f64_to_decimal(close)?, conv(high)?, conv(low)?, conv(open)?, conv(volume)?);
        Ok(self.step(&bar)?.map(decimal_to_f64))
    }

    /// Feeds a sequence of bars (exact) and returns a list of `decimal.Decimal` or `None`.
    ///
    /// Each argument is any iterable of `str`/`int`/`float`/`decimal.Decimal`; optional
    /// columns must have the same length as `close`.
    #[pyo3(signature = (close, high=None, low=None, open=None, volume=None))]
    fn batch<'py>(
        &mut self,
        close: &Bound<'py, PyAny>,
        high: Option<&Bound<'py, PyAny>>,
        low: Option<&Bound<'py, PyAny>>,
        open: Option<&Bound<'py, PyAny>>,
        volume: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = close.py();
        let close = collect_column(Some(close), "close", None)?.unwrap_or_default();
        let n = Some(close.len());
        let high = collect_column(high, "high", n)?;
        let low = collect_column(low, "low", n)?;
        let open = collect_column(open, "open", n)?;
        let volume = collect_column(volume, "volume", n)?;
        let pick = |col: &Option<Vec<Decimal>>, i: usize| col.as_ref().map(|c| c[i]);
        let out = PyList::empty(py);
        for (i, c) in close.iter().enumerate() {
            let bar = bar_from(*c, pick(&high, i), pick(&low, i), pick(&open, i), pick(&volume, i));
            match self.step(&bar).map_err(|e| PyValueError::new_err(format!("row {i}: {}", e.value(py))))? {
                Some(d) => out.append(decimal_to_py(py, d)?)?,
                None => out.append(py.None())?,
            }
        }
        Ok(out)
    }

    /// Fast path: feeds native-endian float64 buffers (`numpy.ndarray.tobytes()` or
    /// `array.array("d").tobytes()`) and returns a float64 buffer, NaN while warming up.
    ///
    /// Runs without holding the GIL. NaN or infinite inputs raise `ValueError`.
    #[pyo3(signature = (close, high=None, low=None, open=None, volume=None))]
    fn batch_f64<'py>(
        &mut self,
        py: Python<'py>,
        close: &[u8],
        high: Option<&[u8]>,
        low: Option<&[u8]>,
        open: Option<&[u8]>,
        volume: Option<&[u8]>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let close = read_f64s(close, "close")?;
        let n = close.len();
        let col = |b: Option<&[u8]>, what: &str| -> PyResult<Option<Vec<f64>>> {
            match b {
                None => Ok(None),
                Some(b) => {
                    let v = read_f64s(b, what)?;
                    if v.len() != n {
                        return Err(PyValueError::new_err(format!("{what} has {} values, close has {n}", v.len())));
                    }
                    Ok(Some(v))
                }
            }
        };
        let (high, low, open, volume) = (col(high, "high")?, col(low, "low")?, col(open, "open")?, col(volume, "volume")?);
        let signal = self.signal();
        let result: Result<Vec<u8>, String> = py.detach(move || {
            let mut out = Vec::with_capacity(n * 8);
            for i in 0..n {
                let get = |c: &Option<Vec<f64>>, what: &str| -> Result<Option<Decimal>, String> {
                    match c {
                        None => Ok(None),
                        Some(c) => to_dec(c[i], what, i).map(Some),
                    }
                };
                let bar = bar_from(
                    to_dec(close[i], "close", i)?,
                    get(&high, "high")?,
                    get(&low, "low")?,
                    get(&open, "open")?,
                    get(&volume, "volume")?,
                );
                let v = signal.update(&bar).map_err(|e| format!("row {i}: {e}"))?;
                let f = value_or_none(v).map_or(f64::NAN, decimal_to_f64);
                out.extend_from_slice(&f.to_ne_bytes());
            }
            Ok(out)
        });
        let bytes = result.map_err(PyValueError::new_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    fn __repr__(&self) -> String {
        format!("Indicator({:?}{}{})", self.entry.key, if self.params.is_empty() { "" } else { ", " }, self.params)
    }
}

/// f64 to Decimal without touching Python (used with the GIL released).
fn to_dec(v: f64, what: &str, i: usize) -> Result<Decimal, String> {
    if !v.is_finite() {
        return Err(format!("{what}[{i}] is {v}; drop or fill missing values first"));
    }
    f64_to_decimal_opt(v)
        .ok_or_else(|| format!("{what}[{i}] = {v:e} is out of the Decimal range (28 decimal places, |x| < 7.9e28)"))
}

/// Lists the catalogue: one dict per indicator with `key`, `type`, `params`
/// (a list of `(name, type)` pairs) and `doc`.
#[pyfunction]
pub fn catalogue(py: Python<'_>) -> PyResult<Bound<'_, PyList>> {
    let out = PyList::empty(py);
    for e in ENTRIES {
        let d = PyDict::new(py);
        d.set_item("key", e.key)?;
        d.set_item("type", e.type_name)?;
        d.set_item("params", e.params.to_vec())?;
        d.set_item("doc", e.doc)?;
        out.append(d)?;
    }
    Ok(out)
}

/// Alternative keys accepted by `Indicator`, as `{alias: key}`.
#[pyfunction]
pub fn aliases(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    for (a, k) in ALIASES {
        d.set_item(a, k)?;
    }
    Ok(d)
}

/// Resolves a key or alias to its catalogue key; raises `KeyError` if unknown.
#[pyfunction]
pub fn resolve(kind: &str) -> PyResult<&'static str> {
    lookup(kind).map(|e| e.key)
}
