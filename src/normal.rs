//! The standard normal distribution: density, cumulative probability and its inverse.
//!
//! Every option pricer, Greek and parametric VaR in this crate goes through these
//! three functions. The CDF is `erfc` from [`libm`] (a port of the musl C library, about
//! 1e-15 relative error, also deep in the tails); the inverse is Wichura's algorithm
//! AS 241 (`PPND16`, about 1e-16 relative error, no extra dependency; it replaced
//! `statrs::function::erf::erfc_inv` in 3.0). Before 2.15 the crate carried nine copies of the Abramowitz and Stegun
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
/// Wichura's algorithm AS 241 (`PPND16`, Applied Statistics 37, 1988): three rational
/// approximations of degree 7, one for the centre (`|p - 0.5| <= 0.425`) and two for
/// the tails, accurate to about 1 part in 1e16 down to the smallest positive `f64`.
///
/// Returns negative infinity for `p == 0`, positive infinity for `p == 1`, and NaN
/// for `p` outside `[0, 1]` or NaN.
#[must_use]
#[allow(clippy::excessive_precision)]
pub fn inv_cdf(p: f64) -> f64 {
    if !(0.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    if p == 0.0 {
        return f64::NEG_INFINITY;
    }
    if p == 1.0 {
        return f64::INFINITY;
    }

    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = 0.180_625 - q * q;
        let num = (((((((2.509_080_928_730_122_672_7e3 * r + 3.343_057_558_358_812_810_5e4)
            * r
            + 6.726_577_092_700_870_085_3e4)
            * r
            + 4.592_195_393_154_987_145_7e4)
            * r
            + 1.373_169_376_550_946_112_5e4)
            * r
            + 1.971_590_950_306_551_442_7e3)
            * r
            + 1.331_416_678_917_843_774_5e2)
            * r
            + 3.387_132_872_796_366_608_0)
            * q;
        let den = ((((((5.226_495_278_852_854_561_0e3 * r + 2.872_908_573_572_194_267_4e4) * r
            + 3.930_789_580_009_271_061_0e4)
            * r
            + 2.121_379_430_158_659_586_7e4)
            * r
            + 5.394_196_021_424_751_107_7e3)
            * r
            + 6.871_870_074_920_579_083_0e2)
            * r
            + 4.231_333_070_160_091_125_2e1)
            * r
            + 1.0;
        return num / den;
    }

    // Tail: work with the smaller of p and 1 - p (exact for p >= 0.5 by Sterbenz).
    let tail = if q < 0.0 { p } else { 1.0 - p };
    let r = (-tail.ln()).sqrt();
    let z = if r <= 5.0 {
        let r = r - 1.6;
        let num = ((((((7.745_450_142_783_414_076_4e-4 * r + 2.272_384_498_926_918_458_3e-2)
            * r
            + 2.417_807_251_774_506_117_7e-1)
            * r
            + 1.270_458_252_452_368_382_58)
            * r
            + 3.647_848_324_763_204_605_04)
            * r
            + 5.769_497_221_460_691_405_5)
            * r
            + 4.630_337_846_156_545_295_9)
            * r
            + 1.423_437_110_749_683_577_34;
        let den = ((((((1.050_750_071_644_416_843_24e-9 * r + 5.475_938_084_995_344_946e-4)
            * r
            + 1.519_866_656_361_645_719_66e-2)
            * r
            + 1.481_039_764_274_800_745_9e-1)
            * r
            + 6.897_673_349_851_000_045_5e-1)
            * r
            + 1.676_384_830_183_803_849_4)
            * r
            + 2.053_191_626_637_758_821_87)
            * r
            + 1.0;
        num / den
    } else {
        let r = r - 5.0;
        let num = ((((((2.010_334_399_292_288_132_65e-7 * r + 2.711_555_568_743_487_578_15e-5)
            * r
            + 1.242_660_947_388_078_438_6e-3)
            * r
            + 2.653_218_952_657_612_309_3e-2)
            * r
            + 2.965_605_718_285_048_912_3e-1)
            * r
            + 1.784_826_539_917_291_335_8)
            * r
            + 5.463_784_911_164_114_369_9)
            * r
            + 6.657_904_643_501_103_777_2;
        let den = ((((((2.044_263_103_389_939_785_64e-15 * r + 1.421_511_758_316_445_888_7e-7)
            * r
            + 1.846_318_317_510_054_681_8e-5)
            * r
            + 7.868_691_311_456_132_591e-4)
            * r
            + 1.487_536_129_085_061_485_25e-2)
            * r
            + 1.369_298_809_227_358_053_1e-1)
            * r
            + 5.998_322_065_558_879_376_9e-1)
            * r
            + 1.0;
        num / den
    };
    if q < 0.0 {
        -z
    } else {
        z
    }
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
            assert!(
                ((got - want) / want).abs() < 1e-12,
                "cdf({x}) = {got}, want {want}"
            );
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
            assert!(
                (got - want).abs() < 1e-12,
                "inv_cdf({p}) = {got}, want {want}"
            );
        }
    }

    #[test]
    fn inv_cdf_round_trips() {
        for i in 1..1000 {
            let p = f64::from(i) / 1000.0;
            assert!((cdf(inv_cdf(p)) - p).abs() < 1e-14, "p = {p}");
        }
    }

    // Reference quantiles from mpmath at 30 digits, deep in the lower tail where the
    // second and third AS 241 branches apply.
    #[test]
    #[allow(clippy::excessive_precision)]
    fn inv_cdf_tail_reference() {
        let cases = [
            (1e-5, -4.264_890_793_922_824_6),
            (1e-10, -6.361_340_902_404_056_2),
            (1e-20, -9.262_340_089_798_407_6),
            (1e-50, -14.933_337_534_788_489),
            (1e-300, -37.047_096_299_361_199),
        ];
        for (p, want) in cases {
            let got = inv_cdf(p);
            assert!(
                ((got - want) / want).abs() < 1e-14,
                "inv_cdf({p}) = {got}, want {want}"
            );
            let upper = inv_cdf(1.0 - p);
            if p >= 1e-10 {
                // 1 - p is exact enough here to check the upper-tail mirror.
                assert!((upper + want).abs() < 1e-5, "inv_cdf(1 - {p}) = {upper}");
            }
        }
    }

    #[test]
    fn inv_cdf_inverts_cdf_on_a_grid() {
        // x <= 3 keeps cdf(x) well conditioned: above that, cdf(x) rounds to within
        // one ulp of 1 and the information about x is lost before inv_cdf sees it.
        for i in -3700..=300 {
            let x = f64::from(i) / 100.0;
            let back = inv_cdf(cdf(x));
            assert!(
                (back - x).abs() < 1e-12,
                "x = {x}: inv_cdf(cdf(x)) = {back}"
            );
        }
    }

    #[test]
    fn inv_cdf_is_odd() {
        for i in 1..500 {
            let p = f64::from(i) / 1000.0;
            assert!((inv_cdf(p) + inv_cdf(1.0 - p)).abs() < 1e-13, "p = {p}");
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
