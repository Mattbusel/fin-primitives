//! Bond pricing, duration, convexity, and yield-to-maturity solver.
//!
//! All functions are pure and allocation-free on the hot path.
//! YTM is solved via Brent's method to a tolerance of 1e-8.
//!
//! # Example
//! ```rust
//! use fin_primitives::fixed_income::bond::{Bond, CouponFrequency, price, yield_to_maturity};
//!
//! let bond = Bond {
//!     face_value: 1000.0,
//!     coupon_rate: 0.05,
//!     maturity_years: 10.0,
//!     frequency: CouponFrequency::SemiAnnual,
//!     settlement_date: 0,
//! };
//! let p = price(0.05, &bond);
//! assert!((p - 1000.0).abs() < 1e-6, "par bond price should equal face value");
//! ```

// ─────────────────────────────────────────────────────────────────────────────
//  CouponFrequency
// ─────────────────────────────────────────────────────────────────────────────

/// How many times per year the bond pays a coupon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CouponFrequency {
    /// One coupon payment per year.
    Annual,
    /// Two coupon payments per year.
    SemiAnnual,
    /// Four coupon payments per year.
    Quarterly,
    /// Twelve coupon payments per year.
    Monthly,
}

impl CouponFrequency {
    /// Returns the number of coupon periods per calendar year.
    pub fn periods_per_year(&self) -> u32 {
        match self {
            CouponFrequency::Annual => 1,
            CouponFrequency::SemiAnnual => 2,
            CouponFrequency::Quarterly => 4,
            CouponFrequency::Monthly => 12,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Bond
// ─────────────────────────────────────────────────────────────────────────────

/// Describes a fixed-rate coupon bond.
#[derive(Debug, Clone)]
pub struct Bond {
    /// Face (par) value of the bond.
    pub face_value: f64,
    /// Annual coupon rate as a decimal (e.g. `0.05` = 5 %).
    pub coupon_rate: f64,
    /// Time to maturity in years from the settlement date.
    pub maturity_years: f64,
    /// Coupon payment frequency.
    pub frequency: CouponFrequency,
    /// Settlement date as Unix seconds (informational; not used in pure math).
    pub settlement_date: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Core helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Returns the periodic coupon payment amount.
#[inline]
fn periodic_coupon(bond: &Bond) -> f64 {
    let m = f64::from(bond.frequency.periods_per_year());
    bond.face_value * bond.coupon_rate / m
}

/// Returns the total number of coupon periods (rounded to nearest integer).
#[inline]
fn num_periods(bond: &Bond) -> u64 {
    let m = f64::from(bond.frequency.periods_per_year());
    (bond.maturity_years * m).round() as u64
}

/// Returns the per-period discount rate: `ytm / periods_per_year`.
#[inline]
fn period_rate(ytm: f64, bond: &Bond) -> f64 {
    ytm / f64::from(bond.frequency.periods_per_year())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Public functions
// ─────────────────────────────────────────────────────────────────────────────

/// Calculates the full (clean) price of a bond given a yield-to-maturity.
///
/// Uses the standard present-value formula:
/// ```text
/// price = Σ_{t=1}^{n} C / (1+y/m)^t  +  F / (1+y/m)^n
/// ```
/// where `C` = periodic coupon, `y` = ytm, `m` = periods per year, `F` = face value.
pub fn price(ytm: f64, bond: &Bond) -> f64 {
    let r = period_rate(ytm, bond);
    let c = periodic_coupon(bond);
    let n = num_periods(bond);

    if r.abs() < 1e-12 {
        // Degenerate: zero yield — all cash flows undiscounted
        return c * n as f64 + bond.face_value;
    }

    let discount_n = (1.0 + r).powi(-(n as i32));
    // Annuity PV: C * (1 - (1+r)^-n) / r
    let coupon_pv = c * (1.0 - discount_n) / r;
    let face_pv = bond.face_value * discount_n;
    coupon_pv + face_pv
}

/// Solves for the yield-to-maturity given a market price, using Brent's method.
///
/// Brackets the root in `[0.001, 0.999]` and converges to tolerance `1e-8`.
/// Returns the YTM as a decimal (e.g. `0.05` = 5 %).
///
/// Returns `f64::NAN` if the root is not bracketed (price is outside the bond's
/// attainable range for the search interval).
pub fn yield_to_maturity(market_price: f64, bond: &Bond) -> f64 {
    let tol = 1e-8;
    let max_iter = 200;

    let f = |ytm: f64| price(ytm, bond) - market_price;

    let mut a = 0.001_f64;
    let mut b = 0.999_f64;
    let mut fa = f(a);
    let mut fb = f(b);

    if fa * fb > 0.0 {
        // Try a wider search
        a = 1e-6;
        b = 50.0;
        fa = f(a);
        fb = f(b);
        if fa * fb > 0.0 {
            return f64::NAN;
        }
    }

    // Brent's method
    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;

    for _ in 0..max_iter {
        if fb * fc > 0.0 {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * tol;
        let xm = 0.5 * (c - b);
        if xm.abs() <= tol1 || fb.abs() < f64::EPSILON {
            return b;
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (p, q) = if (a - c).abs() < f64::EPSILON {
                (2.0 * xm * s, 1.0 - s)
            } else {
                let q_val = fa / fc;
                let r_val = fb / fc;
                (
                    s * (2.0 * xm * q_val * (q_val - r_val) - (b - a) * (r_val - 1.0)),
                    (q_val - 1.0) * (r_val - 1.0) * (s - 1.0),
                )
            };
            let (p, q) = if p > 0.0 { (p, -q) } else { (-p, q) };
            if 2.0 * p < (3.0 * xm * q - (tol1 * q).abs()) && 2.0 * p < (e * q).abs() {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        b += if d.abs() > tol1 { d } else { tol1.copysign(xm) };
        fb = f(b);
    }
    b
}

/// Computes the Macaulay duration: weighted average time to cash flows (in years).
///
/// ```text
/// D_mac = Σ_{t=1}^{n} (t/m) * PV(CF_t) / price
/// ```
pub fn macaulay_duration(ytm: f64, bond: &Bond) -> f64 {
    let r = period_rate(ytm, bond);
    let c = periodic_coupon(bond);
    let n = num_periods(bond);
    let m = f64::from(bond.frequency.periods_per_year());
    let p = price(ytm, bond);

    if p.abs() < f64::EPSILON {
        return 0.0;
    }

    let mut weighted_time = 0.0;
    for t in 1..=n {
        let cf = if t == n { c + bond.face_value } else { c };
        let pv = cf / (1.0 + r).powi(t as i32);
        let time_years = t as f64 / m;
        weighted_time += time_years * pv;
    }
    weighted_time / p
}

/// Computes the modified duration.
///
/// ```text
/// D_mod = D_mac / (1 + ytm/m)
/// ```
pub fn modified_duration(ytm: f64, bond: &Bond) -> f64 {
    let m = f64::from(bond.frequency.periods_per_year());
    macaulay_duration(ytm, bond) / (1.0 + ytm / m)
}

/// Computes dollar convexity (annualised).
///
/// ```text
/// Convexity = Σ_{t=1}^{n} t*(t+1)*PV(CF_t) / (price * (1+y/m)^2 * m^2)
/// ```
pub fn convexity(ytm: f64, bond: &Bond) -> f64 {
    let r = period_rate(ytm, bond);
    let c = periodic_coupon(bond);
    let n = num_periods(bond);
    let m = f64::from(bond.frequency.periods_per_year());
    let p = price(ytm, bond);

    if p.abs() < f64::EPSILON {
        return 0.0;
    }

    let mut sum = 0.0;
    for t in 1..=n {
        let cf = if t == n { c + bond.face_value } else { c };
        let pv = cf / (1.0 + r).powi(t as i32);
        sum += t as f64 * (t as f64 + 1.0) * pv;
    }
    sum / (p * (1.0 + r).powi(2) * m * m)
}

/// Computes DV01: the price change for a 1 basis-point (0.01 %) decrease in yield.
///
/// ```text
/// DV01 = price(ytm - 0.0001) - price(ytm)
/// ```
pub fn dv01(ytm: f64, bond: &Bond) -> f64 {
    price(ytm - 0.0001, bond) - price(ytm, bond)
}

/// Approximates price change using duration and convexity.
///
/// ```text
/// ΔP ≈ -D_mod * Δy + 0.5 * Convexity * Δy²
/// ```
/// where `Δy = delta_ytm`.
pub fn price_change_approximation(ytm: f64, delta_ytm: f64, bond: &Bond) -> f64 {
    let d_mod = modified_duration(ytm, bond);
    let conv = convexity(ytm, bond);
    -d_mod * delta_ytm + 0.5 * conv * delta_ytm * delta_ytm
}

/// Computes the current yield: annual coupon / market price.
pub fn current_yield(market_price: f64, bond: &Bond) -> f64 {
    let annual_coupon = bond.face_value * bond.coupon_rate;
    if market_price.abs() < f64::EPSILON {
        return 0.0;
    }
    annual_coupon / market_price
}

/// Prices a zero-coupon bond with face value 1 at the given YTM and maturity.
///
/// ```text
/// P = 1 / (1 + ytm)^maturity_years
/// ```
pub fn zero_coupon_bond_price(ytm: f64, maturity_years: f64) -> f64 {
    (1.0 + ytm).powf(-maturity_years)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bond(coupon_rate: f64, maturity_years: f64, freq: CouponFrequency) -> Bond {
        Bond {
            face_value: 1000.0,
            coupon_rate,
            maturity_years,
            frequency: freq,
            settlement_date: 0,
        }
    }

    #[test]
    fn zero_coupon_bond_price_correct() {
        // 5 % ytm, 10-year zero
        let p = zero_coupon_bond_price(0.05, 10.0);
        let expected = 1.0 / 1.05_f64.powi(10);
        assert!((p - expected).abs() < 1e-12);
    }

    #[test]
    fn par_bond_price_equals_face_value() {
        // When coupon rate == YTM the bond prices at par
        let bond = make_bond(0.06, 10.0, CouponFrequency::SemiAnnual);
        let p = price(0.06, &bond);
        assert!((p - 1000.0).abs() < 1e-6, "par price = {p}");
    }

    #[test]
    fn par_bond_annual_coupon() {
        let bond = make_bond(0.05, 5.0, CouponFrequency::Annual);
        let p = price(0.05, &bond);
        assert!((p - 1000.0).abs() < 1e-6, "par price = {p}");
    }

    #[test]
    fn ytm_roundtrip() {
        let bond = make_bond(0.04, 7.0, CouponFrequency::SemiAnnual);
        let ytm_original = 0.06;
        let p = price(ytm_original, &bond);
        let ytm_solved = yield_to_maturity(p, &bond);
        assert!(
            (ytm_solved - ytm_original).abs() < 1e-7,
            "YTM roundtrip: got {ytm_solved}, expected {ytm_original}"
        );
    }

    #[test]
    fn ytm_roundtrip_premium_bond() {
        let bond = make_bond(0.08, 5.0, CouponFrequency::Annual);
        let ytm_original = 0.05;
        let p = price(ytm_original, &bond);
        let ytm_solved = yield_to_maturity(p, &bond);
        assert!(
            (ytm_solved - ytm_original).abs() < 1e-7,
            "YTM roundtrip premium: got {ytm_solved}"
        );
    }

    #[test]
    fn modified_duration_positive() {
        let bond = make_bond(0.05, 10.0, CouponFrequency::SemiAnnual);
        let d = modified_duration(0.05, &bond);
        assert!(d > 0.0, "modified duration must be positive");
    }

    #[test]
    fn convexity_positive() {
        let bond = make_bond(0.05, 10.0, CouponFrequency::SemiAnnual);
        let c = convexity(0.05, &bond);
        assert!(c > 0.0, "convexity must be positive");
    }

    #[test]
    fn dv01_positive() {
        // Price rises when yield falls, so DV01 > 0
        let bond = make_bond(0.05, 10.0, CouponFrequency::SemiAnnual);
        let d = dv01(0.05, &bond);
        assert!(d > 0.0, "DV01 must be positive, got {d}");
    }

    #[test]
    fn macaulay_duration_less_than_maturity() {
        let bond = make_bond(0.05, 10.0, CouponFrequency::Annual);
        let d_mac = macaulay_duration(0.05, &bond);
        assert!(
            d_mac > 0.0 && d_mac <= bond.maturity_years,
            "Macaulay duration {d_mac} must be in (0, maturity]"
        );
    }

    #[test]
    fn price_change_approximation_sign() {
        let bond = make_bond(0.05, 10.0, CouponFrequency::SemiAnnual);
        // Rising yield → negative price change
        let dp = price_change_approximation(0.05, 0.01, &bond);
        assert!(dp < 0.0, "price must fall when yield rises, got {dp}");
    }

    #[test]
    fn current_yield_par_bond() {
        let bond = make_bond(0.06, 10.0, CouponFrequency::Annual);
        let cy = current_yield(1000.0, &bond);
        // annual coupon = 60.0, price = 1000.0 → cy = 0.06
        assert!((cy - 0.06).abs() < 1e-12);
    }

    #[test]
    fn quarterly_par_bond() {
        let bond = make_bond(0.08, 3.0, CouponFrequency::Quarterly);
        let p = price(0.08, &bond);
        assert!((p - 1000.0).abs() < 1e-5, "quarterly par price = {p}");
    }

    #[test]
    fn monthly_par_bond() {
        let bond = make_bond(0.06, 2.0, CouponFrequency::Monthly);
        let p = price(0.06, &bond);
        assert!((p - 1000.0).abs() < 1e-4, "monthly par price = {p}");
    }
}
