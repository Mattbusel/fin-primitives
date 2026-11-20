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
    a.as_slice()
        .map_err(|_| PyValueError::new_err(format!("{what} must be a contiguous float64 array")))
}

fn positive(v: usize, what: &str) -> PyResult<()> {
    if v == 0 {
        return Err(PyValueError::new_err(format!("{what} must be at least 1")));
    }
    Ok(())
}

fn same_len(n: usize, a: &[f64], what: &str) -> PyResult<()> {
    if a.len() != n {
        return Err(PyValueError::new_err(format!(
            "{what} has {} values, close has {n}",
            a.len()
        )));
    }
    Ok(())
}

/// Simple moving average (pandas `rolling(period).mean()`).
#[pyfunction]
pub fn fast_sma<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::sma(x, period)).into_pyarray(py))
}

/// SMA-seeded exponential moving average (pandas-ta `ema`).
#[pyfunction]
pub fn fast_ema<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::ema(x, period)).into_pyarray(py))
}

/// SMA-seeded Wilder moving average (pandas-ta `rma`).
#[pyfunction]
pub fn fast_rma<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::rma(x, period)).into_pyarray(py))
}

/// Wilder RSI with TA-Lib seeding (pandas-ta `rsi`).
#[pyfunction]
pub fn fast_rsi<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
) -> PyResult<Arr<'py>> {
    positive(period, "period")?;
    let x = slice(&close, "close")?;
    Ok(py.detach(|| k::rsi(x, period)).into_pyarray(py))
}

fn atr_seed(seed: &str) -> PyResult<k::AtrSeed> {
    match seed {
        "talib" => Ok(k::AtrSeed::TaLib),
        "tradingview" => Ok(k::AtrSeed::TradingView),
        other => Err(PyValueError::new_err(format!(
            "seed must be \"talib\" or \"tradingview\", not {other:?}"
        ))),
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
    let (h, l, c) = (
        slice(&high, "high")?,
        slice(&low, "low")?,
        slice(&close, "close")?,
    );
    same_len(c.len(), h, "high")?;
    same_len(c.len(), l, "low")?;
    Ok(py
        .detach(|| k::true_range(h, l, c, s == k::AtrSeed::TradingView))
        .into_pyarray(py))
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
    let (h, l, c) = (
        slice(&high, "high")?,
        slice(&low, "low")?,
        slice(&close, "close")?,
    );
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
        return Err(PyValueError::new_err(format!(
            "ddof must be less than period ({period}), got {ddof}"
        )));
    }
    Ok(())
}

/// Rolling standard deviation (pandas-ta `stdev`; numpy `var` per window, then sqrt).
#[pyfunction]
#[pyo3(signature = (close, period, ddof = 0))]
pub fn fast_stdev<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
    ddof: usize,
) -> PyResult<Arr<'py>> {
    window_ddof(period, ddof)?;
    let x = slice(&close, "close")?;
    Ok(py
        .detach(|| k::rolling_std(x, period, ddof))
        .into_pyarray(py))
}

/// Rolling variance (pandas-ta `variance`).
#[pyfunction]
#[pyo3(signature = (close, period, ddof = 0))]
pub fn fast_variance<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    period: usize,
    ddof: usize,
) -> PyResult<Arr<'py>> {
    window_ddof(period, ddof)?;
    let x = slice(&close, "close")?;
    Ok(py
        .detach(|| k::rolling_var(x, period, ddof))
        .into_pyarray(py))
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
        return Err(PyValueError::new_err(format!(
            "std must be a positive number, got {std}"
        )));
    }
    let x = slice(&close, "close")?;
    let (a, b, c) = py.detach(|| k::bbands(x, period, std, ddof));
    Ok((a.into_pyarray(py), b.into_pyarray(py), c.into_pyarray(py)))
}

/// One-input, one-period kernels: `fast_rolling_sum` and friends.
macro_rules! period_fn {
    ($name:ident, $kernel:path, $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        pub fn $name<'py>(
            py: Python<'py>,
            x: PyReadonlyArray1<'py, f64>,
            period: usize,
        ) -> PyResult<Arr<'py>> {
            positive(period, "period")?;
            let x = slice(&x, "x")?;
            Ok(py.detach(|| $kernel(x, period)).into_pyarray(py))
        }
    };
}

period_fn!(
    fast_rolling_sum,
    k::rolling_sum,
    "Rolling sum (pandas `rolling(period).sum()`)."
);
period_fn!(
    fast_rolling_max,
    k::rolling_max,
    "Rolling maximum (pandas `rolling(period).max()`)."
);
period_fn!(
    fast_rolling_min,
    k::rolling_min,
    "Rolling minimum (pandas `rolling(period).min()`)."
);
period_fn!(
    fast_wma,
    k::wma,
    "Linearly weighted moving average (pandas-ta `wma`)."
);
period_fn!(
    fast_wilder_sum,
    k::wilder_sum,
    "Wilder's running sum (pandas-ta-classic `wilder_smooth`)."
);

/// Bars since the most recent maximum (`want_max`) or minimum in each window of `length`.
#[pyfunction]
pub fn fast_bars_since<'py>(
    py: Python<'py>,
    x: PyReadonlyArray1<'py, f64>,
    length: usize,
    want_max: bool,
) -> PyResult<Arr<'py>> {
    positive(length, "length")?;
    let x = slice(&x, "x")?;
    Ok(py
        .detach(|| k::bars_since_extreme(x, length, want_max))
        .into_pyarray(py))
}

type Arr4<'py> = (Arr<'py>, Arr<'py>, Arr<'py>, Arr<'py>);

fn four<'py>(py: Python<'py>, v: k::Four) -> Arr4<'py> {
    (
        v.0.into_pyarray(py),
        v.1.into_pyarray(py),
        v.2.into_pyarray(py),
        v.3.into_pyarray(py),
    )
}

/// Parabolic SAR recursion: `(long, short, af, reversal)`.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn fast_psar_loop<'py>(
    py: Python<'py>,
    high: PyReadonlyArray1<'py, f64>,
    low: PyReadonlyArray1<'py, f64>,
    falling: bool,
    sar: f64,
    ep: f64,
    af0: f64,
    max_af: f64,
) -> PyResult<Arr4<'py>> {
    let (h, l) = (slice(&high, "high")?, slice(&low, "low")?);
    same_len(h.len(), l, "low")?;
    let out = py.detach(|| k::psar_loop(h, l, falling, sar, ep, af0, max_af));
    Ok(four(py, out))
}

/// Supertrend band recursion: `(direction, trend, long, short)`.
#[pyfunction]
pub fn fast_supertrend_loop<'py>(
    py: Python<'py>,
    close: PyReadonlyArray1<'py, f64>,
    upper: PyReadonlyArray1<'py, f64>,
    lower: PyReadonlyArray1<'py, f64>,
) -> PyResult<Arr4<'py>> {
    let (c, u, l) = (
        slice(&close, "close")?,
        slice(&upper, "upper")?,
        slice(&lower, "lower")?,
    );
    same_len(c.len(), u, "upper")?;
    same_len(c.len(), l, "lower")?;
    let out = py.detach(|| k::supertrend_loop(c, u, l));
    Ok(four(py, out))
}

/// QQE trailing-band recursion: `(long, short, trend, qqe, qqe_long, qqe_short)`.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub fn fast_qqe_loop<'py>(
    py: Python<'py>,
    rsi_ma: PyReadonlyArray1<'py, f64>,
    upper: PyReadonlyArray1<'py, f64>,
    lower: PyReadonlyArray1<'py, f64>,
) -> PyResult<(Arr<'py>, Arr<'py>, Arr<'py>, Arr<'py>, Arr<'py>, Arr<'py>)> {
    let (r, u, l) = (
        slice(&rsi_ma, "rsi_ma")?,
        slice(&upper, "upper")?,
        slice(&lower, "lower")?,
    );
    same_len(r.len(), u, "upper")?;
    same_len(r.len(), l, "lower")?;
    let q = py.detach(|| k::qqe_loop(r, u, l));
    Ok((
        q.long.into_pyarray(py),
        q.short.into_pyarray(py),
        q.trend.into_pyarray(py),
        q.qqe.into_pyarray(py),
        q.qqe_long.into_pyarray(py),
        q.qqe_short.into_pyarray(py),
    ))
}

/// Chaikin A/D oscillator recursion over the A/D line.
#[pyfunction]
pub fn fast_adosc_loop<'py>(
    py: Python<'py>,
    ad: PyReadonlyArray1<'py, f64>,
    fast: usize,
    slow: usize,
) -> PyResult<Arr<'py>> {
    positive(fast, "fast")?;
    positive(slow, "slow")?;
    let a = slice(&ad, "ad")?;
    Ok(py.detach(|| k::adosc_loop(a, fast, slow)).into_pyarray(py))
}

/// KAMA recursion over the squared smoothing constant `sc` and `close`.
#[pyfunction]
pub fn fast_kama_loop<'py>(
    py: Python<'py>,
    sc: PyReadonlyArray1<'py, f64>,
    close: PyReadonlyArray1<'py, f64>,
    length: usize,
) -> PyResult<Arr<'py>> {
    positive(length, "length")?;
    let s = slice(&sc, "sc")?;
    let c = slice(&close, "close")?;
    same_len(c.len(), s, "sc")?;
    Ok(py.detach(|| k::kama_loop(s, c, length)).into_pyarray(py))
}

/// Fisher Transform recursion over the normalized position `pos`, from bar `length` on.
#[pyfunction]
pub fn fast_fisher_loop<'py>(
    py: Python<'py>,
    pos: PyReadonlyArray1<'py, f64>,
    length: usize,
) -> PyResult<Arr<'py>> {
    positive(length, "length")?;
    let p = slice(&pos, "pos")?;
    Ok(py.detach(|| k::fisher_loop(p, length)).into_pyarray(py))
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
    m.add_function(wrap_pyfunction!(fast_rolling_sum, m)?)?;
    m.add_function(wrap_pyfunction!(fast_rolling_max, m)?)?;
    m.add_function(wrap_pyfunction!(fast_rolling_min, m)?)?;
    m.add_function(wrap_pyfunction!(fast_wma, m)?)?;
    m.add_function(wrap_pyfunction!(fast_wilder_sum, m)?)?;
    m.add_function(wrap_pyfunction!(fast_bars_since, m)?)?;
    m.add_function(wrap_pyfunction!(fast_psar_loop, m)?)?;
    m.add_function(wrap_pyfunction!(fast_supertrend_loop, m)?)?;
    m.add_function(wrap_pyfunction!(fast_qqe_loop, m)?)?;
    m.add_function(wrap_pyfunction!(fast_adosc_loop, m)?)?;
    m.add_function(wrap_pyfunction!(fast_kama_loop, m)?)?;
    m.add_function(wrap_pyfunction!(fast_fisher_loop, m)?)?;
    Ok(())
}
