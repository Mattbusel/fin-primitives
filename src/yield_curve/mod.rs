//! Yield Curve Modeler
//!
//! Provides yield curve construction, interpolation (linear and natural cubic spline),
//! forward rates, Macaulay duration, convexity, curve shape classification, and the
//! Nelson-Siegel parametric model with gradient-descent fitting.

/// A single point on a yield curve.
#[derive(Debug, Clone, PartialEq)]
pub struct YieldPoint {
    /// Time to maturity in years.
    pub maturity_years: f64,
    /// Continuously-compounded yield rate (e.g. 0.05 = 5%).
    pub yield_rate: f64,
}

/// A yield curve composed of sorted [`YieldPoint`]s.
#[derive(Debug, Clone)]
pub struct YieldCurve {
    /// Points sorted ascending by maturity.
    pub points: Vec<YieldPoint>,
}

impl YieldCurve {
    /// Construct a new [`YieldCurve`], sorting points by maturity ascending.
    pub fn new(mut points: Vec<YieldPoint>) -> Self {
        points.sort_by(|a, b| a.maturity_years.partial_cmp(&b.maturity_years).unwrap_or(std::cmp::Ordering::Equal));
        Self { points }
    }

    /// Linear interpolation of the yield at `maturity`.
    ///
    /// Clamps to the nearest endpoint if `maturity` is outside the curve range.
    pub fn linear_interp(&self, maturity: f64) -> f64 {
        let pts = &self.points;
        if pts.is_empty() {
            return 0.0;
        }
        if maturity <= pts[0].maturity_years {
            return pts[0].yield_rate;
        }
        let last = pts.last().expect("non-empty");
        if maturity >= last.maturity_years {
            return last.yield_rate;
        }
        // Find bracketing interval
        let idx = pts.partition_point(|p| p.maturity_years <= maturity);
        let lo = &pts[idx - 1];
        let hi = &pts[idx];
        let t = (maturity - lo.maturity_years) / (hi.maturity_years - lo.maturity_years);
        lo.yield_rate + t * (hi.yield_rate - lo.yield_rate)
    }

    /// Natural cubic spline interpolation at `maturity`.
    ///
    /// Uses a tridiagonal (Thomas) solver to compute the second-derivative
    /// coefficients with natural boundary conditions (M[0] = M[n-1] = 0).
    /// Clamps to endpoints when outside the data range.
    pub fn cubic_spline(&self, maturity: f64) -> f64 {
        let pts = &self.points;
        let n = pts.len();
        if n == 0 {
            return 0.0;
        }
        if n == 1 {
            return pts[0].yield_rate;
        }
        if n == 2 {
            return self.linear_interp(maturity);
        }

        // Clamp
        if maturity <= pts[0].maturity_years {
            return pts[0].yield_rate;
        }
        if maturity >= pts[n - 1].maturity_years {
            return pts[n - 1].yield_rate;
        }

        // h[i] = x[i+1] - x[i]
        let h: Vec<f64> = (0..n - 1)
            .map(|i| pts[i + 1].maturity_years - pts[i].maturity_years)
            .collect();

        // Build tridiagonal system for M (second derivatives), natural spline: M[0]=M[n-1]=0
        // Size: (n-2) interior unknowns
        let m_count = n - 2;
        if m_count == 0 {
            return self.linear_interp(maturity);
        }

        let mut diag = vec![0.0f64; m_count];
        let mut upper = vec![0.0f64; m_count - 1];
        let mut lower = vec![0.0f64; m_count - 1];
        let mut rhs = vec![0.0f64; m_count];

        for i in 0..m_count {
            let i1 = i + 1; // index in pts
            diag[i] = 2.0 * (h[i] + h[i1]);
            if i > 0 {
                lower[i - 1] = h[i];
            }
            if i < m_count - 1 {
                upper[i] = h[i1];
            }
            rhs[i] = 6.0
                * ((pts[i1 + 1].yield_rate - pts[i1].yield_rate) / h[i1]
                    - (pts[i1].yield_rate - pts[i].yield_rate) / h[i]);
        }

        // Thomas algorithm
        let m_inner = tridiagonal_solve(&lower, &diag, &upper, &rhs);

        // Full M array with boundary zeros
        let mut m = vec![0.0f64; n];
        for i in 0..m_count {
            m[i + 1] = m_inner[i];
        }

        // Find interval
        let idx = pts.partition_point(|p| p.maturity_years <= maturity);
        let i = (idx - 1).min(n - 2);
        let x = maturity - pts[i].maturity_years;
        let hi = h[i];

        let a = pts[i].yield_rate;
        let b_coef = (pts[i + 1].yield_rate - pts[i].yield_rate) / hi
            - hi * (2.0 * m[i] + m[i + 1]) / 6.0;
        let c_coef = m[i] / 2.0;
        let d_coef = (m[i + 1] - m[i]) / (6.0 * hi);

        a + b_coef * x + c_coef * x * x + d_coef * x * x * x
    }

    /// Compute the instantaneous forward rate between maturities `t1` and `t2`.
    ///
    /// Formula: `f = (r2*t2 - r1*t1) / (t2 - t1)`
    pub fn forward_rate(&self, t1: f64, t2: f64) -> f64 {
        assert!(t2 > t1, "t2 must be greater than t1");
        let r1 = self.linear_interp(t1);
        let r2 = self.linear_interp(t2);
        (r2 * t2 - r1 * t1) / (t2 - t1)
    }

    /// Macaulay duration for a stream of cash flows.
    ///
    /// `cash_flows`: slice of `(time_years, cash_flow_amount)`.
    ///
    /// Formula: `D = sum(t * CF * e^(-r(t)*t)) / sum(CF * e^(-r(t)*t))`
    pub fn duration(&self, cash_flows: &[(f64, f64)]) -> f64 {
        let mut numerator = 0.0f64;
        let mut denominator = 0.0f64;
        for &(t, cf) in cash_flows {
            let r = self.linear_interp(t);
            let disc = (cf * (-r * t).exp()).abs();
            numerator += t * disc;
            denominator += disc;
        }
        if denominator.abs() < 1e-15 {
            return 0.0;
        }
        numerator / denominator
    }

    /// Convexity for a stream of cash flows.
    ///
    /// Formula: `C = sum(t^2 * CF * e^(-r(t)*t)) / PV`
    pub fn convexity(&self, cash_flows: &[(f64, f64)]) -> f64 {
        let mut numerator = 0.0f64;
        let mut denominator = 0.0f64;
        for &(t, cf) in cash_flows {
            let r = self.linear_interp(t);
            let disc = (cf * (-r * t).exp()).abs();
            numerator += t * t * disc;
            denominator += disc;
        }
        if denominator.abs() < 1e-15 {
            return 0.0;
        }
        numerator / denominator
    }

    /// Classify the shape of the yield curve.
    pub fn shape(&self) -> CurveShape {
        let pts = &self.points;
        if pts.len() < 2 {
            return CurveShape::Flat;
        }
        let first = pts[0].yield_rate;
        let last = pts[pts.len() - 1].yield_rate;
        let flat_tol = 0.001; // 10 bps

        // Find max interior yield
        let max_interior = pts[1..pts.len() - 1]
            .iter()
            .map(|p| p.yield_rate)
            .fold(f64::NEG_INFINITY, f64::max);

        // Humped: interior peak significantly above both endpoints
        if pts.len() >= 3 && max_interior > first + flat_tol && max_interior > last + flat_tol {
            return CurveShape::Humped;
        }

        if (last - first).abs() < flat_tol {
            CurveShape::Flat
        } else if last > first {
            CurveShape::Normal
        } else {
            CurveShape::Inverted
        }
    }
}

/// Yield curve shape classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurveShape {
    /// Long rates higher than short rates (typical).
    Normal,
    /// Short rates higher than long rates (recessionary signal).
    Inverted,
    /// Short and long rates approximately equal.
    Flat,
    /// Interior rates higher than both endpoints.
    Humped,
}

/// Nelson-Siegel parametric yield curve model.
///
/// `r(t) = beta0 + beta1 * (1 - e^(-t/tau)) / (t/tau)
///                + beta2 * ((1 - e^(-t/tau)) / (t/tau) - e^(-t/tau))`
#[derive(Debug, Clone)]
pub struct NelsonSiegel {
    /// Long-run level of the yield curve.
    pub beta0: f64,
    /// Short-term slope factor.
    pub beta1: f64,
    /// Medium-term curvature factor.
    pub beta2: f64,
    /// Decay parameter (controls where the hump occurs).
    pub tau: f64,
}

impl NelsonSiegel {
    /// Compute the yield at maturity `t` using the Nelson-Siegel formula.
    pub fn rate(&self, t: f64) -> f64 {
        if t <= 1e-10 {
            // Limit as t→0: r(0) = beta0 + beta1
            return self.beta0 + self.beta1;
        }
        let tau = self.tau.max(1e-8);
        let x = t / tau;
        let factor = (1.0 - (-x).exp()) / x;
        self.beta0 + self.beta1 * factor + self.beta2 * (factor - (-x).exp())
    }

    /// Fit a Nelson-Siegel model to observed yield points using gradient descent (500 iterations).
    pub fn fit(points: &[YieldPoint]) -> Self {
        if points.is_empty() {
            return Self { beta0: 0.05, beta1: -0.02, beta2: 0.01, tau: 1.5 };
        }

        let mut beta0 = 0.05f64;
        let mut beta1 = -0.02f64;
        let mut beta2 = 0.01f64;
        let mut tau = 1.5f64;

        let lr = 0.001;
        let n = points.len() as f64;

        for _ in 0..500 {
            let mut db0 = 0.0f64;
            let mut db1 = 0.0f64;
            let mut db2 = 0.0f64;
            let mut dtau = 0.0f64;

            for pt in points {
                let t = pt.maturity_years;
                let r_obs = pt.yield_rate;

                let tau_c = tau.max(1e-8);
                let x = t / tau_c;
                let (factor, exp_x) = if t <= 1e-10 {
                    (1.0, 1.0)
                } else {
                    let ex = (-x).exp();
                    let f = (1.0 - ex) / x;
                    (f, ex)
                };

                let r_pred = beta0 + beta1 * factor + beta2 * (factor - exp_x);
                let err = r_pred - r_obs; // residual

                // Gradients w.r.t. beta0, beta1, beta2
                db0 += 2.0 * err * 1.0;
                db1 += 2.0 * err * factor;
                db2 += 2.0 * err * (factor - exp_x);

                // Gradient w.r.t. tau (numerical approximation via finite difference)
                let eps = tau_c * 1e-5;
                let tau2 = tau_c + eps;
                let x2 = t / tau2;
                let (factor2, exp_x2) = if t <= 1e-10 {
                    (1.0, 1.0)
                } else {
                    let ex2 = (-x2).exp();
                    ((1.0 - ex2) / x2, ex2)
                };
                let r_pred2 = beta0 + beta1 * factor2 + beta2 * (factor2 - exp_x2);
                let dr_dtau = (r_pred2 - r_pred) / eps;
                dtau += 2.0 * err * dr_dtau;
            }

            beta0 -= lr * db0 / n;
            beta1 -= lr * db1 / n;
            beta2 -= lr * db2 / n;
            tau -= lr * dtau / n;
            tau = tau.max(0.01); // keep tau positive
        }

        Self { beta0, beta1, beta2, tau }
    }
}

/// Thomas algorithm for tridiagonal system Ax = d.
/// `lower`: sub-diagonal (len n-1), `diag`: main diagonal (len n),
/// `upper`: super-diagonal (len n-1), `rhs`: right-hand side (len n).
fn tridiagonal_solve(lower: &[f64], diag: &[f64], upper: &[f64], rhs: &[f64]) -> Vec<f64> {
    let n = diag.len();
    if n == 0 {
        return vec![];
    }
    let mut c_prime = vec![0.0f64; n - 1];
    let mut d_prime = vec![0.0f64; n];
    let mut x = vec![0.0f64; n];

    // Forward sweep
    c_prime[0] = if diag[0].abs() < 1e-15 { 0.0 } else { upper[0] / diag[0] };
    d_prime[0] = if diag[0].abs() < 1e-15 { 0.0 } else { rhs[0] / diag[0] };

    for i in 1..n {
        let denom = if i <= n - 1 && i - 1 < lower.len() {
            diag[i] - lower[i - 1] * if i - 1 < c_prime.len() { c_prime[i - 1] } else { 0.0 }
        } else {
            diag[i]
        };
        if i < n - 1 && i < c_prime.len() {
            c_prime[i] = if denom.abs() < 1e-15 { 0.0 } else { upper[i] / denom };
        }
        d_prime[i] = if denom.abs() < 1e-15 { 0.0 } else { (rhs[i] - lower.get(i - 1).copied().unwrap_or(0.0) * d_prime[i - 1]) / denom };
    }

    // Back substitution
    x[n - 1] = d_prime[n - 1];
    for i in (0..n - 1).rev() {
        x[i] = d_prime[i] - c_prime[i] * x[i + 1];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_curve() -> YieldCurve {
        YieldCurve::new(vec![
            YieldPoint { maturity_years: 0.25, yield_rate: 0.04 },
            YieldPoint { maturity_years: 0.5,  yield_rate: 0.042 },
            YieldPoint { maturity_years: 1.0,  yield_rate: 0.045 },
            YieldPoint { maturity_years: 2.0,  yield_rate: 0.048 },
            YieldPoint { maturity_years: 5.0,  yield_rate: 0.052 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.055 },
            YieldPoint { maturity_years: 30.0, yield_rate: 0.058 },
        ])
    }

    #[test]
    fn test_curve_sorted_on_construction() {
        let curve = YieldCurve::new(vec![
            YieldPoint { maturity_years: 5.0, yield_rate: 0.05 },
            YieldPoint { maturity_years: 1.0, yield_rate: 0.04 },
            YieldPoint { maturity_years: 2.0, yield_rate: 0.045 },
        ]);
        assert!(curve.points[0].maturity_years <= curve.points[1].maturity_years);
        assert!(curve.points[1].maturity_years <= curve.points[2].maturity_years);
    }

    #[test]
    fn test_linear_interp_at_knot() {
        let c = sample_curve();
        let r = c.linear_interp(1.0);
        assert!((r - 0.045).abs() < 1e-10);
    }

    #[test]
    fn test_linear_interp_between_knots() {
        let c = sample_curve();
        let r = c.linear_interp(1.5);
        // Between 1y=0.045 and 2y=0.048 → midpoint = 0.0465
        assert!((r - 0.0465).abs() < 1e-10);
    }

    #[test]
    fn test_linear_interp_clamp_low() {
        let c = sample_curve();
        assert!((c.linear_interp(0.0) - 0.04).abs() < 1e-10);
    }

    #[test]
    fn test_linear_interp_clamp_high() {
        let c = sample_curve();
        assert!((c.linear_interp(100.0) - 0.058).abs() < 1e-10);
    }

    #[test]
    fn test_cubic_spline_at_knot() {
        let c = sample_curve();
        let r = c.cubic_spline(1.0);
        assert!((r - 0.045).abs() < 1e-6, "spline at knot should equal knot value, got {r}");
    }

    #[test]
    fn test_cubic_spline_between_knots() {
        let c = sample_curve();
        let r = c.cubic_spline(3.0);
        // Should be between 0.048 and 0.052
        assert!(r > 0.047 && r < 0.054, "spline at 3y = {r}");
    }

    #[test]
    fn test_cubic_spline_clamp_low() {
        let c = sample_curve();
        assert!((c.cubic_spline(0.0) - 0.04).abs() < 1e-10);
    }

    #[test]
    fn test_cubic_spline_clamp_high() {
        let c = sample_curve();
        assert!((c.cubic_spline(100.0) - 0.058).abs() < 1e-10);
    }

    #[test]
    fn test_forward_rate_simple() {
        let c = sample_curve();
        let f = c.forward_rate(1.0, 2.0);
        // r1=0.045, r2=0.048 → (0.048*2 - 0.045*1) / (2-1) = 0.051
        assert!((f - 0.051).abs() < 1e-10);
    }

    #[test]
    fn test_forward_rate_positive() {
        let c = sample_curve();
        let f = c.forward_rate(0.5, 5.0);
        assert!(f > 0.0);
    }

    #[test]
    fn test_duration_single_cash_flow() {
        let c = sample_curve();
        // Single cash flow at t=2: duration = 2
        let d = c.duration(&[(2.0, 1000.0)]);
        assert!((d - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_duration_multiple_cash_flows() {
        let c = sample_curve();
        let cfs = vec![(1.0, 50.0), (2.0, 1050.0)];
        let d = c.duration(&cfs);
        // Duration should be between 1 and 2
        assert!(d > 1.0 && d < 2.0, "duration = {d}");
    }

    #[test]
    fn test_convexity_positive() {
        let c = sample_curve();
        let cfs = vec![(1.0, 50.0), (2.0, 50.0), (3.0, 1050.0)];
        let conv = c.convexity(&cfs);
        assert!(conv > 0.0, "convexity should be positive, got {conv}");
    }

    #[test]
    fn test_convexity_single_cf() {
        let c = sample_curve();
        let conv = c.convexity(&[(3.0, 1000.0)]);
        assert!((conv - 9.0).abs() < 1e-10, "single CF convexity = t^2 = 9, got {conv}");
    }

    #[test]
    fn test_shape_normal() {
        let c = YieldCurve::new(vec![
            YieldPoint { maturity_years: 1.0, yield_rate: 0.03 },
            YieldPoint { maturity_years: 5.0, yield_rate: 0.04 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.05 },
        ]);
        assert_eq!(c.shape(), CurveShape::Normal);
    }

    #[test]
    fn test_shape_inverted() {
        let c = YieldCurve::new(vec![
            YieldPoint { maturity_years: 1.0, yield_rate: 0.05 },
            YieldPoint { maturity_years: 5.0, yield_rate: 0.04 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.03 },
        ]);
        assert_eq!(c.shape(), CurveShape::Inverted);
    }

    #[test]
    fn test_shape_flat() {
        let c = YieldCurve::new(vec![
            YieldPoint { maturity_years: 1.0, yield_rate: 0.04 },
            YieldPoint { maturity_years: 5.0, yield_rate: 0.0405 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.0402 },
        ]);
        assert_eq!(c.shape(), CurveShape::Flat);
    }

    #[test]
    fn test_shape_humped() {
        let c = YieldCurve::new(vec![
            YieldPoint { maturity_years: 1.0,  yield_rate: 0.03 },
            YieldPoint { maturity_years: 3.0,  yield_rate: 0.06 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.03 },
        ]);
        assert_eq!(c.shape(), CurveShape::Humped);
    }

    #[test]
    fn test_nelson_siegel_rate_at_zero() {
        let ns = NelsonSiegel { beta0: 0.05, beta1: -0.02, beta2: 0.01, tau: 1.5 };
        // At t→0: rate = beta0 + beta1 = 0.03
        assert!((ns.rate(0.0) - 0.03).abs() < 1e-10);
    }

    #[test]
    fn test_nelson_siegel_rate_long_end() {
        let ns = NelsonSiegel { beta0: 0.05, beta1: -0.02, beta2: 0.01, tau: 1.5 };
        // At t→∞: rate → beta0 = 0.05
        let r = ns.rate(1000.0);
        assert!((r - 0.05).abs() < 0.001, "long-end rate = {r}");
    }

    #[test]
    fn test_nelson_siegel_fit_convergence() {
        let pts = vec![
            YieldPoint { maturity_years: 0.25, yield_rate: 0.03 },
            YieldPoint { maturity_years: 1.0,  yield_rate: 0.035 },
            YieldPoint { maturity_years: 2.0,  yield_rate: 0.04 },
            YieldPoint { maturity_years: 5.0,  yield_rate: 0.045 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.048 },
        ];
        let ns = NelsonSiegel::fit(&pts);
        // Fitted curve should approximate observed rates reasonably
        for pt in &pts {
            let fitted = ns.rate(pt.maturity_years);
            assert!((fitted - pt.yield_rate).abs() < 0.02,
                "NS fit at {} years: fitted={fitted}, obs={}", pt.maturity_years, pt.yield_rate);
        }
    }

    #[test]
    fn test_nelson_siegel_tau_positive_after_fit() {
        let pts = vec![
            YieldPoint { maturity_years: 1.0, yield_rate: 0.04 },
            YieldPoint { maturity_years: 5.0, yield_rate: 0.05 },
        ];
        let ns = NelsonSiegel::fit(&pts);
        assert!(ns.tau > 0.0, "tau must remain positive after fit");
    }

    #[test]
    fn test_tridiagonal_identity() {
        // Solve [2,1; 1,2] * x = [3; 3] → x = [1; 1]
        let lower = vec![1.0f64];
        let diag  = vec![2.0f64, 2.0f64];
        let upper = vec![1.0f64];
        let rhs   = vec![3.0f64, 3.0f64];
        let sol = tridiagonal_solve(&lower, &diag, &upper, &rhs);
        assert!((sol[0] - 1.0).abs() < 1e-10);
        assert!((sol[1] - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_two_point_curve_cubic_equals_linear() {
        let c = YieldCurve::new(vec![
            YieldPoint { maturity_years: 1.0, yield_rate: 0.04 },
            YieldPoint { maturity_years: 5.0, yield_rate: 0.05 },
        ]);
        let m = 3.0;
        assert!((c.cubic_spline(m) - c.linear_interp(m)).abs() < 1e-10);
    }

    #[test]
    fn test_empty_curve_returns_zero() {
        let c = YieldCurve::new(vec![]);
        assert_eq!(c.linear_interp(1.0), 0.0);
        assert_eq!(c.cubic_spline(1.0), 0.0);
        assert_eq!(c.duration(&[(1.0, 100.0)]), 0.0);
        assert_eq!(c.convexity(&[(1.0, 100.0)]), 0.0);
    }
}
