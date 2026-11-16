//! Whole-series float64 batch functions for `fin_primitives.fast`.
//!
//! ## Responsibility
//! Thin numpy wrappers over [`crate::signals::batch_f64`]: each function borrows its
//! float64 input arrays without copying, releases the GIL, runs one O(n) pass and hands
//! the result vector to numpy without copying it again.
//!
//! ## NOT Responsible For
//! - Converting lists, pandas or polars input (the Python wrapper in `fast.py` does that)
//! - The indicator maths (see `signals::batch_f64`)

use numpy::{IntoPyArray, PyArray1, PyReadonlyArray1};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::signals::batch_f64 as k;

type Arr<'py> = Bound<'py, PyArray1<f64>>;

fn slice<'a>(a: &'a PyReadonlyArray1<'_, f64>, what: &str) -> PyResult<&'a [f64]> {
    a.as_slice().map_err(|_| PyValueError::new_err(format!("{what} must be a contiguous float64 array")))
}

fn positive(v: usize, what: &str) -> PyResult<()> {
    if v == 0 {
        return Err(PyValueError::new_err(format!("{what} must be at least 1")));
    }
    Ok(())
}

fn same_len(n: usize, a: &[f64], what: &str) -> PyResult<()> {
    if a.len() != n {
        return Err(PyValueError::new_err(format!("{what} has {} values, close has {n}", a.len())));
    }
    Ok(())
}

/// Simple moving average (pandas `rolling(period).mean()`).
#[pyfunction]
pub fn fast_sma<'py>(py: Python<'py>, close: PyReadonlyArray1<'py, f64>, period: usize) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::sma(x, period)).into_pyarray(py))
}

/// SMA-seeded exponential moving average (pandas-ta `ema`).
#[pyfunction]
pub fn fast_ema<'py>(py: Python<'py>, close: PyReadonlyArray1<'py, f64>, period: usize) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::ema(x, period)).into_pyarray(py))
}

/// SMA-seeded Wilder moving average (pandas-ta `rma`).
#[pyfunction]
pub fn fast_rma<'py>(py: Python<'py>, close: PyReadonlyArray1<'py, f64>, period: usize) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::rma(x, period)).into_pyarray(py))
}

/// Wilder RSI with TA-Lib seeding (pandas-ta `rsi`).
#[pyfunction]
pub fn fast_rsi<'py>(py: Python<'py>, close: PyReadonlyArray1<'py, f64>, period: usize) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::rsi(x, period)).into_pyarray(py))
}

fn atr_seed(seed: &str) -> PyResult<k::AtrSeed> {
    match seed {
        "talib" => Ok(k::AtrSeed::TaLib),
        "tradingview" => Ok(k::AtrSeed::TradingView),
        other => Err(PyValueError::new_err(format!("seed must be \"talib\" or \"tradingview\", not {other:?}"))),
    }
}

/// True range (pandas-ta `true_range`); `seed="tradingview"` gives bar 0 `high - low`.
#[pyfunction]
#[pyo3(signature = (high, low, close, seed = "talib"))]
pub fn fast_true_range<'py>(
    py: Python<'py>,
    high: PyReadonlyArray1<'py, f64>,
    low: PyReadonlyArray1<'py, f64>,
    close: PyReadonlyArray1<'py, f64>,
    seed: &str,
) -> PyResult<Arr<'py>> {
    let s = atr_seed(seed)?;
    let (h, l, c) = (slice(&high, "high")?, slice(&low, "low")?, slice(&close, "close")?);
    same_len(c.len(), h, "high")?;
    same_len(c.len(), l, "low")?;
    Ok(py.detach(|| k::true_range(h, l, c, s == k::AtrSeed::TradingView)).into_pyarray(py))
}

/// Wilder ATR, TA-Lib seed (pandas-ta `atr`) or TradingView seed (`ta.atr`).
#[pyfunction]
#[pyo3(signature = (high, low, close, period, seed = "talib"))]
pub fn fast_atr<'py>(
    py: Python<'py>,
    high: PyReadonlyArray1<'py, f64>,
    low: PyReadonlyArray1<'py, f64>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
    seed: &str,
) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let s = atr_seed(seed)?;
    let (h, l, c) = (slice(&high, "high")?, slice(&low, "low")?, slice(&close, "close")?);
    same_len(c.len(), h, "high")?;
    same_len(c.len(), l, "low")?;
    Ok(py.detach(|| k::atr(h, l, c, period, s)).into_pyarray(py))
}

/// MACD `(line, signal, histogram)` (pandas-ta / TA-Lib `macd`).
#[pyfunction]
pub fn fast_macd<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    fast: usize,
    slow: usize,
    signal: usize,
) -> PyResult<(Arr<'py>, Arr<'py>, Arr<'py>)> {
    positive(fast, "fast")?;
    positive(slow, "slow")?;
    positive(signal, "signal")?;
    let x = slice(&close, "close")?;
    let (a, b, c) = py.detach(|| k::macd(x, fast, slow, signal));
    Ok((a.into_pyarray(py), b.into_pyarray(py), c.into_pyarray(py)))
}

fn window_ddof(period: usize, ddof: usize) -> PyResult<()> {
    if period < 2 {
        return Err(PyValueError::new_err("period must be at least 2"));
    }
    if ddof >= period {
        return Err(PyValueError::new_err(format!("ddof must be less than period ({period}), got {ddof}")));
    }
    Ok(())
}

/// Rolling standard deviation (pandas-ta `stdev`; numpy `var` per window, then sqrt).
#[pyfunction]
#[pyo3(signature = (close, period, ddof = 0))]
pub fn fast_stdev<'py>(py: Python<'py>, close: PyReadonlyArray1<'py, f64>, period: usize, ddof: usize) -> PyResult<Arr<'py>> {
    window_ddof(period, ddof)?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::rolling_std(x, period, ddof)).into_pyarray(py))
}

/// Rolling variance (pandas-ta `variance`).
#[pyfunction]
#[pyo3(signature = (close, period, ddof = 0))]
pub fn fast_variance<'py>(py: Python<'py>, close: PyReadonlyArray1<'py, f64>, period: usize, ddof: usize) -> PyResult<Arr<'py>> {
    window_ddof(period, ddof)?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::rolling_var(x, period, ddof)).into_pyarray(py))
}

/// Bollinger Bands `(lower, mid, upper)` (pandas-ta `bbands`).
#[pyfunction]
#[pyo3(signature = (close, period, std, ddof = 0))]
pub fn fast_bbands<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
    std: f64,
    ddof: usize,
) -> PyResult<(Arr<'py>, Arr<'py>, Arr<'py>)> {
    window_ddof(period, ddof)?;
    if !(std.is_finite() && std > 0.0) {
        return Err(PyValueError::new_err(format!("std must be a positive number, got {std}")));
    }
    let x = slice(&close, "close")?;
    let (a, b, c) = py.detach(|| k::bbands(x, period, std, ddof));
    Ok((a.into_pyarray(py), b.into_pyarray(py), c.into_pyarray(py)))
}

/// Adds the `fast_*` functions to the extension module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fast_sma, m)?)?;
    m.add_function(wrap_pyfunction!(fast_ema, m)?)?;
    m.add_function(wrap_pyfunction!(fast_rma, m)?)?;
    m.add_function(wrap_pyfunction!(fast_rsi, m)?)?;
    m.add_function(wrap_pyfunction!(fast_true_range, m)?)?;
    m.add_function(wrap_pyfunction!(fast_atr, m)?)?;
    m.add_function(wrap_pyfunction!(fast_macd, m)?)?;
    m.add_function(wrap_pyfunction!(fast_stdev, m)?)?;
    m.add_function(wrap_pyfunction!(fast_variance, m)?)?;
    m.add_function(wrap_pyfunction!(fast_bbands, m)?)?;
    Ok(())
}
