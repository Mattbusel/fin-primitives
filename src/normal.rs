//! The standard normal distribution: density, cumulative probability and its inverse.
//!
//! Every option pricer, Greek and parametric VaR in this crate goes through these
//! three functions. The CDF is `erfc` from [`libm`] (a port of the musl C library, about
//! 1e-15 relative error, also deep in the tails); the inverse is `erfc_inv` from
//! [`statrs`]. Before 2.15 the crate carried nine copies of the Abramowitz and Stegun
//! polynomial (relative error 0.16% at `x = -5`, 7% at `x = -8`, and two copies
//! returned exactly 0 below `x = -8`), a probit with an error of about 4e-4, and two
//! VaR paths that looked up z from a three-row table (so 97.5% used 1.645, not 1.960).
//!
//! ```
//! use fin_primitives::normal;
//! assert!((normal::cdf(1.959_963_984_540_054) - 0.975).abs() < 1e-15);
//! assert!((normal::inv_cdf(0.99) - 2.326_347_874_040_841).abs() < 1e-12);
//! // Deep tail: still right to 12 significant digits.
//! assert!((normal::cdf(-7.0) / 1.279_812_543_885_835e-12 - 1.0).abs() < 1e-12);
//! ```


/// `1 / sqrt(2 * pi)`.
const FRAC_1_SQRT_2PI: f64 = 0.398_942_280_401_432_7;

/// Standard normal probability density `phi(x) = exp(-x^2 / 2) / sqrt(2 pi)`.
#[inline]
#[must_use]
pub fn pdf(x: f64) -> f64 {
    FRAC_1_SQRT_2PI * (-0.5 * x * x).exp()
}

/// Standard normal cumulative probability `P(Z <= x)`.
///
/// Computed as `erfc(-x / sqrt 2) / 2`, so `cdf(x)` keeps full relative precision for
/// large negative `x` and `cdf(x) + cdf(-x) == 1` to within one ulp.
/// Returns NaN for NaN input.
#[inline]
#[must_use]
pub fn cdf(x: f64) -> f64 {
    0.5 * libm::erfc(-x / std::f64::consts::SQRT_2)
}

/// Inverse of [`cdf`] (the probit or quantile function): the `z` with `cdf(z) == p`.
///
/// Returns negative infinity for `p == 0`, positive infinity for `p == 1`, and NaN
/// for `p` outside `[0, 1]` or NaN.
#[inline]
#[must_use]
pub fn inv_cdf(p: f64) -> f64 {
    if !(0.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    -std::f64::consts::SQRT_2 * statrs::function::erf::erfc_inv(2.0 * p)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values from mpmath at 30 digits.
    const CDF_REF: [(f64, f64); 7] = [
        (0.0, 0.5),
        (1.0, 0.841_344_746_068_542_9),
        (-1.0, 0.158_655_253_931_457_05),
        (-3.0, 1.349_898_031_630_094_6e-3),
        (-6.0, 9.865_876_450_376_982e-10),
        (-8.0, 6.220_960_574_271_785e-16),
        (-10.0, 7.619_853_024_160_527e-24),
    ];

    #[test]
    fn cdf_matches_reference_to_relative_1e12() {
        for (x, want) in CDF_REF {
            let got = cdf(x);
            assert!(((got - want) / want).abs() < 1e-12, "cdf({x}) = {got}, want {want}");
        }
    }

    #[test]
    fn cdf_symmetry() {
        for i in -80..=80 {
            let x = f64::from(i) / 10.0;
            assert!((cdf(x) + cdf(-x) - 1.0).abs() < 1e-15, "x = {x}");
        }
    }

    #[test]
    fn inv_cdf_known_quantiles() {
        let cases = [
            (0.5, 0.0),
            (0.9, 1.281_551_565_544_600_5),
            (0.95, 1.644_853_626_951_472_7),
            (0.975, 1.959_963_984_540_054),
            (0.99, 2.326_347_874_040_841),
            (0.999, 3.090_232_306_167_813_5),
            (0.05, -1.644_853_626_951_472_7),
        ];
        for (p, want) in cases {
            let got = inv_cdf(p);
            assert!((got - want).abs() < 1e-12, "inv_cdf({p}) = {got}, want {want}");
        }
    }

    #[test]
    fn inv_cdf_round_trips() {
        for i in 1..1000 {
            let p = f64::from(i) / 1000.0;
            assert!((cdf(inv_cdf(p)) - p).abs() < 1e-14, "p = {p}");
        }
    }

    #[test]
    fn inv_cdf_edges() {
        assert_eq!(inv_cdf(0.0), f64::NEG_INFINITY);
        assert_eq!(inv_cdf(1.0), f64::INFINITY);
        assert!(inv_cdf(-0.1).is_nan());
        assert!(inv_cdf(1.1).is_nan());
        assert!(inv_cdf(f64::NAN).is_nan());
    }

    #[test]
    fn pdf_peak() {
        assert!((pdf(0.0) - FRAC_1_SQRT_2PI).abs() < 1e-17);
        assert!((pdf(1.0) - 0.241_970_724_519_143_35).abs() < 1e-16);
    }
}
