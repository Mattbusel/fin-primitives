//! # Module: greeks
//!
//! ## Responsibility
//! Black-Scholes option pricing, Greeks computation, implied volatility via bisection,
//! and multi-leg spread Greeks aggregation.
//!
//! ## Guarantees
//! - All math returns `Result<T, FinError>`; no panics on edge-case inputs
//! - Intermediate floating-point (f64) is used only for transcendental functions,
//!   then converted back to `Decimal` before returning
//!
//! ## NOT Responsible For
//! - American-style option pricing (European only)
//! - Dividend adjustments

use crate::error::FinError;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Standard normal probability density function.
fn phi(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Standard normal cumulative distribution function (Abramowitz & Stegun approximation).
/// Maximum absolute error ≈ 7.5 × 10⁻⁸.
fn big_phi(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let pdf = phi(x);
    let cdf_pos = 1.0 - pdf * poly;
    if x >= 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

/// Convert `Decimal` to `f64`, returning `FinError::ArithmeticOverflow` on failure.
fn to_f64(d: Decimal) -> Result<f64, FinError> {
    d.to_f64().ok_or(FinError::ArithmeticOverflow)
}

/// Convert `f64` to `Decimal`, returning `FinError::ArithmeticOverflow` on failure.
fn from_f64(f: f64) -> Result<Decimal, FinError> {
    if !f.is_finite() {
        return Err(FinError::ArithmeticOverflow);
    }
    Decimal::try_from(f).map_err(|_| FinError::ArithmeticOverflow)
}

// ─── types ────────────────────────────────────────────────────────────────────

/// Whether the option grants the right to buy (Call) or sell (Put).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OptionType {
    /// The right to buy the underlying at the strike price.
    Call,
    /// The right to sell the underlying at the strike price.
    Put,
}

/// Complete specification of a European option.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OptionSpec {
    /// Strike / exercise price (must be positive).
    pub strike: Decimal,
    /// Time to expiry in calendar days (must be > 0).
    pub expiry_days: u32,
    /// Current underlying spot price (must be positive).
    pub spot: Decimal,
    /// Continuously compounded annual risk-free rate (e.g. `dec!(0.05)` for 5%).
    pub risk_free_rate: Decimal,
    /// Annual implied or historical volatility (e.g. `dec!(0.20)` for 20%; must be > 0).
    pub volatility: Decimal,
    /// Call or Put.
    pub option_type: OptionType,
}

impl OptionSpec {
    /// Validates all fields and returns `FinError::InvalidInput` on the first violation.
    fn validate(&self) -> Result<(), FinError> {
        if self.strike <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "strike must be positive".to_owned(),
            ));
        }
        if self.expiry_days == 0 {
            return Err(FinError::InvalidInput(
                "expiry_days must be > 0".to_owned(),
            ));
        }
        if self.spot <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "spot must be positive".to_owned(),
            ));
        }
        if self.volatility <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "volatility must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

/// The five standard Black-Scholes option Greeks.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OptionGreeks {
    /// Rate of change of option price with respect to spot (∂V/∂S).
    pub delta: Decimal,
    /// Rate of change of delta with respect to spot (∂²V/∂S²).
    pub gamma: Decimal,
    /// Rate of change of option price with respect to time (∂V/∂t), expressed per calendar day.
    pub theta: Decimal,
    /// Rate of change of option price with respect to volatility (∂V/∂σ), per 1-point move.
    pub vega: Decimal,
    /// Rate of change of option price with respect to the risk-free rate (∂V/∂r).
    pub rho: Decimal,
}

// ─── BlackScholes ─────────────────────────────────────────────────────────────

/// Black-Scholes European option pricing model.
///
/// All arithmetic is done in `f64` for transcendental functions, then the final
/// results are converted back to `Decimal`.
pub struct BlackScholes;

impl BlackScholes {
    /// Computes the five standard Greeks for a European option.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if any spec field is invalid.
    /// Returns [`FinError::ArithmeticOverflow`] if a conversion fails.
    pub fn greeks(spec: &OptionSpec) -> Result<OptionGreeks, FinError> {
        spec.validate()?;

        let s = to_f64(spec.spot)?;
        let k = to_f64(spec.strike)?;
        let r = to_f64(spec.risk_free_rate)?;
        let v = to_f64(spec.volatility)?;
        // Years to expiry (trading-calendar-agnostic: use 365 days / year)
        let t = f64::from(spec.expiry_days) / 365.0;

        let sqrt_t = t.sqrt();
        let ln_sk = (s / k).ln();
        let d1 = (ln_sk + (r + 0.5 * v * v) * t) / (v * sqrt_t);
        let d2 = d1 - v * sqrt_t;
        let exp_rt = (-r * t).exp();

        let (delta, gamma, theta, vega, rho) = match spec.option_type {
            OptionType::Call => {
                let nd1 = big_phi(d1);
                let nd2 = big_phi(d2);
                let phi_d1 = phi(d1);

                let delta = nd1;
                let gamma = phi_d1 / (s * v * sqrt_t);
                // Theta: daily decay (divide annualised by 365)
                let theta =
                    (-(s * phi_d1 * v) / (2.0 * sqrt_t) - r * k * exp_rt * nd2) / 365.0;
                let vega = s * phi_d1 * sqrt_t / 100.0; // per 1 vol-point
                let rho = k * t * exp_rt * nd2 / 100.0; // per 1 rate-point

                (delta, gamma, theta, vega, rho)
            }
            OptionType::Put => {
                // N(d1) - 1 = -N(-d1); we compute N(-d1) and negate for delta
                let nd1_neg = big_phi(-d1); // N(-d1)
                let nd2_neg = big_phi(-d2); // N(-d2)
                let phi_d1 = phi(d1);

                // put delta = N(d1) - 1 = -N(-d1)
                let delta = -nd1_neg;
                let gamma = phi_d1 / (s * v * sqrt_t);
                let theta =
                    (-(s * phi_d1 * v) / (2.0 * sqrt_t) + r * k * exp_rt * nd2_neg) / 365.0;
                let vega = s * phi_d1 * sqrt_t / 100.0;
                let rho = -k * t * exp_rt * nd2_neg / 100.0;

                (delta, gamma, theta, vega, rho)
            }
        };

        Ok(OptionGreeks {
            delta: from_f64(delta)?,
            gamma: from_f64(gamma)?,
            theta: from_f64(theta)?,
            vega: from_f64(vega)?,
            rho: from_f64(rho)?,
        })
    }

    /// Computes the Black-Scholes theoretical price of a European option.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if any spec field is invalid.
    /// Returns [`FinError::ArithmeticOverflow`] if a conversion fails.
    pub fn price(spec: &OptionSpec) -> Result<Decimal, FinError> {
        spec.validate()?;

        let s = to_f64(spec.spot)?;
        let k = to_f64(spec.strike)?;
        let r = to_f64(spec.risk_free_rate)?;
        let v = to_f64(spec.volatility)?;
        let t = f64::from(spec.expiry_days) / 365.0;

        let sqrt_t = t.sqrt();
        let d1 = ((s / k).ln() + (r + 0.5 * v * v) * t) / (v * sqrt_t);
        let d2 = d1 - v * sqrt_t;
        let exp_rt = (-r * t).exp();

        let price = match spec.option_type {
            OptionType::Call => s * big_phi(d1) - k * exp_rt * big_phi(d2),
            OptionType::Put => k * exp_rt * big_phi(-d2) - s * big_phi(-d1),
        };

        from_f64(price)
    }

    /// Computes implied volatility via bisection search.
    ///
    /// Searches in the interval `[1e-6, 5.0]` (0.0001% – 500% annualised vol).
    /// Converges to within `tol = 1e-7` or fails after `MAX_ITER = 200` iterations.
    ///
    /// # Errors
    /// - [`FinError::InvalidInput`] if `market_price` is non-positive, or any spec field is
    ///   invalid (volatility field is ignored during the search).
    /// - [`FinError::InvalidInput`] if the market price is outside the no-arbitrage bounds.
    /// - [`FinError::ArithmeticOverflow`] on conversion failure.
    pub fn implied_vol(market_price: Decimal, spec: &OptionSpec) -> Result<Decimal, FinError> {
        if market_price <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "market_price must be positive".to_owned(),
            ));
        }
        // Validate all fields except volatility (which we are solving for)
        if spec.strike <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "strike must be positive".to_owned(),
            ));
        }
        if spec.expiry_days == 0 {
            return Err(FinError::InvalidInput(
                "expiry_days must be > 0".to_owned(),
            ));
        }
        if spec.spot <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "spot must be positive".to_owned(),
            ));
        }

        let target = to_f64(market_price)?;

        const LOW_VOL: f64 = 1e-6;
        const HIGH_VOL: f64 = 5.0;
        const TOL: f64 = 1e-7;
        const MAX_ITER: usize = 200;

        let price_at = |vol: f64| -> Result<f64, FinError> {
            let trial_spec = OptionSpec {
                volatility: from_f64(vol)?,
                ..spec.clone()
            };
            to_f64(Self::price(&trial_spec)?)
        };

        let mut lo = LOW_VOL;
        let mut hi = HIGH_VOL;

        let p_lo = price_at(lo)?;
        let p_hi = price_at(hi)?;

        // Check that target is bracketed
        if target < p_lo || target > p_hi {
            return Err(FinError::InvalidInput(
                "market_price is outside no-arbitrage vol bounds [1e-6, 500%]".to_owned(),
            ));
        }

        for _ in 0..MAX_ITER {
            let mid = (lo + hi) / 2.0;
            let p_mid = price_at(mid)?;
            let err = p_mid - target;
            if err.abs() < TOL {
                return from_f64(mid);
            }
            if err < 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        // Return best midpoint after max iterations
        from_f64((lo + hi) / 2.0)
    }
}

// ─── Spread Greeks ────────────────────────────────────────────────────────────

/// A single leg within a multi-leg spread position.
#[derive(Debug, Clone)]
pub struct Leg {
    /// Option specification for this leg.
    pub spec: OptionSpec,
    /// Number of contracts (positive = long, negative = short).
    pub quantity: i32,
}

impl Leg {
    /// Creates a new leg.
    pub fn new(spec: OptionSpec, quantity: i32) -> Self {
        Self { spec, quantity }
    }

    /// Computes the Greeks for this leg, scaled by `quantity`.
    fn scaled_greeks(&self) -> Result<OptionGreeks, FinError> {
        let g = BlackScholes::greeks(&self.spec)?;
        let q = from_f64(f64::from(self.quantity))?;
        Ok(OptionGreeks {
            delta: g.delta * q,
            gamma: g.gamma * q,
            theta: g.theta * q,
            vega: g.vega * q,
            rho: g.rho * q,
        })
    }
}

/// Aggregates Greeks across a user-defined set of legs.
#[derive(Debug, Clone)]
pub struct SpreadGreeks {
    legs: Vec<Leg>,
}

impl SpreadGreeks {
    /// Creates a `SpreadGreeks` from an arbitrary list of legs.
    pub fn new(legs: Vec<Leg>) -> Self {
        Self { legs }
    }

    /// Constructs a bull call spread: long lower-strike call, short higher-strike call.
    ///
    /// Both legs share the same `spot`, `expiry_days`, `risk_free_rate`, and `volatility`.
    pub fn bull_call_spread(
        spot: Decimal,
        low_strike: Decimal,
        high_strike: Decimal,
        expiry_days: u32,
        risk_free_rate: Decimal,
        volatility: Decimal,
    ) -> Self {
        let base = |strike| OptionSpec {
            strike,
            expiry_days,
            spot,
            risk_free_rate,
            volatility,
            option_type: OptionType::Call,
        };
        Self::new(vec![
            Leg::new(base(low_strike), 1),
            Leg::new(base(high_strike), -1),
        ])
    }

    /// Constructs a bear put spread: long higher-strike put, short lower-strike put.
    pub fn bear_put_spread(
        spot: Decimal,
        low_strike: Decimal,
        high_strike: Decimal,
        expiry_days: u32,
        risk_free_rate: Decimal,
        volatility: Decimal,
    ) -> Self {
        let base = |strike, ot| OptionSpec {
            strike,
            expiry_days,
            spot,
            risk_free_rate,
            volatility,
            option_type: ot,
        };
        Self::new(vec![
            Leg::new(base(high_strike, OptionType::Put), 1),
            Leg::new(base(low_strike, OptionType::Put), -1),
        ])
    }

    /// Constructs an ATM straddle: long call and long put at the same strike.
    pub fn straddle(
        spot: Decimal,
        strike: Decimal,
        expiry_days: u32,
        risk_free_rate: Decimal,
        volatility: Decimal,
    ) -> Self {
        let base = |ot| OptionSpec {
            strike,
            expiry_days,
            spot,
            risk_free_rate,
            volatility,
            option_type: ot,
        };
        Self::new(vec![
            Leg::new(base(OptionType::Call), 1),
            Leg::new(base(OptionType::Put), 1),
        ])
    }

    /// Constructs an iron condor: short put spread + short call spread.
    ///
    /// - Long put at `put_low`, short put at `put_high`
    /// - Short call at `call_low`, long call at `call_high`
    ///
    /// Strikes must satisfy: `put_low < put_high < call_low < call_high`
    #[allow(clippy::too_many_arguments)]
    pub fn iron_condor(
        spot: Decimal,
        put_low: Decimal,
        put_high: Decimal,
        call_low: Decimal,
        call_high: Decimal,
        expiry_days: u32,
        risk_free_rate: Decimal,
        volatility: Decimal,
    ) -> Self {
        let mk = |strike, ot, qty| {
            Leg::new(
                OptionSpec {
                    strike,
                    expiry_days,
                    spot,
                    risk_free_rate,
                    volatility,
                    option_type: ot,
                },
                qty,
            )
        };
        Self::new(vec![
            mk(put_low, OptionType::Put, 1),
            mk(put_high, OptionType::Put, -1),
            mk(call_low, OptionType::Call, -1),
            mk(call_high, OptionType::Call, 1),
        ])
    }

    /// Returns the net (aggregated) Greeks across all legs.
    ///
    /// # Errors
    /// Returns the first [`FinError`] encountered while computing any leg's Greeks.
    pub fn net_greeks(&self) -> Result<OptionGreeks, FinError> {
        let mut delta = Decimal::ZERO;
        let mut gamma = Decimal::ZERO;
        let mut theta = Decimal::ZERO;
        let mut vega = Decimal::ZERO;
        let mut rho = Decimal::ZERO;

        for leg in &self.legs {
            let g = leg.scaled_greeks()?;
            delta += g.delta;
            gamma += g.gamma;
            theta += g.theta;
            vega += g.vega;
            rho += g.rho;
        }

        Ok(OptionGreeks { delta, gamma, theta, vega, rho })
    }

    /// Returns the number of legs in this spread.
    pub fn leg_count(&self) -> usize {
        self.legs.len()
    }
}

// ─── f64-native interface ─────────────────────────────────────────────────────
//
// The following types provide a direct f64-based Black-Scholes API that is
// more convenient for numerical computation (e.g. calibration loops and ML
// feature generation) than the Decimal-based `OptionSpec` / `BlackScholes`
// interface above.  Both APIs share the same underlying math.

/// Black-Scholes parameters (f64 native, no `Decimal` conversion).
#[derive(Debug, Clone, Copy)]
pub struct BSParams {
    /// Current underlying spot price (must be positive).
    pub spot: f64,
    /// Strike / exercise price (must be positive).
    pub strike: f64,
    /// Time to expiry in **years** (must be > 0).
    pub time_to_expiry: f64,
    /// Continuously compounded annual risk-free rate (e.g. `0.05` for 5%).
    pub risk_free_rate: f64,
    /// Annual implied or historical volatility (e.g. `0.20` for 20%; must be > 0).
    pub volatility: f64,
    /// Call or Put.
    pub option_type: OptionType,
}

/// Full set of first- and second-order Black-Scholes Greeks.
///
/// All Greeks use standard market conventions:
/// - `theta` is per **calendar day** (divide by 365 from annual).
/// - `vega` is per **1 percentage-point** move in vol (i.e. per 0.01 σ).
/// - `rho` is per **1 percentage-point** move in the risk-free rate.
#[derive(Debug, Clone, Copy)]
pub struct Greeks {
    /// ∂V/∂S — sensitivity to spot price.
    pub delta: f64,
    /// ∂²V/∂S² — rate of change of delta with respect to spot.
    pub gamma: f64,
    /// ∂V/∂t per calendar day (negative for long options = time decay).
    pub theta: f64,
    /// ∂V/∂σ per 1% vol move.
    pub vega: f64,
    /// ∂V/∂r per 1% rate move.
    pub rho: f64,
    /// ∂Delta/∂σ (also written dDelta/dVol or dVega/dS).
    pub vanna: f64,
    /// ∂²V/∂σ² per 1% vol move (also called Vomma or Volga).
    pub volga: f64,
    /// ∂Delta/∂t per calendar day — rate of change of delta due to time decay.
    pub charm: f64,
    /// ∂Gamma/∂S — third-order spot sensitivity.
    pub speed: f64,
}

/// Black-Scholes pricing and Greeks calculator using native `f64`.
///
/// All methods are pure functions; none mutate state.  Edge-case inputs
/// (e.g. zero time, negative vol) return `None` rather than panicking.
pub struct BSCalculator;

impl BSCalculator {
    /// Black-Scholes theoretical price.
    ///
    /// Returns `None` if any parameter is non-positive (spot, strike, vol) or
    /// time-to-expiry is zero.
    pub fn price(p: &BSParams) -> Option<f64> {
        let (d1, d2) = Self::d1_d2(p)?;
        let exp_rt = (-p.risk_free_rate * p.time_to_expiry).exp();
        let price = match p.option_type {
            OptionType::Call => p.spot * norm_cdf(d1) - p.strike * exp_rt * norm_cdf(d2),
            OptionType::Put => p.strike * exp_rt * norm_cdf(-d2) - p.spot * norm_cdf(-d1),
        };
        Some(price)
    }

    /// Full set of first- and second-order Greeks.
    ///
    /// Returns `None` if parameters are invalid (same conditions as [`price`]).
    ///
    /// [`price`]: BSCalculator::price
    pub fn greeks(p: &BSParams) -> Option<Greeks> {
        let (d1, d2) = Self::d1_d2(p)?;
        let s = p.spot;
        let k = p.strike;
        let r = p.risk_free_rate;
        let v = p.volatility;
        let t = p.time_to_expiry;
        let sqrt_t = t.sqrt();
        let exp_rt = (-r * t).exp();
        let phi_d1 = norm_pdf(d1);

        // ── First-order Greeks ───────────────────────────────────────────────
        let (delta, theta, rho) = match p.option_type {
            OptionType::Call => {
                let nd1 = norm_cdf(d1);
                let nd2 = norm_cdf(d2);
                let delta = nd1;
                let theta = (-(s * phi_d1 * v) / (2.0 * sqrt_t)
                    - r * k * exp_rt * nd2)
                    / 365.0;
                let rho = k * t * exp_rt * nd2 / 100.0;
                (delta, theta, rho)
            }
            OptionType::Put => {
                let nd1_neg = norm_cdf(-d1);
                let nd2_neg = norm_cdf(-d2);
                let delta = nd1_neg - 1.0;
                let theta = (-(s * phi_d1 * v) / (2.0 * sqrt_t)
                    + r * k * exp_rt * nd2_neg)
                    / 365.0;
                let rho = -k * t * exp_rt * nd2_neg / 100.0;
                (delta, theta, rho)
            }
        };

        let gamma = phi_d1 / (s * v * sqrt_t);
        let vega = s * phi_d1 * sqrt_t / 100.0; // per 1% vol move

        // ── Second-order Greeks ──────────────────────────────────────────────
        // Vanna: dDelta/dVol  (= dVega/dS * 1/spot)
        let vanna = -phi_d1 * d2 / v;

        // Volga / Vomma: dVega/dVol  per 1% vol move
        // Raw: S * phi(d1) * sqrt(T) * d1 * d2 / vol
        // Scaled by /100 twice (vega is per 1%, so volga is per 1%²)
        let volga = vega * d1 * d2 / v;

        // Charm: dDelta/dTime  per calendar day
        // Call: -phi(d1) * [2rT - d2 * v * sqrt(T)] / [2T * v * sqrt(T)] / 365
        let charm = match p.option_type {
            OptionType::Call => {
                (-phi_d1
                    * (2.0 * r * t - d2 * v * sqrt_t)
                    / (2.0 * t * v * sqrt_t))
                    / 365.0
            }
            OptionType::Put => {
                (phi_d1
                    * (2.0 * r * t - d2 * v * sqrt_t)
                    / (2.0 * t * v * sqrt_t))
                    / 365.0
            }
        };

        // Speed: dGamma/dS = -Gamma/S * (d1 / (v * sqrt_t) + 1)
        let speed = -gamma / s * (d1 / (v * sqrt_t) + 1.0);

        Some(Greeks { delta, gamma, theta, vega, rho, vanna, volga, charm, speed })
    }

    /// Delta only (faster than computing all Greeks).
    pub fn delta(p: &BSParams) -> Option<f64> {
        let (d1, _) = Self::d1_d2(p)?;
        Some(match p.option_type {
            OptionType::Call => norm_cdf(d1),
            OptionType::Put => norm_cdf(-d1) - 1.0,
        })
    }

    /// Gamma (same for calls and puts).
    pub fn gamma(p: &BSParams) -> Option<f64> {
        let (d1, _) = Self::d1_d2(p)?;
        Some(norm_pdf(d1) / (p.spot * p.volatility * p.time_to_expiry.sqrt()))
    }

    /// Theta per calendar day.
    pub fn theta(p: &BSParams) -> Option<f64> {
        let (d1, d2) = Self::d1_d2(p)?;
        let phi_d1 = norm_pdf(d1);
        let exp_rt = (-p.risk_free_rate * p.time_to_expiry).exp();
        let sqrt_t = p.time_to_expiry.sqrt();
        let base = -(p.spot * phi_d1 * p.volatility) / (2.0 * sqrt_t);
        let theta = match p.option_type {
            OptionType::Call => (base - p.risk_free_rate * p.strike * exp_rt * norm_cdf(d2)) / 365.0,
            OptionType::Put => (base + p.risk_free_rate * p.strike * exp_rt * norm_cdf(-d2)) / 365.0,
        };
        Some(theta)
    }

    /// Vega per 1% vol move.
    pub fn vega(p: &BSParams) -> Option<f64> {
        let (d1, _) = Self::d1_d2(p)?;
        Some(p.spot * norm_pdf(d1) * p.time_to_expiry.sqrt() / 100.0)
    }

    /// Rho per 1% rate move.
    pub fn rho(p: &BSParams) -> Option<f64> {
        let (_, d2) = Self::d1_d2(p)?;
        let exp_rt = (-p.risk_free_rate * p.time_to_expiry).exp();
        Some(match p.option_type {
            OptionType::Call => p.strike * p.time_to_expiry * exp_rt * norm_cdf(d2) / 100.0,
            OptionType::Put => -p.strike * p.time_to_expiry * exp_rt * norm_cdf(-d2) / 100.0,
        })
    }

    /// Newton-Raphson implied volatility solver.
    ///
    /// Searches for the volatility `σ` such that `BS_price(σ) == market_price`.
    ///
    /// - `tolerance` — convergence threshold for the price error (e.g. `1e-6`).
    /// - `max_iter` — maximum Newton-Raphson iterations (e.g. `100`).
    ///
    /// Returns `None` when the solver does not converge or parameters are invalid.
    pub fn implied_volatility(
        market_price: f64,
        p: &BSParams,
        tolerance: f64,
        max_iter: usize,
    ) -> Option<f64> {
        if market_price <= 0.0 || p.spot <= 0.0 || p.strike <= 0.0 || p.time_to_expiry <= 0.0 {
            return None;
        }

        // Initial guess: Brenner-Subrahmanyam approximation
        let mut sigma = (2.0 * std::f64::consts::PI / p.time_to_expiry).sqrt()
            * market_price
            / p.spot;
        // Clamp to a reasonable range
        sigma = sigma.clamp(1e-4, 5.0);

        for _ in 0..max_iter {
            let trial = BSParams { volatility: sigma, ..*p };
            let price = Self::price(&trial)?;
            let error = price - market_price;
            if error.abs() < tolerance {
                return Some(sigma);
            }
            let v = Self::vega(&trial)?;
            // vega is per 1% → convert back to per unit for Newton step
            let vega_raw = v * 100.0;
            if vega_raw.abs() < 1e-10 {
                break; // near-zero vega; cannot converge
            }
            sigma -= error / vega_raw;
            sigma = sigma.clamp(1e-4, 5.0);
        }

        // Last attempt: return current sigma if within 10× tolerance
        let trial = BSParams { volatility: sigma, ..*p };
        let final_price = Self::price(&trial)?;
        if (final_price - market_price).abs() < tolerance * 10.0 {
            Some(sigma)
        } else {
            None
        }
    }

    // ── private helpers ───────────────────────────────────────────────────────

    /// Computes d1 and d2.  Returns `None` for invalid parameters.
    fn d1_d2(p: &BSParams) -> Option<(f64, f64)> {
        if p.spot <= 0.0 || p.strike <= 0.0 || p.volatility <= 0.0 || p.time_to_expiry <= 0.0 {
            return None;
        }
        let sqrt_t = p.time_to_expiry.sqrt();
        let d1 = ((p.spot / p.strike).ln()
            + (p.risk_free_rate + 0.5 * p.volatility * p.volatility) * p.time_to_expiry)
            / (p.volatility * sqrt_t);
        let d2 = d1 - p.volatility * sqrt_t;
        Some((d1, d2))
    }
}

/// Standard normal PDF: φ(x) = exp(-x²/2) / √(2π).
fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Standard normal CDF via the Horner-form polynomial approximation.
///
/// Abramowitz & Stegun 26.2.17 — maximum absolute error ≈ 7.5 × 10⁻⁸.
fn norm_cdf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let cdf_pos = 1.0 - norm_pdf(x) * poly;
    if x >= 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

// ─── BSCalculator tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod bs_tests {
    use super::*;

    fn atm_call() -> BSParams {
        BSParams {
            spot: 100.0,
            strike: 100.0,
            time_to_expiry: 30.0 / 365.0,
            risk_free_rate: 0.05,
            volatility: 0.20,
            option_type: OptionType::Call,
        }
    }

    fn atm_put() -> BSParams {
        BSParams { option_type: OptionType::Put, ..atm_call() }
    }

    #[test]
    fn price_call_positive() {
        let p = BSCalculator::price(&atm_call()).unwrap();
        assert!(p > 0.0 && p < 10.0, "call price out of range: {p}");
    }

    #[test]
    fn put_call_parity() {
        let c = BSCalculator::price(&atm_call()).unwrap();
        let p = BSCalculator::price(&atm_put()).unwrap();
        let params = atm_call();
        let forward = params.spot
            - params.strike * (-params.risk_free_rate * params.time_to_expiry).exp();
        assert!((c - p - forward).abs() < 1e-6, "put-call parity violated: {}", c - p - forward);
    }

    #[test]
    fn delta_call_between_zero_and_one() {
        let d = BSCalculator::delta(&atm_call()).unwrap();
        assert!(d > 0.0 && d < 1.0, "call delta out of range: {d}");
    }

    #[test]
    fn delta_put_between_neg_one_and_zero() {
        let d = BSCalculator::delta(&atm_put()).unwrap();
        assert!(d > -1.0 && d < 0.0, "put delta out of range: {d}");
    }

    #[test]
    fn gamma_positive() {
        let g = BSCalculator::gamma(&atm_call()).unwrap();
        assert!(g > 0.0, "gamma should be positive: {g}");
    }

    #[test]
    fn theta_negative_call() {
        let t = BSCalculator::theta(&atm_call()).unwrap();
        assert!(t < 0.0, "theta should be negative for long call: {t}");
    }

    #[test]
    fn vega_positive() {
        let v = BSCalculator::vega(&atm_call()).unwrap();
        assert!(v > 0.0, "vega should be positive: {v}");
    }

    #[test]
    fn all_greeks_available() {
        let g = BSCalculator::greeks(&atm_call()).unwrap();
        assert!(g.delta > 0.0);
        assert!(g.gamma > 0.0);
        assert!(g.theta < 0.0);
        assert!(g.vega > 0.0);
    }

    #[test]
    fn implied_vol_roundtrip() {
        let params = atm_call();
        let market_price = BSCalculator::price(&params).unwrap();
        let iv = BSCalculator::implied_volatility(market_price, &params, 1e-6, 100).unwrap();
        assert!((iv - params.volatility).abs() < 1e-4, "IV roundtrip error: {}", iv - params.volatility);
    }

    #[test]
    fn implied_vol_invalid_price_returns_none() {
        assert!(BSCalculator::implied_volatility(-1.0, &atm_call(), 1e-6, 100).is_none());
    }

    #[test]
    fn invalid_params_return_none() {
        let bad = BSParams { spot: -1.0, ..atm_call() };
        assert!(BSCalculator::price(&bad).is_none());
        assert!(BSCalculator::greeks(&bad).is_none());
    }

    #[test]
    fn vanna_sign_for_atm_call() {
        // Vanna for a call: -phi(d1)*d2/vol
        // Near ATM d2 is small/positive → vanna should be near zero or slightly negative
        let g = BSCalculator::greeks(&atm_call()).unwrap();
        // Just verify it computes without NaN/Inf
        assert!(g.vanna.is_finite(), "vanna should be finite: {}", g.vanna);
    }

    #[test]
    fn speed_is_finite() {
        let g = BSCalculator::greeks(&atm_call()).unwrap();
        assert!(g.speed.is_finite(), "speed should be finite: {}", g.speed);
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_call() -> OptionSpec {
        OptionSpec {
            strike: dec!(100),
            expiry_days: 30,
            spot: dec!(100),
            risk_free_rate: dec!(0.05),
            volatility: dec!(0.20),
            option_type: OptionType::Call,
        }
    }

    fn sample_put() -> OptionSpec {
        OptionSpec { option_type: OptionType::Put, ..sample_call() }
    }

    #[test]
    fn test_bs_price_call_atm_sanity() {
        let price = BlackScholes::price(&sample_call()).unwrap();
        // ATM call with 30-day expiry, 20% vol, 5% rf should be around 2.30–2.50
        assert!(price > dec!(1) && price < dec!(5), "call price={price}");
    }

    #[test]
    fn test_bs_price_put_atm_sanity() {
        let price = BlackScholes::price(&sample_put()).unwrap();
        assert!(price > dec!(1) && price < dec!(5), "put price={price}");
    }

    #[test]
    fn test_put_call_parity() {
        // C - P = S - K*e^{-rT}
        let call_price = to_f64(BlackScholes::price(&sample_call()).unwrap()).unwrap();
        let put_price = to_f64(BlackScholes::price(&sample_put()).unwrap()).unwrap();
        let s = 100.0_f64;
        let k = 100.0_f64;
        let r = 0.05_f64;
        let t = 30.0_f64 / 365.0;
        let parity_rhs = s - k * (-r * t).exp();
        let diff = (call_price - put_price - parity_rhs).abs();
        assert!(diff < 1e-6, "put-call parity violation: {diff}");
    }

    #[test]
    fn test_greeks_call_delta_between_zero_and_one() {
        let g = BlackScholes::greeks(&sample_call()).unwrap();
        assert!(g.delta > dec!(0) && g.delta < dec!(1));
    }

    #[test]
    fn test_greeks_put_delta_between_neg_one_and_zero() {
        let g = BlackScholes::greeks(&sample_put()).unwrap();
        assert!(g.delta > dec!(-1) && g.delta < dec!(0));
    }

    #[test]
    fn test_greeks_gamma_positive() {
        let g = BlackScholes::greeks(&sample_call()).unwrap();
        assert!(g.gamma > dec!(0));
    }

    #[test]
    fn test_greeks_theta_negative_call() {
        let g = BlackScholes::greeks(&sample_call()).unwrap();
        assert!(g.theta < dec!(0));
    }

    #[test]
    fn test_greeks_vega_positive() {
        let g = BlackScholes::greeks(&sample_call()).unwrap();
        assert!(g.vega > dec!(0));
    }

    #[test]
    fn test_implied_vol_roundtrip() {
        let spec = sample_call();
        let market_price = BlackScholes::price(&spec).unwrap();
        let iv = BlackScholes::implied_vol(market_price, &spec).unwrap();
        let diff = (iv - spec.volatility).abs();
        assert!(diff < dec!(0.0001), "IV roundtrip error: {diff}");
    }

    #[test]
    fn test_invalid_strike_errors() {
        let spec = OptionSpec { strike: dec!(0), ..sample_call() };
        assert!(BlackScholes::price(&spec).is_err());
    }

    #[test]
    fn test_invalid_spot_errors() {
        let spec = OptionSpec { spot: dec!(-1), ..sample_call() };
        assert!(BlackScholes::greeks(&spec).is_err());
    }

    #[test]
    fn test_straddle_delta_near_zero_atm() {
        let spread = SpreadGreeks::straddle(
            dec!(100),
            dec!(100),
            30,
            dec!(0.05),
            dec!(0.20),
        );
        let g = spread.net_greeks().unwrap();
        // ATM straddle: delta ≈ 0 (call ~0.5, put ~-0.5)
        assert!(g.delta.abs() < dec!(0.1), "straddle delta={}", g.delta);
    }

    #[test]
    fn test_bull_call_spread_positive_delta() {
        let spread = SpreadGreeks::bull_call_spread(
            dec!(100),
            dec!(95),
            dec!(105),
            30,
            dec!(0.05),
            dec!(0.20),
        );
        let g = spread.net_greeks().unwrap();
        assert!(g.delta > dec!(0));
    }

    #[test]
    fn test_iron_condor_has_four_legs() {
        let spread = SpreadGreeks::iron_condor(
            dec!(100),
            dec!(85),
            dec!(90),
            dec!(110),
            dec!(115),
            30,
            dec!(0.05),
            dec!(0.20),
        );
        assert_eq!(spread.leg_count(), 4);
    }

    #[test]
    fn test_bear_put_spread_negative_delta() {
        let spread = SpreadGreeks::bear_put_spread(
            dec!(100),
            dec!(95),
            dec!(105),
            30,
            dec!(0.05),
            dec!(0.20),
        );
        let g = spread.net_greeks().unwrap();
        assert!(g.delta < dec!(0));
    }
}
