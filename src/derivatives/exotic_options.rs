//! Exotic option pricing: barrier, Asian, lookback, and digital options.
//!
//! All closed-form formulas use the Black-Scholes-Merton framework.
//! Monte Carlo paths use an LCG random number generator with Box-Muller transforms
//! for reproducibility without external dependencies.

use crate::derivatives::option_strategies::OptionType;

// ---------------------------------------------------------------------------
// Core math helpers
// ---------------------------------------------------------------------------

/// Standard normal CDF via Abramowitz & Stegun approximation (error < 7.5e-8).
pub fn norm_cdf(x: f64) -> f64 {
    if x < -8.0 {
        return 0.0;
    }
    if x > 8.0 {
        return 1.0;
    }
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t * (0.319_381_530
        + t * (-0.356_563_782
            + t * (1.781_477_937
                + t * (-1.821_255_978 + t * 1.330_274_429))));
    let pdf = norm_pdf(x);
    let cdf_pos = 1.0 - pdf * poly;
    if x >= 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

/// Standard normal PDF.
pub fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Compute BSM d1 and d2.
///
/// `s` spot, `k` strike, `t` time-to-expiry (years), `r` risk-free rate,
/// `q` continuous dividend yield, `sigma` volatility.
pub fn bsm_d1_d2(s: f64, k: f64, t: f64, r: f64, q: f64, sigma: f64) -> (f64, f64) {
    let sqrt_t = t.sqrt();
    let d1 = ((s / k).ln() + (r - q + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    (d1, d2)
}

// ---------------------------------------------------------------------------
// Simple LCG + Box-Muller RNG (no external deps)
// ---------------------------------------------------------------------------

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_add(1) }
    }

    /// Uniform in (0, 1).
    fn next_f64(&mut self) -> f64 {
        // Numerical Recipes LCG parameters.
        self.state = self.state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal via Box-Muller.
    fn next_normal(&mut self) -> f64 {
        let u1 = self.next_f64().max(1e-15);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

// ---------------------------------------------------------------------------
// Barrier Options
// ---------------------------------------------------------------------------

/// Barrier option type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierType {
    /// Option is knocked out when spot crosses barrier from below.
    UpAndOut,
    /// Option is activated when spot crosses barrier from below.
    UpAndIn,
    /// Option is knocked out when spot crosses barrier from above.
    DownAndOut,
    /// Option is activated when spot crosses barrier from above.
    DownAndIn,
}

/// Closed-form BSM barrier option.
///
/// Uses the Reiner-Rubinstein (1991) analytical formulas.
pub struct BarrierOption {
    /// Current spot price.
    pub s: f64,
    /// Strike price.
    pub k: f64,
    /// Barrier level.
    pub h: f64,
    /// Time to expiry in years.
    pub t: f64,
    /// Risk-free rate.
    pub r: f64,
    /// Continuous dividend yield.
    pub q: f64,
    /// Implied volatility.
    pub sigma: f64,
    /// Barrier type.
    pub barrier_type: BarrierType,
    /// Call or Put.
    pub option_type: OptionType,
}

impl BarrierOption {
    /// Closed-form barrier option price.
    pub fn price(&self) -> f64 {
        let s = self.s;
        let k = self.k;
        let h = self.h;
        let t = self.t;
        let r = self.r;
        let q = self.q;
        let sigma = self.sigma;

        if t <= 0.0 || sigma <= 0.0 {
            return 0.0;
        }

        // Vanilla BSM price for reference.
        let vanilla = self.vanilla_price();

        // mu for the reflection formula.
        let mu = (r - q - 0.5 * sigma * sigma) / (sigma * sigma);
        let lambda = ((mu * mu + 2.0 * r / (sigma * sigma)) as f64).sqrt();
        let sqrt_t = t.sqrt();

        let x1 = (s / k).ln() / (sigma * sqrt_t) + (1.0 + mu) * sigma * sqrt_t;
        let x2 = (s / h).ln() / (sigma * sqrt_t) + (1.0 + mu) * sigma * sqrt_t;
        let y1 = (h * h / (s * k)).ln() / (sigma * sqrt_t) + (1.0 + mu) * sigma * sqrt_t;
        let y2 = (h / s).ln() / (sigma * sqrt_t) + (1.0 + mu) * sigma * sqrt_t;

        let phi: f64 = match self.option_type {
            OptionType::Call => 1.0,
            OptionType::Put => -1.0,
        };

        // eta: +1 for up barriers, -1 for down barriers.
        let eta: f64 = match self.barrier_type {
            BarrierType::UpAndOut | BarrierType::UpAndIn => 1.0,
            BarrierType::DownAndOut | BarrierType::DownAndIn => -1.0,
        };

        let a = phi * s * (-q * t).exp() * norm_cdf(phi * x1)
            - phi * k * (-r * t).exp() * norm_cdf(phi * x1 - phi * sigma * sqrt_t);
        let b = phi * s * (-q * t).exp() * norm_cdf(phi * x2)
            - phi * k * (-r * t).exp() * norm_cdf(phi * x2 - phi * sigma * sqrt_t);
        let c = phi * s * (-q * t).exp() * (h / s).powf(2.0 * (mu + 1.0)) * norm_cdf(eta * y1)
            - phi * k * (-r * t).exp() * (h / s).powf(2.0 * mu) * norm_cdf(eta * y1 - eta * sigma * sqrt_t);
        let d = phi * s * (-q * t).exp() * (h / s).powf(2.0 * (mu + 1.0)) * norm_cdf(eta * y2)
            - phi * k * (-r * t).exp() * (h / s).powf(2.0 * mu) * norm_cdf(eta * y2 - eta * sigma * sqrt_t);

        // Suppress unused lambda warning — it is used in rebate_pv.
        let _ = lambda;

        match (self.option_type.clone(), self.barrier_type) {
            // Down-and-out Call: k >= h
            (OptionType::Call, BarrierType::DownAndOut) if k >= h => a - c,
            // Down-and-out Call: k < h
            (OptionType::Call, BarrierType::DownAndOut) => b - d,
            // Down-and-in Call: k >= h
            (OptionType::Call, BarrierType::DownAndIn) if k >= h => vanilla - (a - c),
            // Down-and-in Call: k < h
            (OptionType::Call, BarrierType::DownAndIn) => vanilla - (b - d),
            // Up-and-out Call: k >= h => zero (always knocked out at inception)
            (OptionType::Call, BarrierType::UpAndOut) if s >= h => 0.0,
            (OptionType::Call, BarrierType::UpAndOut) if k >= h => 0.0,
            (OptionType::Call, BarrierType::UpAndOut) => a - b + c - d,
            // Up-and-in Call
            (OptionType::Call, BarrierType::UpAndIn) if s >= h => vanilla,
            (OptionType::Call, BarrierType::UpAndIn) if k >= h => vanilla,
            (OptionType::Call, BarrierType::UpAndIn) => vanilla - (a - b + c - d),
            // Down-and-out Put
            (OptionType::Put, BarrierType::DownAndOut) if s <= h => 0.0,
            (OptionType::Put, BarrierType::DownAndOut) if k <= h => a - b + c - d,
            (OptionType::Put, BarrierType::DownAndOut) => a - c,
            // Down-and-in Put
            (OptionType::Put, BarrierType::DownAndIn) if s <= h => vanilla,
            (OptionType::Put, BarrierType::DownAndIn) if k <= h => vanilla - (a - b + c - d),
            (OptionType::Put, BarrierType::DownAndIn) => vanilla - (a - c),
            // Up-and-out Put
            (OptionType::Put, BarrierType::UpAndOut) if k <= h => b - d,
            (OptionType::Put, BarrierType::UpAndOut) => 0.0,
            // Up-and-in Put
            (OptionType::Put, BarrierType::UpAndIn) if k <= h => vanilla - (b - d),
            (OptionType::Put, BarrierType::UpAndIn) => vanilla,
        }
    }

    /// Numerical delta via central-difference bump.
    pub fn delta(&self) -> f64 {
        let bump = self.s * 0.001;
        let up = BarrierOption { s: self.s + bump, ..BarrierOption::clone_self(self) };
        let dn = BarrierOption { s: self.s - bump, ..BarrierOption::clone_self(self) };
        (up.price() - dn.price()) / (2.0 * bump)
    }

    /// Present value of a rebate paid when the option is knocked out.
    ///
    /// Uses the reflection formula: PV = R * e^(-r*T) * [ N(z1) + (H/S)^(2*lambda) * N(z2) ]
    /// where lambda = sqrt(mu^2 + 2r/sigma^2).
    pub fn rebate_pv(&self, rebate: f64) -> f64 {
        let s = self.s;
        let h = self.h;
        let t = self.t;
        let r = self.r;
        let sigma = self.sigma;

        if t <= 0.0 || sigma <= 0.0 || rebate == 0.0 {
            return 0.0;
        }

        let mu = (r - self.q - 0.5 * sigma * sigma) / (sigma * sigma);
        let lambda = (mu * mu + 2.0 * r / (sigma * sigma)).sqrt();
        let sqrt_t = t.sqrt();

        let z1 = (h / s).ln() / (sigma * sqrt_t) + lambda * sigma * sqrt_t;
        let z2 = (h / s).ln() / (sigma * sqrt_t) - lambda * sigma * sqrt_t;

        let eta: f64 = match self.barrier_type {
            BarrierType::UpAndOut | BarrierType::UpAndIn => 1.0,
            BarrierType::DownAndOut | BarrierType::DownAndIn => -1.0,
        };

        rebate * (-r * t).exp()
            * (norm_cdf(eta * z1) + (h / s).powf(2.0 * lambda) * norm_cdf(eta * z2))
    }

    fn vanilla_price(&self) -> f64 {
        let (d1, d2) = bsm_d1_d2(self.s, self.k, self.t, self.r, self.q, self.sigma);
        match self.option_type {
            OptionType::Call => {
                self.s * (-self.q * self.t).exp() * norm_cdf(d1)
                    - self.k * (-self.r * self.t).exp() * norm_cdf(d2)
            }
            OptionType::Put => {
                self.k * (-self.r * self.t).exp() * norm_cdf(-d2)
                    - self.s * (-self.q * self.t).exp() * norm_cdf(-d1)
            }
        }
    }

    fn clone_self(o: &BarrierOption) -> BarrierOption {
        BarrierOption {
            s: o.s,
            k: o.k,
            h: o.h,
            t: o.t,
            r: o.r,
            q: o.q,
            sigma: o.sigma,
            barrier_type: o.barrier_type,
            option_type: o.option_type.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Asian Options
// ---------------------------------------------------------------------------

/// Arithmetic-average Asian option (fixed strike).
pub struct AsianOption {
    /// Current spot price.
    pub s: f64,
    /// Strike price.
    pub k: f64,
    /// Time to expiry in years.
    pub t: f64,
    /// Risk-free rate.
    pub r: f64,
    /// Implied volatility.
    pub sigma: f64,
    /// Number of averaging fixings.
    pub n_fixings: usize,
}

impl AsianOption {
    /// Monte Carlo price for arithmetic-average Asian call.
    ///
    /// Uses LCG + Box-Muller. Each path simulates `n_fixings` equally-spaced
    /// price observations; the payoff is max(A - K, 0) discounted at r.
    pub fn price_mc(&self, simulations: usize, seed: u64) -> f64 {
        if simulations == 0 || self.n_fixings == 0 {
            return 0.0;
        }
        let mut rng = Lcg::new(seed);
        let dt = self.t / self.n_fixings as f64;
        let drift = (self.r - 0.5 * self.sigma * self.sigma) * dt;
        let vol_dt = self.sigma * dt.sqrt();
        let mut sum_payoff = 0.0;

        for _ in 0..simulations {
            let mut spot = self.s;
            let mut avg = 0.0;
            for _ in 0..self.n_fixings {
                spot *= (drift + vol_dt * rng.next_normal()).exp();
                avg += spot;
            }
            avg /= self.n_fixings as f64;
            let payoff = (avg - self.k).max(0.0);
            sum_payoff += payoff;
        }

        (sum_payoff / simulations as f64) * (-self.r * self.t).exp()
    }

    /// Closed-form geometric Asian call price (Kemna-Vorst approximation).
    pub fn price_geometric_closed(&self) -> f64 {
        let n = self.n_fixings as f64;
        let sig_g = self.sigma * ((2.0 * n + 1.0) / (6.0 * (n + 1.0))).sqrt();
        let mu_g = 0.5 * (self.r - 0.5 * self.sigma * self.sigma)
            + 0.5 * sig_g * sig_g;
        let d1 = ((self.s / self.k).ln() + (mu_g + 0.5 * sig_g * sig_g) * self.t)
            / (sig_g * self.t.sqrt());
        let d2 = d1 - sig_g * self.t.sqrt();
        (-self.r * self.t).exp()
            * (self.s * (mu_g * self.t).exp() * norm_cdf(d1) - self.k * norm_cdf(d2))
    }

    /// Control-variate price: MC arithmetic + (closed geometric - MC geometric) correction.
    pub fn control_variate_price(&self, simulations: usize, seed: u64) -> f64 {
        if simulations == 0 || self.n_fixings == 0 {
            return 0.0;
        }
        let closed_geo = self.price_geometric_closed();

        let mut rng = Lcg::new(seed);
        let dt = self.t / self.n_fixings as f64;
        let drift = (self.r - 0.5 * self.sigma * self.sigma) * dt;
        let vol_dt = self.sigma * dt.sqrt();

        let mut sum_arith = 0.0;
        let mut sum_geo = 0.0;

        for _ in 0..simulations {
            let mut spot = self.s;
            let mut arith_sum = 0.0;
            let mut log_sum = 0.0;
            for _ in 0..self.n_fixings {
                spot *= (drift + vol_dt * rng.next_normal()).exp();
                arith_sum += spot;
                log_sum += spot.ln();
            }
            let arith_avg = arith_sum / self.n_fixings as f64;
            let geo_avg = (log_sum / self.n_fixings as f64).exp();
            sum_arith += (arith_avg - self.k).max(0.0);
            sum_geo += (geo_avg - self.k).max(0.0);
        }

        let disc = (-self.r * self.t).exp();
        let mc_arith = disc * sum_arith / simulations as f64;
        let mc_geo = disc * sum_geo / simulations as f64;

        // Control variate correction.
        mc_arith + (closed_geo - mc_geo)
    }
}

// ---------------------------------------------------------------------------
// Lookback Options
// ---------------------------------------------------------------------------

/// Floating-strike lookback option (closed-form + MC).
pub struct LookbackOption {
    /// Current spot price.
    pub s: f64,
    /// Strike price (used for fixed-strike variant).
    pub k: f64,
    /// Time to expiry in years.
    pub t: f64,
    /// Risk-free rate.
    pub r: f64,
    /// Implied volatility.
    pub sigma: f64,
}

impl LookbackOption {
    /// Closed-form price for a fixed-strike lookback option.
    ///
    /// Call pays max(S_max - K, 0), Put pays max(K - S_min, 0).
    /// `observed_min_or_max`: observed running min (put) or max (call) so far.
    pub fn price_fixed_strike(&self, observed_min_or_max: f64, is_call: bool) -> f64 {
        let s = self.s;
        let k = self.k;
        let t = self.t;
        let r = self.r;
        let sigma = self.sigma;

        if t <= 0.0 || sigma <= 0.0 {
            return if is_call {
                (observed_min_or_max - k).max(0.0)
            } else {
                (k - observed_min_or_max).max(0.0)
            };
        }

        let b = r; // cost-of-carry = r (no dividends)
        let sqrt_t = t.sqrt();

        if is_call {
            let m = observed_min_or_max.max(s);
            let a1 = ((s / m).ln() + (b + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
            let a2 = a1 - sigma * sqrt_t;
            let a3 = ((s / m).ln() + (-b + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);

            s * (b * t).exp() * norm_cdf(a1)
                - m * (-r * t).exp() * norm_cdf(a2)
                - s * (-r * t).exp() * (sigma * sigma / (2.0 * b))
                    * ((s / m).powf(-2.0 * b / (sigma * sigma)) * norm_cdf(-a3)
                        - (b * t).exp() * norm_cdf(-a1))
        } else {
            let m = observed_min_or_max.min(s);
            let a1 = ((s / m).ln() + (b + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
            let a2 = a1 - sigma * sqrt_t;
            let a3 = ((s / m).ln() + (-b + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);

            m * (-r * t).exp() * norm_cdf(-a2)
                - s * (b * t).exp() * norm_cdf(-a1)
                + s * (-r * t).exp() * (sigma * sigma / (2.0 * b))
                    * ((s / m).powf(-2.0 * b / (sigma * sigma)) * norm_cdf(a3)
                        - (b * t).exp() * norm_cdf(a1))
        }
    }

    /// Monte Carlo price for a floating-strike lookback option.
    ///
    /// Call pays S_T - S_min; Put pays S_max - S_T.
    pub fn price_floating_mc(&self, is_call: bool, simulations: usize, seed: u64) -> f64 {
        if simulations == 0 {
            return 0.0;
        }
        let mut rng = Lcg::new(seed);
        let steps = 252_usize; // daily steps
        let dt = self.t / steps as f64;
        let drift = (self.r - 0.5 * self.sigma * self.sigma) * dt;
        let vol_dt = self.sigma * dt.sqrt();
        let mut sum_payoff = 0.0;

        for _ in 0..simulations {
            let mut spot = self.s;
            let mut running_min = spot;
            let mut running_max = spot;
            for _ in 0..steps {
                spot *= (drift + vol_dt * rng.next_normal()).exp();
                if spot < running_min { running_min = spot; }
                if spot > running_max { running_max = spot; }
            }
            let payoff = if is_call {
                spot - running_min
            } else {
                running_max - spot
            };
            sum_payoff += payoff;
        }

        (sum_payoff / simulations as f64) * (-self.r * self.t).exp()
    }
}

// ---------------------------------------------------------------------------
// Digital Options
// ---------------------------------------------------------------------------

/// Cash-or-nothing and asset-or-nothing digital options.
pub struct DigitalOption {
    /// Current spot price.
    pub s: f64,
    /// Strike price.
    pub k: f64,
    /// Time to expiry in years.
    pub t: f64,
    /// Risk-free rate.
    pub r: f64,
    /// Implied volatility.
    pub sigma: f64,
}

impl DigitalOption {
    /// Cash-or-nothing call: pays `payout` if S_T > K at expiry.
    ///
    /// Price = payout * e^(-rT) * N(d2).
    pub fn cash_or_nothing_call(&self, payout: f64) -> f64 {
        if self.t <= 0.0 || self.sigma <= 0.0 {
            return if self.s > self.k { payout * (-self.r * self.t).exp() } else { 0.0 };
        }
        let (_, d2) = bsm_d1_d2(self.s, self.k, self.t, self.r, 0.0, self.sigma);
        payout * (-self.r * self.t).exp() * norm_cdf(d2)
    }

    /// Asset-or-nothing call: pays S_T if S_T > K at expiry.
    ///
    /// Price = S * N(d1).
    pub fn asset_or_nothing_call(&self) -> f64 {
        if self.t <= 0.0 || self.sigma <= 0.0 {
            return if self.s > self.k { self.s } else { 0.0 };
        }
        let (d1, _) = bsm_d1_d2(self.s, self.k, self.t, self.r, 0.0, self.sigma);
        self.s * norm_cdf(d1)
    }

    /// Gap call option: pays (S_T - trigger) if S_T > K at expiry.
    ///
    /// Uses the asset-or-nothing minus cash-or-nothing decomposition.
    pub fn gap_call(&self, trigger: f64) -> f64 {
        // Asset-or-nothing - trigger * cash-or-nothing-call(1)
        self.asset_or_nothing_call() - trigger * self.cash_or_nothing_call(1.0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_cdf_symmetry() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((norm_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((norm_cdf(-1.96) - 0.025).abs() < 1e-3);
    }

    #[test]
    fn barrier_price_non_negative() {
        let opt = BarrierOption {
            s: 100.0, k: 100.0, h: 120.0, t: 1.0,
            r: 0.05, q: 0.0, sigma: 0.2,
            barrier_type: BarrierType::UpAndOut,
            option_type: OptionType::Call,
        };
        assert!(opt.price() >= 0.0);
        assert!(opt.delta().is_finite());
    }

    #[test]
    fn asian_mc_positive() {
        let opt = AsianOption { s: 100.0, k: 100.0, t: 1.0, r: 0.05, sigma: 0.2, n_fixings: 12 };
        let price = opt.price_mc(10_000, 42);
        assert!(price > 0.0);
        let cv = opt.control_variate_price(10_000, 42);
        assert!(cv > 0.0);
    }

    #[test]
    fn lookback_mc_positive() {
        let opt = LookbackOption { s: 100.0, k: 100.0, t: 1.0, r: 0.05, sigma: 0.2 };
        let price = opt.price_floating_mc(true, 5_000, 7);
        assert!(price >= 0.0);
    }

    #[test]
    fn digital_call_bounds() {
        let opt = DigitalOption { s: 100.0, k: 100.0, t: 1.0, r: 0.05, sigma: 0.2 };
        let p = opt.cash_or_nothing_call(1.0);
        assert!((0.0..=1.0).contains(&p));
        assert!(opt.asset_or_nothing_call() > 0.0);
        assert!(opt.gap_call(90.0) >= 0.0);
    }
}
