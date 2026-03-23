//! Yield curve modeling: interpolation, forward rates, Nelson-Siegel fitting.

/// A single point on the yield curve.
#[derive(Debug, Clone, PartialEq)]
pub struct YieldPoint {
    /// Time to maturity in years.
    pub maturity_years: f64,
    /// Continuously compounded yield rate (as a decimal, e.g. 0.05 = 5%).
    pub yield_rate: f64,
}

/// An empirical yield curve built from a set of observed maturity/yield pairs.
#[derive(Debug, Clone)]
pub struct YieldCurve {
    /// Points sorted ascending by maturity.
    pub points: Vec<YieldPoint>,
}

impl YieldCurve {
    /// Construct a `YieldCurve`, sorting points by maturity ascending.
    pub fn new(mut points: Vec<YieldPoint>) -> Self {
        points.sort_by(|a, b| {
            a.maturity_years
                .partial_cmp(&b.maturity_years)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self { points }
    }

    /// Linear interpolation (or flat extrapolation) of the yield at `maturity`.
    pub fn linear_interpolate(&self, maturity: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        // Flat extrapolation at the short end.
        if maturity <= self.points[0].maturity_years {
            return Some(self.points[0].yield_rate);
        }
        // Flat extrapolation at the long end.
        if maturity >= self.points[self.points.len() - 1].maturity_years {
            return Some(self.points[self.points.len() - 1].yield_rate);
        }
        // Find bracketing points.
        for i in 0..self.points.len() - 1 {
            let p0 = &self.points[i];
            let p1 = &self.points[i + 1];
            if maturity >= p0.maturity_years && maturity <= p1.maturity_years {
                let t = (maturity - p0.maturity_years) / (p1.maturity_years - p0.maturity_years);
                return Some(p0.yield_rate + t * (p1.yield_rate - p0.yield_rate));
            }
        }
        None
    }

    /// Natural cubic spline interpolation of the yield at `maturity`.
    ///
    /// Falls back to linear interpolation for fewer than 3 points.
    pub fn cubic_spline_interpolate(&self, maturity: f64) -> f64 {
        let n = self.points.len();
        if n < 3 {
            return self.linear_interpolate(maturity).unwrap_or(0.0);
        }

        // Build natural cubic spline coefficients.
        // h[i] = x[i+1] - x[i]
        let h: Vec<f64> = (0..n - 1)
            .map(|i| self.points[i + 1].maturity_years - self.points[i].maturity_years)
            .collect();

        // Solve tridiagonal system for second derivatives (M[i]).
        let mut alpha = vec![0.0f64; n];
        for i in 1..n - 1 {
            alpha[i] = 3.0
                * ((self.points[i + 1].yield_rate - self.points[i].yield_rate) / h[i]
                    - (self.points[i].yield_rate - self.points[i - 1].yield_rate) / h[i - 1]);
        }

        let mut l = vec![1.0f64; n];
        let mut mu = vec![0.0f64; n];
        let mut z = vec![0.0f64; n];

        for i in 1..n - 1 {
            l[i] = 2.0 * (self.points[i + 1].maturity_years - self.points[i - 1].maturity_years)
                - h[i - 1] * mu[i - 1];
            if l[i].abs() < 1e-15 {
                l[i] = 1e-15;
            }
            mu[i] = h[i] / l[i];
            z[i] = (alpha[i] - h[i - 1] * z[i - 1]) / l[i];
        }

        let mut c = vec![0.0f64; n];
        let mut b = vec![0.0f64; n];
        let mut d = vec![0.0f64; n];

        for j in (0..n - 1).rev() {
            c[j] = z[j] - mu[j] * c[j + 1];
            b[j] = (self.points[j + 1].yield_rate - self.points[j].yield_rate) / h[j]
                - h[j] * (c[j + 1] + 2.0 * c[j]) / 3.0;
            d[j] = (c[j + 1] - c[j]) / (3.0 * h[j]);
        }

        // Evaluate spline.
        let t = maturity;
        // Clamp to range.
        if t <= self.points[0].maturity_years {
            return self.points[0].yield_rate;
        }
        if t >= self.points[n - 1].maturity_years {
            return self.points[n - 1].yield_rate;
        }

        for i in 0..n - 1 {
            if t >= self.points[i].maturity_years && t <= self.points[i + 1].maturity_years {
                let dx = t - self.points[i].maturity_years;
                let a = self.points[i].yield_rate;
                return a + b[i] * dx + c[i] * dx * dx + d[i] * dx * dx * dx;
            }
        }

        self.linear_interpolate(maturity).unwrap_or(0.0)
    }

    /// Forward rate between maturities `t1` and `t2` (t2 > t1).
    ///
    /// Uses: f(t1, t2) = (t2*y2 - t1*y1) / (t2 - t1)
    pub fn forward_rate(&self, t1: f64, t2: f64) -> f64 {
        if (t2 - t1).abs() < 1e-12 {
            return self.linear_interpolate(t1).unwrap_or(0.0);
        }
        let y1 = self.linear_interpolate(t1).unwrap_or(0.0);
        let y2 = self.linear_interpolate(t2).unwrap_or(0.0);
        (t2 * y2 - t1 * y1) / (t2 - t1)
    }

    /// Par yield at a given maturity: the coupon rate that prices the bond at par.
    ///
    /// Uses iterative calculation: c = (1 - df(T)) / sum(df(t_i)) where
    /// discount factors use the interpolated zero rates.
    pub fn par_yield(&self, maturity: f64) -> f64 {
        // Use semi-annual coupon periods.
        let n_periods = (maturity * 2.0).round() as usize;
        if n_periods == 0 {
            return self.linear_interpolate(maturity).unwrap_or(0.0);
        }

        let dt = maturity / n_periods as f64;
        let mut sum_df = 0.0;
        for i in 1..=n_periods {
            let t = i as f64 * dt;
            sum_df += self.discount_factor(t);
        }

        let df_t = self.discount_factor(maturity);
        if sum_df.abs() < 1e-12 {
            return 0.0;
        }

        // Par yield (annualized): c such that c * dt * sum_df + df_t = 1
        (1.0 - df_t) / (dt * sum_df)
    }

    /// Continuous compounding discount factor: exp(-y * t).
    pub fn discount_factor(&self, maturity: f64) -> f64 {
        let y = self.linear_interpolate(maturity).unwrap_or(0.0);
        (-y * maturity).exp()
    }
}

/// Nelson-Siegel yield curve model.
///
/// Yield(t) = β0 + β1 * [(1 - e^(-t/λ)) / (t/λ)]
///           + β2 * [(1 - e^(-t/λ)) / (t/λ) - e^(-t/λ)]
#[derive(Debug, Clone)]
pub struct NelsonSiegel {
    /// Level factor (long-run yield).
    pub beta0: f64,
    /// Slope factor (short-rate minus long-rate).
    pub beta1: f64,
    /// Curvature factor (hump shape).
    pub beta2: f64,
    /// Decay parameter (controls where the hump occurs).
    pub lambda: f64,
}

impl NelsonSiegel {
    /// Compute the Nelson-Siegel yield at maturity `t`.
    pub fn yield_at(&self, maturity: f64) -> f64 {
        if maturity < 1e-10 {
            // Limit as t -> 0: yield = beta0 + beta1
            return self.beta0 + self.beta1;
        }
        let lt = maturity / self.lambda;
        let factor1 = (1.0 - (-lt).exp()) / lt;
        let factor2 = factor1 - (-lt).exp();
        self.beta0 + self.beta1 * factor1 + self.beta2 * factor2
    }

    /// Fit Nelson-Siegel model to observed yield points via OLS.
    ///
    /// For a fixed lambda (0.5), solves for beta0, beta1, beta2 via
    /// least-squares (normal equations, 3x3 system via Gaussian elimination).
    pub fn fit(points: &[YieldPoint]) -> Self {
        if points.is_empty() {
            return Self {
                beta0: 0.0,
                beta1: 0.0,
                beta2: 0.0,
                lambda: 0.5,
            };
        }

        let lambda = 0.5_f64;

        // Build design matrix X (n x 3) and response y (n x 1).
        let n = points.len();
        let mut xtx = [[0.0f64; 3]; 3];
        let mut xty = [0.0f64; 3];

        for p in points {
            let t = p.maturity_years;
            let y = p.yield_rate;

            let x0 = 1.0_f64;
            let (x1, x2) = if t < 1e-10 {
                (1.0, 0.0)
            } else {
                let lt = t / lambda;
                let f1 = (1.0 - (-lt).exp()) / lt;
                let f2 = f1 - (-lt).exp();
                (f1, f2)
            };

            let row = [x0, x1, x2];

            for i in 0..3 {
                xty[i] += row[i] * y;
                for j in 0..3 {
                    xtx[i][j] += row[i] * row[j];
                }
            }
        }

        // Solve 3x3 system via Gaussian elimination.
        let betas = solve_3x3(&xtx, &xty);

        let (beta0, beta1, beta2) = if n == 1 {
            // With a single point, just set level to that yield.
            (points[0].yield_rate, 0.0, 0.0)
        } else {
            (betas[0], betas[1], betas[2])
        };

        Self {
            beta0,
            beta1,
            beta2,
            lambda,
        }
    }

    /// Nelson-Siegel instantaneous forward rate at maturity `t`.
    pub fn forward_rate(&self, maturity: f64) -> f64 {
        if maturity < 1e-10 {
            return self.instantaneous_forward();
        }
        let lt = maturity / self.lambda;
        let e = (-lt).exp();
        // d/dt [t * yield(t)] = yield(t) + t * yield'(t)
        // NS forward: f(t) = beta0 + beta1*e^(-t/lambda) + beta2*(t/lambda)*e^(-t/lambda)
        self.beta0 + self.beta1 * e + self.beta2 * lt * e
    }

    /// Instantaneous forward rate as t -> 0: β0 + β1.
    pub fn instantaneous_forward(&self) -> f64 {
        self.beta0 + self.beta1
    }
}

/// Summary metrics derived from a Nelson-Siegel fit.
#[derive(Debug, Clone)]
pub struct CurveMetrics {
    /// Long-run yield level (beta0).
    pub level: f64,
    /// Slope of the curve — short vs long rate difference (beta1).
    pub slope: f64,
    /// Curvature / hump magnitude (beta2).
    pub curvature: f64,
}

impl CurveMetrics {
    /// Extract metrics directly from a fitted `NelsonSiegel` model.
    pub fn from_nelson_siegel(ns: &NelsonSiegel) -> Self {
        Self {
            level: ns.beta0,
            slope: ns.beta1,
            curvature: ns.beta2,
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Solve a 3x3 linear system Ax = b via Gaussian elimination with partial pivoting.
/// Returns [x0, x1, x2] or zeros on singular matrix.
fn solve_3x3(a: &[[f64; 3]; 3], b: &[f64; 3]) -> [f64; 3] {
    let mut aug = [
        [a[0][0], a[0][1], a[0][2], b[0]],
        [a[1][0], a[1][1], a[1][2], b[1]],
        [a[2][0], a[2][1], a[2][2], b[2]],
    ];

    for col in 0..3 {
        // Partial pivot.
        let mut max_row = col;
        let mut max_val = aug[col][col].abs();
        for row in col + 1..3 {
            if aug[row][col].abs() > max_val {
                max_val = aug[row][col].abs();
                max_row = row;
            }
        }
        aug.swap(col, max_row);

        let pivot = aug[col][col];
        if pivot.abs() < 1e-14 {
            return [0.0; 3];
        }

        for row in col + 1..3 {
            let factor = aug[row][col] / pivot;
            for k in col..4 {
                let val = aug[col][k] * factor;
                aug[row][k] -= val;
            }
        }
    }

    // Back-substitution.
    let mut x = [0.0f64; 3];
    for i in (0..3).rev() {
        let mut sum = aug[i][3];
        for j in i + 1..3 {
            sum -= aug[i][j] * x[j];
        }
        let denom = aug[i][i];
        if denom.abs() < 1e-14 {
            return [0.0; 3];
        }
        x[i] = sum / denom;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_curve() -> YieldCurve {
        YieldCurve::new(vec![
            YieldPoint { maturity_years: 0.25, yield_rate: 0.04 },
            YieldPoint { maturity_years: 1.0,  yield_rate: 0.045 },
            YieldPoint { maturity_years: 2.0,  yield_rate: 0.050 },
            YieldPoint { maturity_years: 5.0,  yield_rate: 0.055 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.060 },
            YieldPoint { maturity_years: 30.0, yield_rate: 0.065 },
        ])
    }

    #[test]
    fn linear_interpolate_at_knot() {
        let c = sample_curve();
        let y = c.linear_interpolate(1.0).unwrap();
        assert!((y - 0.045).abs() < 1e-10);
    }

    #[test]
    fn linear_interpolate_midpoint() {
        let c = sample_curve();
        let y = c.linear_interpolate(1.5).unwrap();
        assert!((y - 0.0475).abs() < 1e-10);
    }

    #[test]
    fn discount_factor_decreasing() {
        let c = sample_curve();
        let df1 = c.discount_factor(1.0);
        let df5 = c.discount_factor(5.0);
        assert!(df1 > df5);
        assert!(df5 > 0.0);
    }

    #[test]
    fn forward_rate_positive() {
        let c = sample_curve();
        let fwd = c.forward_rate(1.0, 5.0);
        assert!(fwd > 0.0);
    }

    #[test]
    fn nelson_siegel_fit_and_yield() {
        let points: Vec<YieldPoint> = vec![
            YieldPoint { maturity_years: 1.0,  yield_rate: 0.04 },
            YieldPoint { maturity_years: 2.0,  yield_rate: 0.045 },
            YieldPoint { maturity_years: 5.0,  yield_rate: 0.05 },
            YieldPoint { maturity_years: 10.0, yield_rate: 0.055 },
            YieldPoint { maturity_years: 30.0, yield_rate: 0.06 },
        ];
        let ns = NelsonSiegel::fit(&points);
        // Level should be near the long-end yield.
        assert!(ns.beta0 > 0.0);
        // Yields should be in a reasonable range.
        for p in &points {
            let y = ns.yield_at(p.maturity_years);
            assert!(y > -0.1 && y < 0.5, "unreasonable yield {y}");
        }
    }

    #[test]
    fn nelson_siegel_instantaneous_forward() {
        let ns = NelsonSiegel { beta0: 0.06, beta1: -0.02, beta2: 0.01, lambda: 0.5 };
        assert!((ns.instantaneous_forward() - 0.04).abs() < 1e-10);
    }

    #[test]
    fn curve_metrics_from_ns() {
        let ns = NelsonSiegel { beta0: 0.05, beta1: -0.01, beta2: 0.02, lambda: 1.0 };
        let m = CurveMetrics::from_nelson_siegel(&ns);
        assert!((m.level - 0.05).abs() < 1e-10);
        assert!((m.slope - (-0.01)).abs() < 1e-10);
        assert!((m.curvature - 0.02).abs() < 1e-10);
    }

    #[test]
    fn cubic_spline_at_knot() {
        let c = sample_curve();
        let y = c.cubic_spline_interpolate(1.0);
        assert!((y - 0.045).abs() < 1e-8);
    }
}
