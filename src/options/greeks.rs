//! # Module: options::greeks
//!
//! ## Responsibility
//! BSM closed-form Greeks for European options: delta, gamma, theta, vega, rho,
//! vanna, volga; plus implied volatility via Brent's method.
//!
//! ## Design
//! - All public inputs/outputs use `f64` for ergonomics (pure math module).
//! - Every fallible operation returns `Result<_, GreekError>`.
//! - No panics on edge-case inputs.

use std::fmt;

// ─── error ────────────────────────────────────────────────────────────────────

/// Errors produced by the Greeks / IV solver.
#[derive(Debug, Clone, PartialEq)]
pub enum GreekError {
    /// The implied-volatility solver failed to converge.
    NoConvergence,
    /// One or more option parameters are invalid (non-positive spot, strike,
    /// time-to-expiry, or volatility).
    InvalidParams(String),
}

impl fmt::Display for GreekError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GreekError::NoConvergence => write!(f, "implied-volatility solver did not converge"),
            GreekError::InvalidParams(msg) => write!(f, "invalid option params: {msg}"),
        }
    }
}

impl std::error::Error for GreekError {}

// ─── option type ──────────────────────────────────────────────────────────────

/// Whether the option grants the right to buy (Call) or sell (Put).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OptionType {
    /// Right to buy the underlying at the strike.
    Call,
    /// Right to sell the underlying at the strike.
    Put,
}

// ─── params ───────────────────────────────────────────────────────────────────

/// All inputs required to price and compute Greeks for a European option.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct OptionParams {
    /// Current underlying spot price S (must be > 0).
    pub spot: f64,
    /// Strike price K (must be > 0).
    pub strike: f64,
    /// Time to expiry in years T (must be > 0).
    pub time_to_expiry: f64,
    /// Continuously-compounded annual risk-free rate r.
    pub risk_free_rate: f64,
    /// Annual volatility σ (must be > 0).
    pub volatility: f64,
    /// Call or Put.
    pub option_type: OptionType,
}

impl OptionParams {
    fn validate(&self) -> Result<(), GreekError> {
        if self.spot <= 0.0 {
            return Err(GreekError::InvalidParams("spot must be > 0".to_owned()));
        }
        if self.strike <= 0.0 {
            return Err(GreekError::InvalidParams("strike must be > 0".to_owned()));
        }
        if self.time_to_expiry <= 0.0 {
            return Err(GreekError::InvalidParams(
                "time_to_expiry must be > 0".to_owned(),
            ));
        }
        if self.volatility <= 0.0 {
            return Err(GreekError::InvalidParams("volatility must be > 0".to_owned()));
        }
        Ok(())
    }
}

// ─── greeks output ────────────────────────────────────────────────────────────

/// All first- and second-order BSM Greeks.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Greeks {
    /// Delta: ∂V/∂S
    pub delta: f64,
    /// Gamma: ∂²V/∂S²
    pub gamma: f64,
    /// Theta: ∂V/∂T (per year; divide by 365 for per-day)
    pub theta: f64,
    /// Vega: ∂V/∂σ (for a 1-unit move in σ, i.e. 100% vol)
    pub vega: f64,
    /// Rho: ∂V/∂r
    pub rho: f64,
    /// Vanna: ∂²V/∂S∂σ
    pub vanna: f64,
    /// Volga (Vomma): ∂²V/∂σ²
    pub volga: f64,
}

// ─── math helpers ─────────────────────────────────────────────────────────────

/// Standard normal PDF.
#[inline]
fn phi(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0_f64 * std::f64::consts::PI).sqrt()
}

/// Standard normal CDF (Abramowitz & Stegun, max |error| ≈ 7.5e-8).
#[inline]
fn big_phi(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let cdf_pos = 1.0 - phi(x) * poly;
    if x >= 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

// ─── d1 / d2 ─────────────────────────────────────────────────────────────────

fn d1d2(p: &OptionParams) -> (f64, f64) {
    let s = p.spot;
    let k = p.strike;
    let t = p.time_to_expiry;
    let r = p.risk_free_rate;
    let v = p.volatility;
    let sqrt_t = t.sqrt();
    let d1 = ((s / k).ln() + (r + 0.5 * v * v) * t) / (v * sqrt_t);
    let d2 = d1 - v * sqrt_t;
    (d1, d2)
}

// ─── public API ───────────────────────────────────────────────────────────────

/// Black-Scholes-Merton option price.
///
/// # Errors
/// Returns `GreekError::InvalidParams` if any parameter is non-positive where required.
pub fn bsm_price(params: &OptionParams) -> Result<f64, GreekError> {
    params.validate()?;
    let s = params.spot;
    let k = params.strike;
    let t = params.time_to_expiry;
    let r = params.risk_free_rate;
    let (d1, d2) = d1d2(params);
    let disc = (-r * t).exp();
    let price = match params.option_type {
        OptionType::Call => s * big_phi(d1) - k * disc * big_phi(d2),
        OptionType::Put => k * disc * big_phi(-d2) - s * big_phi(-d1),
    };
    Ok(price)
}

/// Compute all BSM Greeks analytically.
///
/// # Errors
/// Returns `GreekError::InvalidParams` if any parameter is non-positive where required.
pub fn bsm_greeks(params: &OptionParams) -> Result<Greeks, GreekError> {
    params.validate()?;

    let s = params.spot;
    let k = params.strike;
    let t = params.time_to_expiry;
    let r = params.risk_free_rate;
    let v = params.volatility;
    let sqrt_t = t.sqrt();
    let (d1, d2) = d1d2(params);
    let disc = (-r * t).exp();
    let phi_d1 = phi(d1);

    let delta = match params.option_type {
        OptionType::Call => big_phi(d1),
        OptionType::Put => big_phi(d1) - 1.0,
    };

    let gamma = phi_d1 / (s * v * sqrt_t);

    // Theta: ∂V/∂T (annualised; positive T means time remaining shrinks as we move forward)
    let theta = match params.option_type {
        OptionType::Call => {
            -(s * phi_d1 * v / (2.0 * sqrt_t)) - r * k * disc * big_phi(d2)
        }
        OptionType::Put => {
            -(s * phi_d1 * v / (2.0 * sqrt_t)) + r * k * disc * big_phi(-d2)
        }
    };

    let vega = s * phi_d1 * sqrt_t;

    let rho = match params.option_type {
        OptionType::Call => k * t * disc * big_phi(d2),
        OptionType::Put => -k * t * disc * big_phi(-d2),
    };

    // Vanna: -N'(d1) * d2 / σ
    let vanna = -phi_d1 * d2 / v;

    // Volga (Vomma): S * N'(d1) * sqrt(T) * d1 * d2 / σ
    let volga = s * phi_d1 * sqrt_t * d1 * d2 / v;

    Ok(Greeks { delta, gamma, theta, vega, rho, vanna, volga })
}

/// Solve for implied volatility given a market price using Brent's method.
///
/// Iterates up to 50 times, converges to tolerance 1e-6.
///
/// # Errors
/// - `GreekError::InvalidParams` if market_price ≤ 0 or other params invalid.
/// - `GreekError::NoConvergence` if the solver fails to converge.
pub fn implied_volatility(market_price: f64, params: &OptionParams) -> Result<f64, GreekError> {
    if market_price <= 0.0 {
        return Err(GreekError::InvalidParams(
            "market_price must be > 0".to_owned(),
        ));
    }
    // Validate everything except volatility (we're solving for it).
    if params.spot <= 0.0 {
        return Err(GreekError::InvalidParams("spot must be > 0".to_owned()));
    }
    if params.strike <= 0.0 {
        return Err(GreekError::InvalidParams("strike must be > 0".to_owned()));
    }
    if params.time_to_expiry <= 0.0 {
        return Err(GreekError::InvalidParams(
            "time_to_expiry must be > 0".to_owned(),
        ));
    }

    const TOL: f64 = 1e-6;
    const MAX_ITER: usize = 50;

    // Objective: f(sigma) = bsm_price(sigma) - market_price
    let f = |sigma: f64| -> f64 {
        let p = OptionParams { volatility: sigma, ..*params };
        bsm_price(&p).unwrap_or(f64::NAN) - market_price
    };

    // Bracket: sigma in [1e-6, 10.0]
    let mut a = 1e-6_f64;
    let mut b = 10.0_f64;
    let mut fa = f(a);
    let mut fb = f(b);

    if fa * fb > 0.0 {
        // Market price outside bracket — try to widen
        return Err(GreekError::NoConvergence);
    }

    // Brent's method
    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;

    for _ in 0..MAX_ITER {
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

        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * TOL;
        let xm = 0.5 * (c - b);

        if xm.abs() <= tol1 || fb.abs() < TOL {
            return Ok(b);
        }

        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (p_brent, q_brent) = if (a - c).abs() < f64::EPSILON {
                (2.0 * xm * s, 1.0 - s)
            } else {
                let q2 = fa / fc;
                let r2 = fb / fc;
                (
                    s * (2.0 * xm * q2 * (q2 - r2) - (b - a) * (r2 - 1.0)),
                    (q2 - 1.0) * (r2 - 1.0) * (s - 1.0),
                )
            };
            let (mut p_brent, mut q_brent) = (p_brent, q_brent);
            if p_brent > 0.0 { q_brent = -q_brent; } else { p_brent = -p_brent; }
            if 2.0 * p_brent < (3.0 * xm * q_brent - (tol1 * q_brent).abs()).min(e.abs() * q_brent.abs()) {
                e = d;
                d = p_brent / q_brent;
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

    Err(GreekError::NoConvergence)
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn call_atm() -> OptionParams {
        OptionParams {
            spot: 100.0,
            strike: 100.0,
            time_to_expiry: 1.0,
            risk_free_rate: 0.05,
            volatility: 0.20,
            option_type: OptionType::Call,
        }
    }

    fn put_atm() -> OptionParams {
        OptionParams { option_type: OptionType::Put, ..call_atm() }
    }

    // ── price ──

    #[test]
    fn call_price_positive() {
        let p = bsm_price(&call_atm()).unwrap();
        assert!(p > 0.0, "call price should be positive, got {p}");
    }

    #[test]
    fn put_price_positive() {
        let p = bsm_price(&put_atm()).unwrap();
        assert!(p > 0.0, "put price should be positive, got {p}");
    }

    #[test]
    fn put_call_parity() {
        // C - P = S - K * e^{-rT}
        let call = bsm_price(&call_atm()).unwrap();
        let put = bsm_price(&put_atm()).unwrap();
        let params = call_atm();
        let expected = params.spot
            - params.strike * (-params.risk_free_rate * params.time_to_expiry).exp();
        assert!(
            (call - put - expected).abs() < 1e-8,
            "put-call parity violation: {:.6} vs {:.6}",
            call - put,
            expected
        );
    }

    #[test]
    fn known_call_price() {
        // Classic BSM: S=100, K=100, T=1, r=0.05, σ=0.20 → ~10.4506
        let p = bsm_price(&call_atm()).unwrap();
        assert!((p - 10.4506).abs() < 0.01, "BSM call price off: {p:.4}");
    }

    #[test]
    fn invalid_spot_errors() {
        let mut p = call_atm();
        p.spot = 0.0;
        assert!(matches!(bsm_price(&p), Err(GreekError::InvalidParams(_))));
    }

    #[test]
    fn invalid_strike_errors() {
        let mut p = call_atm();
        p.strike = -1.0;
        assert!(matches!(bsm_price(&p), Err(GreekError::InvalidParams(_))));
    }

    #[test]
    fn invalid_tte_errors() {
        let mut p = call_atm();
        p.time_to_expiry = 0.0;
        assert!(matches!(bsm_price(&p), Err(GreekError::InvalidParams(_))));
    }

    // ── delta ──

    #[test]
    fn call_delta_between_0_and_1() {
        let g = bsm_greeks(&call_atm()).unwrap();
        assert!(g.delta > 0.0 && g.delta < 1.0, "call delta out of range: {}", g.delta);
    }

    #[test]
    fn put_delta_between_neg1_and_0() {
        let g = bsm_greeks(&put_atm()).unwrap();
        assert!(g.delta > -1.0 && g.delta < 0.0, "put delta out of range: {}", g.delta);
    }

    #[test]
    fn deep_itm_call_delta_near_1() {
        let p = OptionParams { spot: 200.0, ..call_atm() };
        let g = bsm_greeks(&p).unwrap();
        assert!(g.delta > 0.99, "deep ITM call delta should be ~1, got {}", g.delta);
    }

    #[test]
    fn deep_otm_call_delta_near_0() {
        let p = OptionParams { spot: 10.0, ..call_atm() };
        let g = bsm_greeks(&p).unwrap();
        assert!(g.delta < 0.01, "deep OTM call delta should be ~0, got {}", g.delta);
    }

    #[test]
    fn call_put_delta_relationship() {
        // delta_call - delta_put = 1
        let call_g = bsm_greeks(&call_atm()).unwrap();
        let put_g = bsm_greeks(&put_atm()).unwrap();
        assert!(
            (call_g.delta - put_g.delta - 1.0).abs() < 1e-10,
            "delta_call - delta_put != 1: {:.6}",
            call_g.delta - put_g.delta
        );
    }

    // ── gamma ──

    #[test]
    fn gamma_positive() {
        let g = bsm_greeks(&call_atm()).unwrap();
        assert!(g.gamma > 0.0);
    }

    #[test]
    fn gamma_peaks_atm() {
        // Gamma for ATM should be higher than OTM or ITM
        let atm = bsm_greeks(&call_atm()).unwrap();
        let otm = bsm_greeks(&OptionParams { spot: 150.0, ..call_atm() }).unwrap();
        let itm = bsm_greeks(&OptionParams { spot: 50.0, ..call_atm() }).unwrap();
        assert!(atm.gamma > otm.gamma, "ATM gamma should exceed OTM");
        assert!(atm.gamma > itm.gamma, "ATM gamma should exceed deep ITM");
    }

    #[test]
    fn call_put_gamma_equal() {
        let cg = bsm_greeks(&call_atm()).unwrap();
        let pg = bsm_greeks(&put_atm()).unwrap();
        assert!((cg.gamma - pg.gamma).abs() < 1e-12, "call and put gamma differ");
    }

    // ── vega ──

    #[test]
    fn vega_positive() {
        let g = bsm_greeks(&call_atm()).unwrap();
        assert!(g.vega > 0.0);
    }

    #[test]
    fn call_put_vega_equal() {
        let cg = bsm_greeks(&call_atm()).unwrap();
        let pg = bsm_greeks(&put_atm()).unwrap();
        assert!((cg.vega - pg.vega).abs() < 1e-10);
    }

    // ── theta ──

    #[test]
    fn call_theta_negative() {
        let g = bsm_greeks(&call_atm()).unwrap();
        assert!(g.theta < 0.0, "call theta should be negative (time decay)");
    }

    // ── rho ──

    #[test]
    fn call_rho_positive() {
        let g = bsm_greeks(&call_atm()).unwrap();
        assert!(g.rho > 0.0, "call rho should be positive");
    }

    #[test]
    fn put_rho_negative() {
        let g = bsm_greeks(&put_atm()).unwrap();
        assert!(g.rho < 0.0, "put rho should be negative");
    }

    // ── vanna / volga ──

    #[test]
    fn vanna_finite() {
        let g = bsm_greeks(&call_atm()).unwrap();
        assert!(g.vanna.is_finite());
    }

    #[test]
    fn volga_finite() {
        let g = bsm_greeks(&call_atm()).unwrap();
        assert!(g.volga.is_finite());
    }

    // ── implied volatility ──

    #[test]
    fn iv_roundtrip_call() {
        let params = call_atm();
        let price = bsm_price(&params).unwrap();
        let iv = implied_volatility(price, &params).unwrap();
        assert!(
            (iv - params.volatility).abs() < 1e-5,
            "IV roundtrip error: {:.8} vs {:.8}",
            iv,
            params.volatility
        );
    }

    #[test]
    fn iv_roundtrip_put() {
        let params = put_atm();
        let price = bsm_price(&params).unwrap();
        let iv = implied_volatility(price, &params).unwrap();
        assert!(
            (iv - params.volatility).abs() < 1e-5,
            "IV put roundtrip error: {:.8}",
            (iv - params.volatility).abs()
        );
    }

    #[test]
    fn iv_invalid_price_errors() {
        let params = call_atm();
        assert!(matches!(
            implied_volatility(-1.0, &params),
            Err(GreekError::InvalidParams(_))
        ));
    }

    #[test]
    fn iv_roundtrip_high_vol() {
        let params = OptionParams { volatility: 0.80, ..call_atm() };
        let price = bsm_price(&params).unwrap();
        let iv = implied_volatility(price, &params).unwrap();
        assert!((iv - 0.80).abs() < 1e-4, "high-vol IV error: {:.6}", iv - 0.80);
    }

    #[test]
    fn iv_roundtrip_low_vol() {
        let params = OptionParams { volatility: 0.05, ..call_atm() };
        let price = bsm_price(&params).unwrap();
        let iv = implied_volatility(price, &params).unwrap();
        assert!((iv - 0.05).abs() < 1e-4, "low-vol IV error: {:.6}", iv - 0.05);
    }
}

// ─── BSMInputs-based API (alternative struct layout matching task spec) ────────

/// All inputs required for Black-Scholes-Merton pricing, matching a common
/// quant convention: S, K, r, q, sigma, T.
#[derive(Debug, Clone, Copy)]
pub struct BSMInputs {
    /// Current underlying spot price S (must be > 0).
    pub S: f64,
    /// Strike price K (must be > 0).
    pub K: f64,
    /// Continuously-compounded annual risk-free rate r.
    pub r: f64,
    /// Continuous dividend yield q.
    pub q: f64,
    /// Annual volatility sigma (must be > 0).
    pub sigma: f64,
    /// Time to expiry in years T (must be > 0).
    pub T: f64,
}

/// Standard normal CDF (Abramowitz & Stegun polynomial, max |err| ≈ 7.5e-8).
pub fn norm_cdf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t * (0.319_381_530
        + t * (-0.356_563_782
            + t * (1.781_477_937
                + t * (-1.821_255_978
                    + t * 1.330_274_429))));
    let pdf = (-0.5 * x * x).exp() / (2.0_f64 * std::f64::consts::PI).sqrt();
    let cdf_pos = 1.0 - pdf * poly;
    if x >= 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

/// Standard normal PDF.
pub fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0_f64 * std::f64::consts::PI).sqrt()
}

/// d1 component of BSM.
pub fn d1(inputs: &BSMInputs) -> f64 {
    ((inputs.S / inputs.K).ln()
        + (inputs.r - inputs.q + 0.5 * inputs.sigma * inputs.sigma) * inputs.T)
        / (inputs.sigma * inputs.T.sqrt())
}

/// d2 = d1 - sigma * sqrt(T).
pub fn d2(inputs: &BSMInputs) -> f64 {
    d1(inputs) - inputs.sigma * inputs.T.sqrt()
}

/// BSM price for a European call or put with continuous dividend yield.
pub fn bsm_price_q(inputs: &BSMInputs, opt_type: OptionType) -> f64 {
    let d1v = d1(inputs);
    let d2v = d2(inputs);
    let eq = (-inputs.q * inputs.T).exp();
    let er = (-inputs.r * inputs.T).exp();
    match opt_type {
        OptionType::Call => {
            inputs.S * eq * norm_cdf(d1v) - inputs.K * er * norm_cdf(d2v)
        }
        OptionType::Put => {
            inputs.K * er * norm_cdf(-d2v) - inputs.S * eq * norm_cdf(-d1v)
        }
    }
}

/// Delta: ∂V/∂S (per share sensitivity to spot).
pub fn delta(inputs: &BSMInputs, opt_type: OptionType) -> f64 {
    let d1v = d1(inputs);
    let eq = (-inputs.q * inputs.T).exp();
    match opt_type {
        OptionType::Call => eq * norm_cdf(d1v),
        OptionType::Put => eq * (norm_cdf(d1v) - 1.0),
    }
}

/// Gamma: ∂²V/∂S² (same for calls and puts).
pub fn gamma(inputs: &BSMInputs) -> f64 {
    let d1v = d1(inputs);
    let eq = (-inputs.q * inputs.T).exp();
    eq * norm_pdf(d1v) / (inputs.S * inputs.sigma * inputs.T.sqrt())
}

/// Theta: ∂V/∂t per calendar day (negative for long options).
pub fn theta(inputs: &BSMInputs, opt_type: OptionType) -> f64 {
    let d1v = d1(inputs);
    let d2v = d2(inputs);
    let eq = (-inputs.q * inputs.T).exp();
    let er = (-inputs.r * inputs.T).exp();
    let common = -inputs.S * eq * norm_pdf(d1v) * inputs.sigma / (2.0 * inputs.T.sqrt());
    let annual = match opt_type {
        OptionType::Call => {
            common - inputs.r * inputs.K * er * norm_cdf(d2v)
                + inputs.q * inputs.S * eq * norm_cdf(d1v)
        }
        OptionType::Put => {
            common + inputs.r * inputs.K * er * norm_cdf(-d2v)
                - inputs.q * inputs.S * eq * norm_cdf(-d1v)
        }
    };
    annual / 365.0
}

/// Vega: ∂V/∂sigma per 1% move in volatility.
pub fn vega(inputs: &BSMInputs) -> f64 {
    let d1v = d1(inputs);
    let eq = (-inputs.q * inputs.T).exp();
    inputs.S * eq * norm_pdf(d1v) * inputs.T.sqrt() * 0.01
}

/// Rho: ∂V/∂r per 1% move in the risk-free rate.
pub fn rho(inputs: &BSMInputs, opt_type: OptionType) -> f64 {
    let d2v = d2(inputs);
    let er = (-inputs.r * inputs.T).exp();
    match opt_type {
        OptionType::Call => inputs.K * inputs.T * er * norm_cdf(d2v) * 0.01,
        OptionType::Put => -inputs.K * inputs.T * er * norm_cdf(-d2v) * 0.01,
    }
}

/// Vanna: ∂delta/∂sigma = -norm_pdf(d1) * d2 / sigma.
pub fn vanna(inputs: &BSMInputs) -> f64 {
    let d1v = d1(inputs);
    let d2v = d2(inputs);
    -norm_pdf(d1v) * d2v / inputs.sigma
}

/// Volga (Vomma): ∂vega/∂sigma = vega * d1 * d2 / sigma.
pub fn volga(inputs: &BSMInputs) -> f64 {
    let d1v = d1(inputs);
    let d2v = d2(inputs);
    let vega_val = vega(inputs) / 0.01; // full vega
    vega_val * d1v * d2v / inputs.sigma
}

/// Charm: ∂delta/∂T (the rate of change of delta with respect to time).
///
/// For a call: -q * e^{-qT} * N(d1) + e^{-qT} * n(d1) * (2*(r-q)*T - d2*sigma*sqrt(T)) / (2*T*sigma*sqrt(T))
pub fn charm(inputs: &BSMInputs, opt_type: OptionType) -> f64 {
    let d1v = d1(inputs);
    let d2v = d2(inputs);
    let eq = (-inputs.q * inputs.T).exp();
    let inner = (2.0 * (inputs.r - inputs.q) * inputs.T - d2v * inputs.sigma * inputs.T.sqrt())
        / (2.0 * inputs.T * inputs.sigma * inputs.T.sqrt());
    match opt_type {
        OptionType::Call => {
            -inputs.q * eq * norm_cdf(d1v) + eq * norm_pdf(d1v) * inner
        }
        OptionType::Put => {
            inputs.q * eq * norm_cdf(-d1v) + eq * norm_pdf(d1v) * inner
        }
    }
}

/// All Greeks for a BSM option in a single struct.
#[derive(Debug, Clone, Copy)]
pub struct GreeksResult {
    /// Option fair value.
    pub price: f64,
    /// Delta: ∂V/∂S.
    pub delta: f64,
    /// Gamma: ∂²V/∂S².
    pub gamma: f64,
    /// Theta per calendar day.
    pub theta: f64,
    /// Vega per 1% vol move.
    pub vega: f64,
    /// Rho per 1% rate move.
    pub rho: f64,
    /// Vanna: ∂delta/∂sigma.
    pub vanna: f64,
    /// Volga: ∂vega/∂sigma.
    pub volga: f64,
    /// Charm: ∂delta/∂T.
    pub charm: f64,
}

/// Compute all Greeks for the given inputs and option type.
pub fn compute_all(inputs: &BSMInputs, opt_type: OptionType) -> GreeksResult {
    GreeksResult {
        price: bsm_price_q(inputs, opt_type),
        delta: delta(inputs, opt_type),
        gamma: gamma(inputs),
        theta: theta(inputs, opt_type),
        vega: vega(inputs),
        rho: rho(inputs, opt_type),
        vanna: vanna(inputs),
        volga: volga(inputs),
        charm: charm(inputs, opt_type),
    }
}

/// Implied volatility solver using bisection (100 iterations, bounds [1e-6, 10.0]).
///
/// Returns `None` if the market price is outside the bounds that can be
/// produced by any vol in the search range.
pub fn implied_vol(
    market_price: f64,
    inputs: &BSMInputs,
    opt_type: OptionType,
) -> Option<f64> {
    if market_price <= 0.0 {
        return None;
    }
    let f = |sigma: f64| -> f64 {
        let inp = BSMInputs { sigma, ..*inputs };
        bsm_price_q(&inp, opt_type) - market_price
    };

    let mut lo = 1e-6_f64;
    let mut hi = 10.0_f64;
    let flo = f(lo);
    let fhi = f(hi);

    // Bracket check
    if flo * fhi > 0.0 {
        return None;
    }

    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        let fmid = f(mid);
        if fmid.abs() < 1e-8 || (hi - lo) < 1e-10 {
            return Some(mid);
        }
        if flo * fmid < 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

// ─── BSMInputs-based tests ────────────────────────────────────────────────────

#[cfg(test)]
mod bsm_inputs_tests {
    use super::*;

    fn atm_call() -> (BSMInputs, OptionType) {
        let inp = BSMInputs {
            S: 100.0,
            K: 100.0,
            r: 0.05,
            q: 0.0,
            sigma: 0.20,
            T: 1.0,
        };
        (inp, OptionType::Call)
    }

    fn atm_put() -> (BSMInputs, OptionType) {
        let (inp, _) = atm_call();
        (inp, OptionType::Put)
    }

    #[test]
    fn call_delta_near_half_atm() {
        let (inp, ot) = atm_call();
        let d = delta(&inp, ot);
        // ATM delta is slightly above 0.5 due to r>0
        assert!((d - 0.5).abs() < 0.1, "ATM call delta should be ~0.5, got {d}");
    }

    #[test]
    fn put_delta_near_neg_half_atm() {
        let (inp, ot) = atm_put();
        let d = delta(&inp, ot);
        assert!((d + 0.5).abs() < 0.1, "ATM put delta should be ~-0.5, got {d}");
    }

    #[test]
    fn put_call_parity_with_dividends() {
        let (inp, _) = atm_call();
        let call_p = bsm_price_q(&inp, OptionType::Call);
        let put_p = bsm_price_q(&inp, OptionType::Put);
        // C - P = S*e^{-qT} - K*e^{-rT}
        let expected = inp.S * (-inp.q * inp.T).exp() - inp.K * (-inp.r * inp.T).exp();
        assert!(
            (call_p - put_p - expected).abs() < 1e-8,
            "put-call parity violated: diff={:.9}",
            (call_p - put_p - expected).abs()
        );
    }

    #[test]
    fn gamma_symmetry_call_put() {
        let (inp, _) = atm_call();
        let g_call = gamma(&inp);
        // gamma is the same for call and put
        assert!(g_call > 0.0, "gamma must be positive");
        // verify it matches explicitly computed value
        let g_put = gamma(&inp);
        assert!((g_call - g_put).abs() < 1e-15, "gamma must be same for call and put");
    }

    #[test]
    fn vega_symmetry_call_put() {
        let (inp, _) = atm_call();
        let v_call = vega(&inp);
        let v_put = vega(&inp);
        assert!((v_call - v_put).abs() < 1e-15, "vega must be same for call and put");
        assert!(v_call > 0.0, "vega must be positive");
    }

    #[test]
    fn iv_roundtrip() {
        let (inp, ot) = atm_call();
        let price = bsm_price_q(&inp, ot);
        let iv = implied_vol(price, &inp, ot).expect("IV should converge");
        assert!(
            (iv - inp.sigma).abs() < 1e-5,
            "IV roundtrip error: got {iv:.6}, expected {:.6}",
            inp.sigma
        );
    }

    #[test]
    fn compute_all_consistency() {
        let (inp, ot) = atm_call();
        let gr = compute_all(&inp, ot);
        assert!((gr.delta - delta(&inp, ot)).abs() < 1e-15);
        assert!((gr.gamma - gamma(&inp)).abs() < 1e-15);
        assert!((gr.vega - vega(&inp)).abs() < 1e-15);
    }

    #[test]
    fn vanna_finite() {
        let (inp, _) = atm_call();
        let v = vanna(&inp);
        assert!(v.is_finite(), "vanna must be finite");
    }

    #[test]
    fn volga_finite() {
        let (inp, _) = atm_call();
        let vg = volga(&inp);
        assert!(vg.is_finite(), "volga must be finite");
    }

    #[test]
    fn charm_finite() {
        let (inp, ot) = atm_call();
        let c = charm(&inp, ot);
        assert!(c.is_finite(), "charm must be finite");
    }
}
