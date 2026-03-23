//! Bond spread analytics: Z-spread, OAS, I-spread, asset-swap spread.
//!
//! Provides tools for computing and decomposing bond spreads relative to
//! benchmark curves (zero-rate / swap).

/// Type of spread being computed.
#[derive(Debug, Clone, PartialEq)]
pub enum SpreadType {
    /// Zero-volatility spread: constant shift to zero-rate curve that prices bond at market.
    ZSpread,
    /// Option-adjusted spread: Z-spread after stripping embedded optionality.
    OAS,
    /// Interpolated-rate spread: YTM minus interpolated swap rate at same tenor.
    ISpread,
    /// Asset-swap spread: simplified annuity-based decomposition.
    ASWSpread,
}

/// A zero-rate curve represented by (tenor, continuously-compounded rate) pairs.
#[derive(Debug, Clone)]
pub struct ZeroRateCurve {
    /// Tenors in years (must be sorted ascending).
    pub tenors: Vec<f64>,
    /// Continuously-compounded zero rates corresponding to each tenor.
    pub rates: Vec<f64>,
}

impl ZeroRateCurve {
    /// Linearly interpolate (or extrapolate flat) the zero rate at tenor `t`.
    pub fn rate_at(&self, t: f64) -> f64 {
        let n = self.tenors.len();
        if n == 0 {
            return 0.0;
        }
        if t <= self.tenors[0] {
            return self.rates[0];
        }
        if t >= self.tenors[n - 1] {
            return self.rates[n - 1];
        }
        // Binary search for surrounding interval.
        let mut lo = 0usize;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.tenors[mid] <= t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let t0 = self.tenors[lo];
        let t1 = self.tenors[hi];
        let r0 = self.rates[lo];
        let r1 = self.rates[hi];
        r0 + (r1 - r0) * (t - t0) / (t1 - t0)
    }

    /// Discount factor at tenor `t`: exp(-r(t) * t).
    pub fn discount_factor(&self, t: f64) -> f64 {
        let r = self.rate_at(t);
        (-r * t).exp()
    }

    /// Instantaneous forward rate between `t1` and `t2` derived from discount factors.
    pub fn forward_rate(&self, t1: f64, t2: f64) -> f64 {
        if (t2 - t1).abs() < 1e-12 {
            return self.rate_at(t1);
        }
        let df1 = self.discount_factor(t1);
        let df2 = self.discount_factor(t2);
        if df2 <= 0.0 || df1 <= 0.0 {
            return 0.0;
        }
        (df1 / df2).ln() / (t2 - t1)
    }
}

/// A par-swap-rate curve represented by (tenor, par swap rate) pairs.
#[derive(Debug, Clone)]
pub struct SwapCurve {
    /// Tenors in years (must be sorted ascending).
    pub tenors: Vec<f64>,
    /// Par swap rates at each tenor.
    pub swap_rates: Vec<f64>,
}

impl SwapCurve {
    /// Linearly interpolate the swap rate at `tenor_years`.
    pub fn rate_at(&self, tenor_years: f64) -> f64 {
        let n = self.tenors.len();
        if n == 0 {
            return 0.0;
        }
        if tenor_years <= self.tenors[0] {
            return self.swap_rates[0];
        }
        if tenor_years >= self.tenors[n - 1] {
            return self.swap_rates[n - 1];
        }
        let mut lo = 0usize;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.tenors[mid] <= tenor_years {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let t0 = self.tenors[lo];
        let t1 = self.tenors[hi];
        let r0 = self.swap_rates[lo];
        let r1 = self.swap_rates[hi];
        r0 + (r1 - r0) * (tenor_years - t0) / (t1 - t0)
    }
}

/// A single bond cash flow.
#[derive(Debug, Clone)]
pub struct CashFlow {
    /// Time of the cash flow in years.
    pub time: f64,
    /// Cash flow amount (coupon or principal).
    pub amount: f64,
}

/// Generate cash flows for a standard fixed-rate bond.
///
/// # Arguments
/// * `face` — face/par value
/// * `coupon_rate` — annual coupon rate (e.g. 0.05 for 5%)
/// * `tenor_years` — maturity in years
/// * `frequency` — coupon payments per year (1=annual, 2=semi-annual, etc.)
pub fn bond_cash_flows(
    face: f64,
    coupon_rate: f64,
    tenor_years: f64,
    frequency: u32,
) -> Vec<CashFlow> {
    let freq = frequency.max(1) as f64;
    let period = 1.0 / freq;
    let coupon = face * coupon_rate / freq;
    let num_periods = (tenor_years * freq).round() as usize;
    let mut flows = Vec::with_capacity(num_periods);
    for i in 1..=num_periods {
        let t = i as f64 * period;
        let amount = if i == num_periods {
            coupon + face
        } else {
            coupon
        };
        flows.push(CashFlow { time: t, amount });
    }
    flows
}

/// Present value of cash flows discounted by `curve` plus a constant `spread_bps` in basis points.
pub fn pv_cash_flows(flows: &[CashFlow], curve: &ZeroRateCurve, spread_bps: f64) -> f64 {
    let spread = spread_bps / 10_000.0;
    flows.iter().map(|cf| {
        let r = curve.rate_at(cf.time) + spread;
        let df = (-r * cf.time).exp();
        cf.amount * df
    }).sum()
}

/// Compute the Z-spread (in bps) via bisection.
///
/// Returns the spread in basis points such that `pv_cash_flows(flows, curve, spread) == market_price`.
pub fn z_spread(flows: &[CashFlow], curve: &ZeroRateCurve, market_price: f64) -> f64 {
    let f = |s: f64| pv_cash_flows(flows, curve, s) - market_price;
    let mut lo = -500.0_f64;
    let mut hi = 2000.0_f64;
    // Ensure bracket.
    let f_lo = f(lo);
    let f_hi = f(hi);
    if f_lo * f_hi > 0.0 {
        // Market price outside bracket — return boundary closest to zero.
        return if f_lo.abs() < f_hi.abs() { lo } else { hi };
    }
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if (hi - lo).abs() < 1e-8 {
            return mid;
        }
        if f(lo) * f(mid) <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (lo + hi) / 2.0
}

/// Compute the I-spread (in bps): YTM minus interpolated swap rate at the same tenor.
///
/// Returns the difference in basis points.
pub fn i_spread(ytm: f64, swap_curve: &SwapCurve, tenor_years: f64) -> f64 {
    let swap_rate = swap_curve.rate_at(tenor_years);
    (ytm - swap_rate) * 10_000.0
}

/// Simplified asset-swap spread (in bps).
///
/// ASW ≈ (face - PV_flows_at_par_curve) / annuity_PV
pub fn asset_swap_spread(
    flows: &[CashFlow],
    curve: &ZeroRateCurve,
    _market_price: f64,
    face: f64,
) -> f64 {
    // PV of all cash flows at zero spread.
    let pv_flows = pv_cash_flows(flows, curve, 0.0);
    // Annuity PV: sum of discount factors at each coupon time (assume unit coupon).
    let annuity_pv: f64 = flows.iter().map(|cf| curve.discount_factor(cf.time)).sum();
    if annuity_pv.abs() < 1e-12 {
        return 0.0;
    }
    let asw = (face - pv_flows) / annuity_pv;
    asw * 10_000.0
}

/// Summary result for a single spread type.
#[derive(Debug, Clone)]
pub struct SpreadResult {
    /// Which spread type was computed.
    pub spread_type: SpreadType,
    /// The spread value in basis points.
    pub spread_bps: f64,
    /// Modified duration at the computed spread.
    pub duration: f64,
    /// Convexity at the computed spread.
    pub convexity: f64,
}

/// Compute modified duration and convexity of cash flows discounted at `curve + spread_bps`.
fn duration_convexity(flows: &[CashFlow], curve: &ZeroRateCurve, spread_bps: f64) -> (f64, f64) {
    let spread = spread_bps / 10_000.0;
    let pv: f64 = flows.iter().map(|cf| {
        let r = curve.rate_at(cf.time) + spread;
        cf.amount * (-r * cf.time).exp()
    }).sum();
    if pv.abs() < 1e-12 {
        return (0.0, 0.0);
    }
    let dur: f64 = flows.iter().map(|cf| {
        let r = curve.rate_at(cf.time) + spread;
        cf.time * cf.amount * (-r * cf.time).exp()
    }).sum::<f64>() / pv;
    let conv: f64 = flows.iter().map(|cf| {
        let r = curve.rate_at(cf.time) + spread;
        cf.time * cf.time * cf.amount * (-r * cf.time).exp()
    }).sum::<f64>() / pv;
    (dur, conv)
}

/// Analyse all spread types and return a vector of `SpreadResult`.
pub fn analyze_spreads(
    flows: &[CashFlow],
    curve: &ZeroRateCurve,
    swap_curve: &SwapCurve,
    market_price: f64,
    face: f64,
    ytm: f64,
    tenor: f64,
) -> Vec<SpreadResult> {
    let zs = z_spread(flows, curve, market_price);
    let is = i_spread(ytm, swap_curve, tenor);
    let asw = asset_swap_spread(flows, curve, market_price, face);

    // OAS: simplified — treat same as Z-spread (no embedded option in vanilla bond).
    let oas = zs;

    let (dur_z, conv_z) = duration_convexity(flows, curve, zs);
    let (dur_i, conv_i) = duration_convexity(flows, curve, is);
    let (dur_asw, conv_asw) = duration_convexity(flows, curve, asw);

    vec![
        SpreadResult { spread_type: SpreadType::ZSpread, spread_bps: zs, duration: dur_z, convexity: conv_z },
        SpreadResult { spread_type: SpreadType::OAS,     spread_bps: oas, duration: dur_z, convexity: conv_z },
        SpreadResult { spread_type: SpreadType::ISpread,  spread_bps: is,  duration: dur_i, convexity: conv_i },
        SpreadResult { spread_type: SpreadType::ASWSpread,spread_bps: asw, duration: dur_asw, convexity: conv_asw },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_curve(rate: f64) -> ZeroRateCurve {
        ZeroRateCurve {
            tenors: vec![0.0, 30.0],
            rates: vec![rate, rate],
        }
    }

    fn flat_swap(rate: f64) -> SwapCurve {
        SwapCurve {
            tenors: vec![0.0, 30.0],
            swap_rates: vec![rate, rate],
        }
    }

    #[test]
    fn test_cash_flows_count() {
        // 5-year, semi-annual => 10 cash flows.
        let flows = bond_cash_flows(1000.0, 0.05, 5.0, 2);
        assert_eq!(flows.len(), 10);
        // Last cash flow includes principal.
        let last = flows.last().unwrap();
        assert!((last.amount - (25.0 + 1000.0)).abs() < 1e-9);
    }

    #[test]
    fn test_z_spread_par_bond_is_zero() {
        // Par bond: coupon rate == discount rate => Z-spread ~ 0 bps.
        let rate = 0.05;
        let curve = flat_curve(rate);
        let flows = bond_cash_flows(1000.0, rate, 5.0, 2);
        let par_price = pv_cash_flows(&flows, &curve, 0.0);
        let zs = z_spread(&flows, &curve, par_price);
        assert!(zs.abs() < 0.5, "Z-spread of par bond should be ~0, got {zs}");
    }

    #[test]
    fn test_z_spread_discount_bond_positive() {
        // Coupon < discount rate => bond trades at discount => Z-spread > 0 vs curve at lower rate.
        let curve = flat_curve(0.03);
        let flows = bond_cash_flows(1000.0, 0.05, 5.0, 2);
        // Market price below par (simulate discount by adding 100 bps to pricing).
        let market_price = pv_cash_flows(&flows, &curve, 100.0);
        let zs = z_spread(&flows, &curve, market_price);
        assert!(zs > 50.0, "Z-spread should be > 50 bps for discount bond, got {zs}");
    }

    #[test]
    fn test_i_spread_formula() {
        let swap = flat_swap(0.03);
        // YTM 5%, swap rate 3% at any tenor => I-spread = 200 bps.
        let is = i_spread(0.05, &swap, 5.0);
        assert!((is - 200.0).abs() < 1e-6, "Expected 200 bps, got {is}");
    }

    #[test]
    fn test_pv_with_positive_spread_less_than_without() {
        let curve = flat_curve(0.05);
        let flows = bond_cash_flows(1000.0, 0.05, 5.0, 2);
        let pv0 = pv_cash_flows(&flows, &curve, 0.0);
        let pv_pos = pv_cash_flows(&flows, &curve, 50.0);
        assert!(pv_pos < pv0, "Positive spread should reduce PV");
    }

    #[test]
    fn test_discount_factor_at_zero() {
        let curve = flat_curve(0.05);
        assert!((curve.discount_factor(0.0) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_forward_rate_flat_curve() {
        let curve = flat_curve(0.05);
        let fwd = curve.forward_rate(1.0, 2.0);
        assert!((fwd - 0.05).abs() < 1e-6);
    }
}
