//! Exact-decimal helpers shared by the indicators.
//!
//! ## Responsibility
//! A correctly rounded-to-working-precision square root for [`Decimal`], valid over the
//! whole `Decimal` range (from `1e-28` to about `7.9e28`).
//!
//! ## Guarantees
//! - `sqrt(x)` for `x > 0` is within one unit in the last representable place of the true
//!   root (Newton's method from an `f64` seed, iterated to a fixed point)
//! - Never panics; negative input gives `None`

use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;

/// Square root of a non-negative `Decimal`; `None` for a negative input.
///
/// The seed is the `f64` square root (about 16 correct digits), and Newton's iteration
/// `x = (x + n / x) / 2` then doubles the correct digits each step, so two or three steps
/// reach `Decimal`'s precision at any magnitude. The loop stops at a fixed point (or a
/// two-value cycle from the last digit's rounding), with a hard cap of 12 steps.
pub(crate) fn sqrt(n: Decimal) -> Option<Decimal> {
    if n.is_sign_negative() && !n.is_zero() {
        return None;
    }
    if n.is_zero() {
        return Some(Decimal::ZERO);
    }
    let seed = n.to_f64().map(f64::sqrt).and_then(Decimal::from_f64);
    let mut x = match seed {
        Some(s) if s > Decimal::ZERO => s,
        // Only reached if the f64 conversion fails; Newton still converges from n or 1.
        _ => n.max(Decimal::ONE),
    };
    let mut prev = Decimal::ZERO;
    for _ in 0..12 {
        let next = (x.checked_add(n.checked_div(x)?)?).checked_div(Decimal::TWO)?;
        if next == x || next == prev {
            // Fixed point, or oscillating between the two neighbours of the true root:
            // keep the smaller, whose square does not exceed n by more than rounding.
            return Some(next.min(x));
        }
        prev = x;
        x = next;
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    /// Relative error of root against n / root (no squaring, so Decimal::MAX's root works).
    fn rel_err(root: Decimal, n: Decimal) -> f64 {
        ((n / root - root) / root).abs().to_f64().unwrap()
    }

    #[test]
    fn exact_squares() {
        assert_eq!(sqrt(dec!(4)), Some(dec!(2)));
        assert_eq!(sqrt(dec!(0.0001)), Some(dec!(0.01)));
        assert_eq!(sqrt(dec!(1e20)), Some(dec!(1e10)));
        assert_eq!(sqrt(Decimal::ZERO), Some(Decimal::ZERO));
        assert_eq!(sqrt(dec!(-1)), None);
    }

    #[test]
    fn tiny_magnitudes() {
        // The old Newton loop seeded at max(n / 2, 1) and capped at 20 steps returned
        // about 9.5e-7 for sqrt(1e-16) (true 1e-8).
        for s in ["1e-16", "2.5e-17", "1e-20", "3e-24", "0.0000000000000000000000000001"] {
            let n = Decimal::from_str(s).unwrap_or_else(|_| Decimal::from_scientific(s).unwrap());
            let r = sqrt(n).unwrap();
            let want = n.to_f64().unwrap().sqrt();
            let got = r.to_f64().unwrap();
            assert!(((got - want) / want).abs() < 1e-9, "{s}: {got} vs {want}");
        }
        let r = sqrt(Decimal::from_scientific("1e-16").unwrap()).unwrap();
        assert_eq!(r, Decimal::from_scientific("1e-8").unwrap());
    }

    #[test]
    fn huge_magnitudes() {
        for n in [dec!(1e24), dec!(123456789012345678901234567), Decimal::MAX] {
            let r = sqrt(n).unwrap();
            assert!(rel_err(r, n) < 1e-26, "{n}");
        }
    }

    #[test]
    fn ordinary_values_are_full_precision() {
        let r = sqrt(dec!(2)).unwrap();
        assert_eq!(r.round_dp(20), dec!(1.41421356237309504880));
        for n in [dec!(0.5), dec!(3), dec!(101.25), dec!(98765.4321)] {
            assert!(rel_err(sqrt(n).unwrap(), n) < 1e-26, "{n}");
        }
    }
}
