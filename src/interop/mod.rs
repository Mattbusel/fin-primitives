//! Interop with other Rust trading crates, so you can add fin-primitives to a
//! project that already uses one of them without rewriting anything.
//!
//! Each integration is behind its own cargo feature, off by default:
//!
//! | feature | crate | what you get |
//! |---|---|---|
//! | `ta` | [`ta`](https://docs.rs/ta) | `OhlcvBar` and `BarInput` implement ta's `Open`/`High`/`Low`/`Close`/`Volume`, so ta's indicators accept fin-primitives bars; `interop::ta::bar_input` goes the other way. |
//! | `yata` | [`yata`](https://docs.rs/yata) | `OhlcvBar` and `BarInput` implement yata's `OHLCV`, so yata methods and indicators take them directly; `interop::yata::bar_input` goes the other way. |
//! | `wickra` | [`wickra-core`](https://docs.rs/wickra-core) | wickra's `Candle` from [`OhlcvBar`](crate::ohlcv::OhlcvBar) or [`BarInput`](crate::signals::BarInput) (`TryFrom`), and `Candle` back to `BarInput`. |
//!
//! Going from fin-primitives to the f64 crates converts each exact decimal to the
//! nearest `f64`. Coming back, each `f64` becomes the shortest decimal that prints
//! the same (`0.1` becomes exactly `0.1`, not `0.1000000000000000055...`), and
//! NaN or infinite values are an error rather than a silent zero.

use crate::error::FinError;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

#[cfg(feature = "ta")]
#[cfg_attr(docsrs, doc(cfg(feature = "ta")))]
pub mod ta;

#[cfg(feature = "yata")]
#[cfg_attr(docsrs, doc(cfg(feature = "yata")))]
pub mod yata;

#[cfg(feature = "wickra")]
#[cfg_attr(docsrs, doc(cfg(feature = "wickra")))]
pub mod wickra;

/// Nearest `f64` to a decimal (NaN only if the decimal cannot be represented, which
/// does not happen for `rust_decimal` values in practice).
#[inline]
#[must_use]
pub fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(f64::NAN)
}

/// The shortest decimal that prints the same as `x`.
///
/// # Errors
/// [`FinError::InvalidInput`] if `x` is NaN, infinite, or outside `Decimal`'s range.
pub fn f64_to_decimal(x: f64) -> Result<Decimal, FinError> {
    if !x.is_finite() {
        return Err(FinError::InvalidInput(format!("cannot convert {x} to a decimal")));
    }
    // `Display` for f64 prints the shortest round-trip digits without an exponent.
    Decimal::from_str_exact(&x.to_string())
        .or_else(|_| Decimal::try_from(x))
        .map_err(|e| FinError::InvalidInput(format!("cannot convert {x} to a decimal: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn f64_to_decimal_is_the_printed_value() {
        assert_eq!(f64_to_decimal(0.1).unwrap(), dec!(0.1));
        assert_eq!(f64_to_decimal(64_250.5).unwrap(), dec!(64250.5));
        assert_eq!(f64_to_decimal(-3.0).unwrap(), dec!(-3));
        assert_eq!(f64_to_decimal(1e-7).unwrap(), dec!(0.0000001));
        assert!(f64_to_decimal(f64::NAN).is_err());
        assert!(f64_to_decimal(f64::INFINITY).is_err());
        assert!(f64_to_decimal(1e300).is_err());
    }

    #[test]
    fn decimal_to_f64_round_trips_typical_prices() {
        for d in [dec!(0.01), dec!(64250.50), dec!(3190.94), dec!(0.00012345)] {
            assert_eq!(f64_to_decimal(decimal_to_f64(d)).unwrap(), d.normalize());
        }
    }
}
