//! Conversions between Python values and the crate's numeric types.
//!
//! Prices and indicator parameters accept `str`, `int`, `float` or `decimal.Decimal`:
//! - `str` and `decimal.Decimal` are parsed exactly (`"0.1"` is exactly one tenth);
//! - `int` is exact up to 28 significant digits;
//! - `float` becomes the shortest decimal that prints as the same float, so `0.1`
//!   becomes exactly `0.1`, never `0.1000000000000000055511151231`.
//!
//! `bool` is rejected (it is an `int` subclass in Python, and `True` as a price is a bug).
//! NaN and infinities are rejected. `Decimal` keeps at most 28 decimal places: extra places
//! are rounded, but a non-zero value that would round to zero (`1e-30`, or
//! `"0.00000000000000000000000000001"`) is rejected instead of silently becoming `0`.

use std::io::Write as _;
use std::str::FromStr;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBool, PyFloat, PyInt, PyString, PyType};
use rust_decimal::Decimal;

use crate::error::FinError;

static DECIMAL_TYPE: PyOnceLock<Py<PyType>> = PyOnceLock::new();

/// The `decimal.Decimal` class, imported once per interpreter.
pub(crate) fn decimal_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    DECIMAL_TYPE.import(py, "decimal", "Decimal")
}

/// Maps a [`FinError`] to `ValueError`.
pub(crate) fn fin_err(e: FinError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Parses decimal text, including exponent forms such as `1E+2` that `str(Decimal)` can print.
pub(crate) fn parse_decimal(text: &str) -> PyResult<Decimal> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if lower.contains("nan") || lower.contains("inf") {
        return Err(PyValueError::new_err(format!(
            "{t:?} is not a finite number"
        )));
    }
    let mantissa = t.split(['e', 'E']).next().unwrap_or("");
    let nonzero = mantissa.bytes().any(|b| (b'1'..=b'9').contains(&b));
    let d = match Decimal::from_str(t).or_else(|_| Decimal::from_scientific(t)) {
        Ok(d) => d,
        // `str(Decimal("0E-30"))` is a zero with an exponent past 28 places: still zero.
        Err(_) if !nonzero && Decimal::from_str(mantissa).is_ok() => Decimal::ZERO,
        Err(e) => {
            return Err(PyValueError::new_err(format!(
                "cannot read {t:?} as a decimal: {e}"
            )))
        }
    };
    if d.is_zero() && nonzero {
        return Err(PyValueError::new_err(format!(
            "{t:?} is too small for a decimal with 28 decimal places (it would become 0)"
        )));
    }
    Ok(d)
}

/// Converts an `f64` to the shortest decimal that round-trips to the same float.
///
/// Returns `None` for NaN, infinities, values outside the `Decimal` range and non-zero
/// values too small for 28 decimal places (they would otherwise become `0`). Does not
/// touch the Python runtime, so it is safe to call with the GIL released.
pub(crate) fn f64_to_decimal_opt(value: f64) -> Option<Decimal> {
    if !value.is_finite() {
        return None;
    }
    // `Display` for f64 prints the shortest round-trip digits and never uses an exponent.
    let mut buf = [0u8; 400];
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    let mut parsed = None;
    if write!(cursor, "{value}").is_ok() {
        let n = usize::try_from(cursor.position()).unwrap_or(0);
        parsed = std::str::from_utf8(&buf[..n])
            .ok()
            .and_then(|s| Decimal::from_str(s).ok());
    }
    // `from_str` rounds digits past 28 decimal places; if it failed, fall back to
    // `from_f64`. Either way, never let a non-zero float round all the way to zero.
    use rust_decimal::prelude::FromPrimitive;
    parsed
        .or_else(|| Decimal::from_f64(value))
        .filter(|d| !d.is_zero() || value == 0.0)
}

/// [`f64_to_decimal_opt`] with a `ValueError` for non-finite or out-of-range input.
pub(crate) fn f64_to_decimal(value: f64) -> PyResult<Decimal> {
    f64_to_decimal_opt(value)
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "{value:e} is NaN, infinite or outside the Decimal range (28 decimal places, |x| < 7.9e28)"
            ))
        })
}

/// Converts a `str`, `int`, `float` or `decimal.Decimal` to a [`Decimal`].
///
/// Objects of other types that define `__float__` (numpy scalars, for example) are read
/// through `float()`.
pub(crate) fn to_decimal(obj: &Bound<'_, PyAny>) -> PyResult<Decimal> {
    if obj.is_instance_of::<PyBool>() {
        return Err(PyTypeError::new_err("expected a number, got bool"));
    }
    if obj.is_instance_of::<PyFloat>() {
        return f64_to_decimal(obj.extract::<f64>()?);
    }
    if obj.is_instance_of::<PyInt>() {
        if let Ok(v) = obj.extract::<i64>() {
            return Ok(Decimal::from(v));
        }
        return parse_decimal(&obj.str()?.to_cow()?);
    }
    if obj.is_instance_of::<PyString>() {
        return parse_decimal(&obj.cast::<PyString>()?.to_cow()?);
    }
    if obj.is_instance(decimal_type(obj.py())?)? {
        return parse_decimal(&obj.str()?.to_cow()?);
    }
    if obj.hasattr("__float__")? {
        let f: f64 = obj.call_method0("__float__")?.extract()?;
        return f64_to_decimal(f);
    }
    Err(PyTypeError::new_err(format!(
        "expected str, int, float or decimal.Decimal, got {}",
        obj.get_type().name()?
    )))
}

/// Converts a [`Decimal`] to a Python `decimal.Decimal`.
pub(crate) fn decimal_to_py<'py>(py: Python<'py>, d: Decimal) -> PyResult<Bound<'py, PyAny>> {
    decimal_type(py)?.call1((d.to_string(),))
}

/// Converts a [`Decimal`] to the nearest `f64`, the same float Python's `float(Decimal)` gives.
///
/// `Decimal::to_f64` can be one unit in the last place off; parsing the decimal text with
/// `str::parse::<f64>` is correctly rounded. Does not touch the Python runtime.
pub(crate) fn decimal_to_f64(d: Decimal) -> f64 {
    let mut buf = [0u8; 64];
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    if write!(cursor, "{d}").is_ok() {
        let n = usize::try_from(cursor.position()).unwrap_or(0);
        if let Some(f) = std::str::from_utf8(&buf[..n])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
        {
            return f;
        }
    }
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(f64::NAN)
}

/// Fails unless exactly `n` constructor arguments were given.
pub(crate) fn check_arity(args: &[Bound<'_, PyAny>], n: usize) -> PyResult<()> {
    if args.len() != n {
        return Err(PyTypeError::new_err(format!(
            "expected {n} parameter(s), got {}",
            args.len()
        )));
    }
    Ok(())
}

fn arg<'a, 'py>(args: &'a [Bound<'py, PyAny>], i: usize) -> PyResult<&'a Bound<'py, PyAny>> {
    args.get(i)
        .ok_or_else(|| PyTypeError::new_err(format!("missing parameter {i}")))
}

/// Prefixes an argument error with the parameter name, keeping the exception type.
fn named<T>(a: &Bound<'_, PyAny>, r: PyResult<T>, name: &str) -> PyResult<T> {
    r.map_err(|e| {
        let py = a.py();
        PyErr::from_type(
            e.get_type(py),
            format!("parameter `{name}`: {}", e.value(py)),
        )
    })
}

/// Integer parameter (`usize`).
pub(crate) fn arg_usize(args: &[Bound<'_, PyAny>], i: usize, name: &str) -> PyResult<usize> {
    let a = arg(args, i)?;
    if a.is_instance_of::<PyBool>()
        || !a.is_instance_of::<PyInt>() && a.hasattr("__index__").ok() != Some(true)
    {
        return Err(PyTypeError::new_err(format!(
            "parameter `{name}` must be an int"
        )));
    }
    named(a, a.extract::<usize>(), name)
}

/// Integer parameter (`u32`).
pub(crate) fn arg_u32(args: &[Bound<'_, PyAny>], i: usize, name: &str) -> PyResult<u32> {
    let v = arg_usize(args, i, name)?;
    u32::try_from(v).map_err(|_| PyValueError::new_err(format!("parameter `{name}` is too large")))
}

/// Decimal parameter: `str`, `int`, `float` or `decimal.Decimal`.
pub(crate) fn arg_decimal(args: &[Bound<'_, PyAny>], i: usize, name: &str) -> PyResult<Decimal> {
    let a = arg(args, i)?;
    named(a, to_decimal(a), name)
}

/// Float parameter.
pub(crate) fn arg_f64(args: &[Bound<'_, PyAny>], i: usize, name: &str) -> PyResult<f64> {
    let a = arg(args, i)?;
    named(a, a.extract::<f64>(), name)
}

/// Optional float parameter (`None` allowed).
pub(crate) fn arg_opt_f64(
    args: &[Bound<'_, PyAny>],
    i: usize,
    name: &str,
) -> PyResult<Option<f64>> {
    let a = arg(args, i)?;
    if a.is_none() {
        return Ok(None);
    }
    named(a, a.extract::<f64>(), name).map(Some)
}

/// Boolean parameter.
pub(crate) fn arg_bool(args: &[Bound<'_, PyAny>], i: usize, name: &str) -> PyResult<bool> {
    let a = arg(args, i)?;
    named(a, a.extract::<bool>(), name)
}

/// Text parameter. A number is accepted too and passed as its exact decimal text
/// (the one text parameter in the catalogue, `bollinger_pct_b`'s multiplier, is a decimal).
pub(crate) fn arg_string(args: &[Bound<'_, PyAny>], i: usize, name: &str) -> PyResult<String> {
    let a = arg(args, i)?;
    if a.is_instance_of::<PyString>() {
        return named(a, a.extract::<String>(), name);
    }
    named(a, to_decimal(a).map(|d| d.to_string()), name)
}

/// Four integers (a list or tuple of length 4).
pub(crate) fn arg_usize4(args: &[Bound<'_, PyAny>], i: usize, name: &str) -> PyResult<[usize; 4]> {
    let a = arg(args, i)?;
    let v: Vec<usize> = named(a, a.extract::<Vec<usize>>(), name)?;
    <[usize; 4]>::try_from(v).map_err(|v| {
        PyValueError::new_err(format!("parameter `{name}` needs 4 ints, got {}", v.len()))
    })
}
