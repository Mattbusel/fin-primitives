//! Short-rate models: Vasicek, CIR, Hull-White

use std::f64::consts::PI;

/// Vasicek model: dr = a(b - r)dt + sigma*dW
pub struct VasicekModel {
    /// Mean reversion speed
    pub a: f64,
    /// Long-term mean
    pub b: f64,
    /// Volatility
    pub sigma: f64,
    /// Initial short rate
    pub r0: f64,
}

impl VasicekModel {
    /// Construct a new Vasicek model with the given parameters.
    pub fn new(a: f64, b: f64, sigma: f64, r0: f64) -> Self {
        VasicekModel { a, b, sigma, r0 }
    }

    /// E[r(t)] = b + (r0 - b)*exp(-a*t)
    pub fn expected_rate(&self, t: f64) -> f64 {
        self.b + (self.r0 - self.b) * (-self.a * t).exp()
    }

    /// Var[r(t)] = sigma^2 / (2a) * (1 - exp(-2at))
    pub fn variance(&self, t: f64) -> f64 {
        self.sigma * self.sigma / (2.0 * self.a) * (1.0 - (-2.0 * self.a * t).exp())
    }

    /// Zero-coupon bond price P(0,T) = A(T)*exp(-B(T)*r0)
    pub fn bond_price(&self, t: f64) -> f64 {
        let b_t = (1.0 - (-self.a * t).exp()) / self.a;
        let a_t = ((self.b - self.sigma*self.sigma / (2.0*self.a*self.a)) * (b_t - t)
                   - self.sigma*self.sigma * b_t*b_t / (4.0*self.a)).exp();
        a_t * (-b_t * self.r0).exp()
    }

    /// Simulate a short-rate path using Euler-Maruyama discretisation.
    pub fn simulate(&self, dt: f64, steps: usize, seed: u64) -> Vec<f64> {
        let mut r = self.r0;
        let mut path = vec![r];
        let mut state = seed;
        let sqrt_dt = dt.sqrt();
        for _ in 0..steps {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u1 = (state >> 11) as f64 / (1u64 << 53) as f64;
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u2 = (state >> 11) as f64 / (1u64 << 53) as f64;
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
            r += self.a * (self.b - r) * dt + self.sigma * sqrt_dt * z;
            path.push(r);
        }
        path
    }
}

/// Cox-Ingersoll-Ross model: dr = a(b - r)dt + sigma*sqrt(r)*dW
pub struct CirModel {
    /// Mean reversion speed
    pub a: f64,
    /// Long-term mean
    pub b: f64,
    /// Volatility coefficient
    pub sigma: f64,
    /// Initial short rate
    pub r0: f64,
}

impl CirModel {
    /// Construct a new CIR model.
    pub fn new(a: f64, b: f64, sigma: f64, r0: f64) -> Self {
        CirModel { a, b, sigma, r0 }
    }

    /// Feller condition: 2ab >= sigma^2 (guarantees strict positivity of r).
    pub fn feller_condition(&self) -> bool {
        2.0 * self.a * self.b >= self.sigma * self.sigma
    }

    /// Expected short rate at time t.
    pub fn expected_rate(&self, t: f64) -> f64 {
        self.r0 * (-self.a * t).exp() + self.b * (1.0 - (-self.a * t).exp())
    }

    /// Bond price: P(0,T) = A(T)*exp(-B(T)*r0)
    pub fn bond_price(&self, t: f64) -> f64 {
        let gamma = (self.a*self.a + 2.0*self.sigma*self.sigma).sqrt();
        let denom = (gamma + self.a) * ((-gamma * t).exp() - 1.0) + 2.0 * gamma;
        let b_t = 2.0 * ((-gamma * t).exp() - 1.0) / denom;
        let a_t_ln = 2.0 * self.a * self.b / (self.sigma * self.sigma)
            * (2.0 * gamma * (((self.a + gamma) * t / 2.0).exp()) / denom).ln();
        a_t_ln.exp() * (-b_t * self.r0).exp()
    }

    /// Simulate a short-rate path using reflected Euler-Maruyama.
    pub fn simulate(&self, dt: f64, steps: usize, seed: u64) -> Vec<f64> {
        let mut r = self.r0.max(0.0);
        let mut path = vec![r];
        let mut state = seed;
        let sqrt_dt = dt.sqrt();
        for _ in 0..steps {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u1 = ((state >> 11) as f64 / (1u64 << 53) as f64).max(1e-10);
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u2 = (state >> 11) as f64 / (1u64 << 53) as f64;
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
            let dr = self.a * (self.b - r) * dt + self.sigma * r.max(0.0).sqrt() * sqrt_dt * z;
            r = (r + dr).max(0.0);
            path.push(r);
        }
        path
    }
}

/// Hull-White (extended Vasicek): dr = (theta(t) - a*r)dt + sigma*dW
///
/// theta(t) is calibrated to fit the initial yield curve.
pub struct HullWhiteModel {
    /// Mean reversion speed
    pub a: f64,
    /// Volatility
    pub sigma: f64,
    /// Initial short rate
    pub r0: f64,
    /// Piecewise-constant theta schedule: (time, theta) pairs sorted by time.
    pub theta_params: Vec<(f64, f64)>,
}

impl HullWhiteModel {
    /// Construct a Hull-White model with no theta calibration yet.
    pub fn new(a: f64, sigma: f64, r0: f64) -> Self {
        HullWhiteModel { a, sigma, r0, theta_params: Vec::new() }
    }

    /// Return the piecewise-constant theta value at time t.
    pub fn theta_at(&self, t: f64) -> f64 {
        // Find latest theta <= t
        self.theta_params.iter()
            .filter(|(ti, _)| *ti <= t)
            .last()
            .map(|(_, th)| *th)
            .unwrap_or(self.a * 0.05) // default: calibrated to 5% mean
    }

    /// Add a theta breakpoint (builder pattern).
    pub fn add_theta(mut self, t: f64, theta: f64) -> Self {
        self.theta_params.push((t, theta));
        self.theta_params.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        self
    }

    /// Calibrate theta to fit zero-coupon bond prices from a yield curve.
    ///
    /// Simple approximation: theta(t) = a*fwd(t) + sigma^2/(2a)*(1-exp(-2at))
    pub fn calibrate_to_yield_curve(a: f64, sigma: f64, r0: f64,
                                     curve_points: &[(f64, f64)]) -> Self {
        let mut hw = HullWhiteModel::new(a, sigma, r0);
        for window in curve_points.windows(2) {
            let (t1, r1) = window[0];
            let (t2, r2) = window[1];
            let fwd = (r2*t2 - r1*t1) / (t2 - t1); // forward rate approx
            let t_mid = (t1 + t2) / 2.0;
            let theta = a * fwd + sigma*sigma / (2.0*a) * (1.0 - (-2.0*a*t_mid).exp());
            hw.theta_params.push((t_mid, theta));
        }
        hw
    }

    /// Approximate zero-coupon bond price under Hull-White.
    pub fn bond_price(&self, t: f64) -> f64 {
        // B(0,T) = (1 - exp(-aT)) / a
        let b_t = (1.0 - (-self.a * t).exp()) / self.a;
        // Simplified A(T) assuming constant theta
        let theta_avg = if self.theta_params.is_empty() { self.a * 0.05 }
                        else { self.theta_params.iter().map(|(_, th)| th).sum::<f64>() / self.theta_params.len() as f64 };
        let ln_a = theta_avg / self.a * (b_t - t)
                   - self.sigma*self.sigma / (4.0*self.a) * b_t*b_t;
        ln_a.exp() * (-b_t * self.r0).exp()
    }

    /// Simulate a short-rate path using Euler-Maruyama with piecewise theta.
    pub fn simulate(&self, dt: f64, steps: usize, seed: u64) -> Vec<f64> {
        let mut r = self.r0;
        let mut path = vec![r];
        let mut state = seed;
        let sqrt_dt = dt.sqrt();
        let mut t = 0.0;
        for _ in 0..steps {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u1 = ((state >> 11) as f64 / (1u64 << 53) as f64).max(1e-10);
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u2 = (state >> 11) as f64 / (1u64 << 53) as f64;
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
            let theta = self.theta_at(t);
            r += (theta - self.a * r) * dt + self.sigma * sqrt_dt * z;
            path.push(r);
            t += dt;
        }
        path
    }
}

/// Interest rate caplet price under the Vasicek model (Jamshidian 1989).
pub fn vasicek_caplet_price(model: &VasicekModel, strike: f64, t: f64, tau: f64) -> f64 {
    let p_t = model.bond_price(t);
    let p_ttau = model.bond_price(t + tau);
    let b_tau = (1.0 - (-model.a * tau).exp()) / model.a;
    let sigma_p = model.sigma * b_tau * ((1.0 - (-2.0*model.a*t).exp()) / (2.0*model.a)).sqrt();

    if sigma_p < 1e-10 { return 0.0; }
    let h = (p_ttau / (p_t * (1.0 + strike * tau))).ln() / sigma_p + sigma_p / 2.0;
    let nd1 = norm_cdf(h);
    let nd2 = norm_cdf(h - sigma_p);
    p_ttau * nd1 - p_t * (1.0 + strike * tau) * nd2
}

fn norm_cdf(x: f64) -> f64 {
    0.5 * (1.0 + libm_erf(x / std::f64::consts::SQRT_2))
}

fn libm_erf(x: f64) -> f64 {
    // Abramowitz & Stegun approximation
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let result = 1.0 - poly * (-(x*x)).exp();
    if x >= 0.0 { result } else { -result }
}
