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
                let nd1_neg = big_phi(-d1);
                let nd2_neg = big_phi(-d2);
                let phi_d1 = phi(d1);

                let delta = nd1_neg - 1.0;
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
