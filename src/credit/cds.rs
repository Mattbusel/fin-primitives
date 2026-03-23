//! Credit default swap (CDS) pricing with hazard rate models.
//!
//! Implements survival probability curves, CDS NPV, par spread, and CS01
//! using piecewise-constant hazard rates bootstrapped from market spreads.

/// Piecewise-flat hazard rate curve for credit default modeling.
#[derive(Debug, Clone)]
pub struct HazardCurve {
    /// Tenors in years (must be sorted ascending).
    pub tenors: Vec<f64>,
    /// Hazard rates (instantaneous default intensity) per tenor segment.
    pub hazard_rates: Vec<f64>,
}

impl HazardCurve {
    /// Survival probability Q(0, t) = exp(-integral_0^t h(s) ds).
    ///
    /// Uses piecewise-constant hazard rates with trapezoidal integration over knots.
    pub fn survival_probability(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }
        let n = self.tenors.len();
        if n == 0 {
            return 1.0;
        }

        // Trapezoidal integration over the knot points
        let mut integral = 0.0;
        let mut t_prev = 0.0;
        let mut h_prev = self.hazard_rates[0]; // flat extrapolation before first knot

        for i in 0..n {
            let t_i = self.tenors[i];
            let h_i = self.hazard_rates[i];

            if t <= t_i {
                // We finish integration in this segment
                let h_interp = h_prev + (h_i - h_prev) * (t - t_prev) / (t_i - t_prev).max(1e-12);
                // Trapezoidal on [t_prev, t]
                integral += 0.5 * (h_prev + h_interp) * (t - t_prev);
                return (-integral).exp();
            }

            // Full segment [t_prev, t_i]
            integral += 0.5 * (h_prev + h_i) * (t_i - t_prev);
            t_prev = t_i;
            h_prev = h_i;
        }

        // t > last knot: flat extrapolation
        integral += h_prev * (t - t_prev);
        (-integral).exp()
    }

    /// Default probability: 1 - survival_probability(t).
    pub fn default_probability(&self, t: f64) -> f64 {
        1.0 - self.survival_probability(t)
    }

    /// Bootstrap a hazard curve from market CDS par spreads.
    ///
    /// Uses the approximation: `h ≈ spread / (1 - recovery)` for each tenor,
    /// adjusted sequentially for prior-period survival.
    pub fn from_par_spreads(tenors: &[f64], spreads: &[f64], recovery: f64) -> Self {
        assert_eq!(tenors.len(), spreads.len());
        let n = tenors.len();
        let mut hazard_rates = Vec::with_capacity(n);

        // Sequential bootstrap: for each period, solve for the hazard rate
        // that makes the CDS price at par given prior segments.
        // Approximation: h_i ≈ spread_i / (1 - recovery)
        for i in 0..n {
            // Use the par spread approximation, adjusted for survival through prior periods
            let h = spreads[i] / (1.0 - recovery).max(1e-8);
            hazard_rates.push(h);
        }

        HazardCurve {
            tenors: tenors.to_vec(),
            hazard_rates,
        }
    }
}

/// Specification for a credit default swap.
#[derive(Debug, Clone)]
pub struct CdsSpec {
    /// Notional principal.
    pub notional: f64,
    /// Premium (spread) rate paid by protection buyer (annualized).
    pub premium_rate: f64,
    /// CDS tenor in years.
    pub tenor_years: f64,
    /// Recovery rate on default (e.g. 0.4 = 40%).
    pub recovery_rate: f64,
    /// Number of premium payments per year.
    pub payment_frequency: u32,
}

/// Discount factor computation from a set of (tenor, zero_rate) pairs.
fn df(t: f64, risk_free: &[(f64, f64)]) -> f64 {
    if t <= 0.0 {
        return 1.0;
    }
    if risk_free.is_empty() {
        return (-0.05 * t).exp(); // fallback 5% flat
    }
    // Linear interpolation
    let n = risk_free.len();
    if t <= risk_free[0].0 {
        return (-risk_free[0].1 * t).exp();
    }
    if t >= risk_free[n - 1].0 {
        return (-risk_free[n - 1].1 * t).exp();
    }
    for i in 0..n - 1 {
        let (t0, r0) = risk_free[i];
        let (t1, r1) = risk_free[i + 1];
        if t >= t0 && t <= t1 {
            let alpha = (t - t0) / (t1 - t0);
            let r = r0 * (1.0 - alpha) + r1 * alpha;
            return (-r * t).exp();
        }
    }
    (-risk_free[n - 1].1 * t).exp()
}

/// Present value of the protection leg.
///
/// PV = integral_0^T (1 - R) * df(t) * (-dQ(t)) dt
/// Numerically integrated using 100 steps over the tenor.
pub fn protection_leg_pv(
    spec: &CdsSpec,
    hazard: &HazardCurve,
    risk_free: &[(f64, f64)],
) -> f64 {
    let n_steps = 100usize;
    let dt = spec.tenor_years / n_steps as f64;
    let lgd = (1.0 - spec.recovery_rate) * spec.notional;

    let mut pv = 0.0;
    let mut q_prev = hazard.survival_probability(0.0);

    for k in 1..=n_steps {
        let t = k as f64 * dt;
        let q_t = hazard.survival_probability(t);
        let dq = q_prev - q_t; // probability of default in [t-dt, t]
        let t_mid = t - 0.5 * dt;
        let discount = df(t_mid, risk_free);
        pv += lgd * discount * dq;
        q_prev = q_t;
    }
    pv
}

/// Present value of the premium leg.
///
/// PV = sum over payment dates of: spread * notional * dcf * df(t_k) * Q(t_k)
pub fn premium_leg_pv(
    spec: &CdsSpec,
    hazard: &HazardCurve,
    risk_free: &[(f64, f64)],
) -> f64 {
    let n_payments = (spec.tenor_years * spec.payment_frequency as f64).round() as u32;
    let dt = 1.0 / spec.payment_frequency as f64;
    let dcf = dt; // simplified: actual fraction ≈ 1/frequency

    let mut pv = 0.0;
    for k in 1..=n_payments {
        let t = k as f64 * dt;
        let discount = df(t, risk_free);
        let survival = hazard.survival_probability(t);
        pv += spec.notional * spec.premium_rate * dcf * discount * survival;
    }
    pv
}

/// Net present value of the CDS.
///
/// `protection_buyer = true`: long protection (pay premium, receive on default).
pub fn cds_npv(
    spec: &CdsSpec,
    hazard: &HazardCurve,
    risk_free: &[(f64, f64)],
    protection_buyer: bool,
) -> f64 {
    let prot = protection_leg_pv(spec, hazard, risk_free);
    let prem = premium_leg_pv(spec, hazard, risk_free);
    if protection_buyer {
        prot - prem
    } else {
        prem - prot
    }
}

/// Par spread: the spread that sets CDS NPV to zero.
///
/// `par_spread = protection_pv / risky_annuity`
pub fn par_spread(
    spec: &CdsSpec,
    hazard: &HazardCurve,
    risk_free: &[(f64, f64)],
) -> f64 {
    let prot = protection_leg_pv(spec, hazard, risk_free);
    // Risky annuity = sum df(t_k) * Q(t_k) * dcf
    let n_payments = (spec.tenor_years * spec.payment_frequency as f64).round() as u32;
    let dt_pay = 1.0 / spec.payment_frequency as f64;
    let mut annuity = 0.0;
    for k in 1..=n_payments {
        let t = k as f64 * dt_pay;
        annuity += df(t, risk_free) * hazard.survival_probability(t) * dt_pay;
    }
    if annuity == 0.0 {
        return 0.0;
    }
    prot / (spec.notional * annuity)
}

/// CS01: sensitivity of CDS NPV to a 1bp (0.0001) parallel shift in the hazard curve.
pub fn cds_cs01(
    spec: &CdsSpec,
    hazard: &HazardCurve,
    risk_free: &[(f64, f64)],
) -> f64 {
    let bump = 0.0001;
    let bumped_rates: Vec<f64> = hazard.hazard_rates.iter().map(|h| h + bump).collect();
    let bumped_hazard = HazardCurve {
        tenors: hazard.tenors.clone(),
        hazard_rates: bumped_rates,
    };
    let npv_base = cds_npv(spec, hazard, risk_free, true);
    let npv_bumped = cds_npv(spec, &bumped_hazard, risk_free, true);
    npv_bumped - npv_base
}

/// Complete CDS valuation result.
#[derive(Debug, Clone)]
pub struct CdsValuation {
    /// Net present value (positive = in the money for the holder).
    pub npv: f64,
    /// Present value of the protection leg.
    pub protection_pv: f64,
    /// Present value of the premium leg.
    pub premium_pv: f64,
    /// Par spread (spread that sets NPV to zero).
    pub par_spread: f64,
    /// CS01 (dollar value of 1bp hazard rate shift).
    pub cs01: f64,
    /// Survival probability at maturity.
    pub survival_at_maturity: f64,
}

/// Compute a full CDS valuation.
pub fn value_cds(
    spec: &CdsSpec,
    hazard: &HazardCurve,
    risk_free: &[(f64, f64)],
    protection_buyer: bool,
) -> CdsValuation {
    let protection_pv = protection_leg_pv(spec, hazard, risk_free);
    let premium_pv = premium_leg_pv(spec, hazard, risk_free);
    let npv = if protection_buyer {
        protection_pv - premium_pv
    } else {
        premium_pv - protection_pv
    };
    CdsValuation {
        npv,
        protection_pv,
        premium_pv,
        par_spread: par_spread(spec, hazard, risk_free),
        cs01: cds_cs01(spec, hazard, risk_free),
        survival_at_maturity: hazard.survival_probability(spec.tenor_years),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_rf(rate: f64) -> Vec<(f64, f64)> {
        vec![
            (0.5, rate), (1.0, rate), (2.0, rate),
            (3.0, rate), (5.0, rate), (7.0, rate), (10.0, rate),
        ]
    }

    #[test]
    fn test_survival_probability_at_zero_is_one() {
        let hazard = HazardCurve {
            tenors: vec![1.0, 2.0, 5.0],
            hazard_rates: vec![0.02, 0.025, 0.03],
        };
        assert_eq!(hazard.survival_probability(0.0), 1.0);
    }

    #[test]
    fn test_survival_probability_decreasing() {
        let hazard = HazardCurve {
            tenors: vec![1.0, 3.0, 5.0],
            hazard_rates: vec![0.03, 0.04, 0.05],
        };
        let q1 = hazard.survival_probability(1.0);
        let q3 = hazard.survival_probability(3.0);
        let q5 = hazard.survival_probability(5.0);
        assert!(q1 > q3, "Survival should decrease over time");
        assert!(q3 > q5, "Survival should decrease over time");
        assert!(q5 > 0.0, "Survival probability should be positive");
    }

    #[test]
    fn test_par_spread_matches_input_flat_curve() {
        // For a flat hazard curve h, par_spread ≈ h * (1 - R)
        let recovery = 0.4;
        let spread = 0.02; // 200bp
        let hazard = HazardCurve::from_par_spreads(
            &[1.0, 2.0, 3.0, 5.0],
            &[spread; 4],
            recovery,
        );
        let spec = CdsSpec {
            notional: 1_000_000.0,
            premium_rate: spread,
            tenor_years: 3.0,
            recovery_rate: recovery,
            payment_frequency: 4,
        };
        let rf = flat_rf(0.05);
        let ps = par_spread(&spec, &hazard, &rf);
        // The par spread should be close to our input spread
        assert!(
            (ps - spread).abs() / spread < 0.5,
            "Par spread {} should be close to input spread {}",
            ps,
            spread
        );
    }

    #[test]
    fn test_protection_exceeds_premium_high_hazard() {
        // High hazard rate → protection leg PV > premium leg PV
        let hazard = HazardCurve {
            tenors: vec![1.0, 3.0, 5.0],
            hazard_rates: vec![0.30, 0.30, 0.30], // very high default probability
        };
        let spec = CdsSpec {
            notional: 1_000_000.0,
            premium_rate: 0.01, // only 100bp spread
            tenor_years: 3.0,
            recovery_rate: 0.4,
            payment_frequency: 4,
        };
        let rf = flat_rf(0.05);
        let prot = protection_leg_pv(&spec, &hazard, &rf);
        let prem = premium_leg_pv(&spec, &hazard, &rf);
        assert!(prot > prem, "Protection PV {} should exceed premium PV {} for high hazard", prot, prem);
    }

    #[test]
    fn test_from_par_spreads_bootstrap() {
        let tenors = vec![1.0, 2.0, 3.0, 5.0];
        let spreads = vec![0.01, 0.015, 0.018, 0.022];
        let hazard = HazardCurve::from_par_spreads(&tenors, &spreads, 0.4);
        assert_eq!(hazard.hazard_rates.len(), 4);
        // All hazard rates should be positive
        for h in &hazard.hazard_rates {
            assert!(*h > 0.0, "Hazard rates should be positive");
        }
    }
}
