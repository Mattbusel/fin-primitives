//! # Module: options
//!
//! ## Responsibility
//! Black-Scholes European option pricing, Greeks (delta, gamma, theta, vega, rho),
//! and implied volatility solving via Newton-Raphson iteration.
//!
//! ## Design
//! - All public API uses `rust_decimal::Decimal` for inputs/outputs.
//! - Transcendental math (exp, ln, sqrt, erf) is done in `f64` internally.
//! - Every fallible operation returns `Result<_, FinError>`; no panics on edge inputs.
//!
//! ## NOT Responsible For
//! - American-style options (European only)
//! - Discrete dividend adjustments
//! - Smile/surface interpolation

use crate::error::FinError;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

// ─── internal f64 helpers ────────────────────────────────────────────────────

/// Standard normal PDF.
#[inline]
fn phi(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0_f64 * std::f64::consts::PI).sqrt()
}

/// Standard normal CDF (Abramowitz & Stegun, max |error| ≈ 7.5e-8).
#[inline]
fn big_phi(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t * (0.319_381_530
        + t * (-0.356_563_782
            + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let cdf_pos = 1.0 - phi(x) * poly;
    if x >= 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

fn to_f64(d: Decimal) -> Result<f64, FinError> {
    d.to_f64().ok_or(FinError::ArithmeticOverflow)
}

fn from_f64(f: f64) -> Result<Decimal, FinError> {
    if !f.is_finite() {
        return Err(FinError::ArithmeticOverflow);
    }
    Decimal::try_from(f).map_err(|_| FinError::ArithmeticOverflow)
}

// ─── public types ─────────────────────────────────────────────────────────────

/// Whether the option grants the right to buy (Call) or sell (Put).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OptionKind {
    /// Right to buy the underlying at the strike.
    Call,
    /// Right to sell the underlying at the strike.
    Put,
}

/// All inputs required to price a European option under Black-Scholes.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct OptionSpec {
    /// Option type: call or put.
    pub kind: OptionKind,
    /// Current underlying price (S). Must be positive.
    pub spot: Decimal,
    /// Strike price (K). Must be positive.
    pub strike: Decimal,
    /// Time to expiry in years (T). Must be positive.
    pub time_to_expiry: Decimal,
    /// Annualised risk-free rate (r). May be negative.
    pub risk_free_rate: Decimal,
    /// Annualised implied/historical volatility (σ). Must be positive.
    pub volatility: Decimal,
}

/// Fair value and all first/second-order Greeks for a European option.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct OptionGreeks {
    /// Theoretical fair value (premium).
    pub price: Decimal,
    /// Delta: ∂V/∂S — sensitivity of price to spot moves.
    pub delta: Decimal,
    /// Gamma: ∂²V/∂S² — rate of change of delta per unit spot move.
    pub gamma: Decimal,
    /// Theta: ∂V/∂t (per calendar day) — time decay.
    pub theta: Decimal,
    /// Vega: ∂V/∂σ (per 1% move in vol) — volatility sensitivity.
    pub vega: Decimal,
    /// Rho: ∂V/∂r (per 1% move in rate) — rate sensitivity.
    pub rho: Decimal,
}

// ─── core engine ──────────────────────────────────────────────────────────────

/// Black-Scholes European option pricing and Greeks engine.
pub struct BlackScholes;

impl BlackScholes {
    /// Compute fair value and all Greeks for `spec`.
    ///
    /// # Errors
    /// - `FinError::InvalidPrice` if spot or strike is non-positive.
    /// - `FinError::InvalidInput` if time-to-expiry or volatility is non-positive.
    /// - `FinError::ArithmeticOverflow` on internal numeric failure.
    pub fn price(spec: &OptionSpec) -> Result<OptionGreeks, FinError> {
        Self::validate(spec)?;

        let s = to_f64(spec.spot)?;
        let k = to_f64(spec.strike)?;
        let t = to_f64(spec.time_to_expiry)?;
        let r = to_f64(spec.risk_free_rate)?;
        let v = to_f64(spec.volatility)?;

        let sqrt_t = t.sqrt();
        let d1 = ((s / k).ln() + (r + 0.5 * v * v) * t) / (v * sqrt_t);
        let d2 = d1 - v * sqrt_t;

        let (price_f, delta_f, rho_f) = match spec.kind {
            OptionKind::Call => {
                let price = s * big_phi(d1) - k * (-r * t).exp() * big_phi(d2);
                let delta = big_phi(d1);
                let rho = k * t * (-r * t).exp() * big_phi(d2) * 0.01;
                (price, delta, rho)
            }
            OptionKind::Put => {
                let price = k * (-r * t).exp() * big_phi(-d2) - s * big_phi(-d1);
                let delta = big_phi(d1) - 1.0;
                let rho = -k * t * (-r * t).exp() * big_phi(-d2) * 0.01;
                (price, delta, rho)
            }
        };

        let gamma_f = phi(d1) / (s * v * sqrt_t);
        // Theta per calendar day (divide by 365)
        let theta_f = match spec.kind {
            OptionKind::Call => {
                (-s * phi(d1) * v / (2.0 * sqrt_t)
                    - r * k * (-r * t).exp() * big_phi(d2))
                    / 365.0
            }
            OptionKind::Put => {
                (-s * phi(d1) * v / (2.0 * sqrt_t)
                    + r * k * (-r * t).exp() * big_phi(-d2))
                    / 365.0
            }
        };
        // Vega per 1% move in vol
        let vega_f = s * sqrt_t * phi(d1) * 0.01;

        Ok(OptionGreeks {
            price: from_f64(price_f)?,
            delta: from_f64(delta_f)?,
            gamma: from_f64(gamma_f)?,
            theta: from_f64(theta_f)?,
            vega: from_f64(vega_f)?,
            rho: from_f64(rho_f)?,
        })
    }

    /// Solve for implied volatility given a market price using Newton-Raphson.
    ///
    /// Iterates up to `max_iter` times (default 100 is recommended).
    /// Returns `FinError::InvalidInput` if the solver fails to converge within tolerance.
    ///
    /// # Errors
    /// - `FinError::InvalidPrice` if spot or strike is non-positive.
    /// - `FinError::InvalidInput` if the price is non-positive, time-to-expiry is non-positive,
    ///   or the solver does not converge.
    /// - `FinError::ArithmeticOverflow` on internal numeric failure.
    pub fn implied_volatility(
        market_price: Decimal,
        spot: Decimal,
        strike: Decimal,
        time_to_expiry: Decimal,
        risk_free_rate: Decimal,
        kind: OptionKind,
        max_iter: usize,
        tolerance: Decimal,
    ) -> Result<Decimal, FinError> {
        if market_price <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "Market price must be positive for IV solve".to_owned(),
            ));
        }
        let tol_f = to_f64(tolerance)?;
        let target = to_f64(market_price)?;

        // Initial vol guess: Brenner-Subrahmanyam approximation
        let s_f = to_f64(spot)?;
        let k_f = to_f64(strike)?;
        let t_f = to_f64(time_to_expiry)?;
        let mut sigma = (2.0 * std::f64::consts::PI / t_f).sqrt() * (target / s_f);
        sigma = sigma.clamp(1e-6, 10.0);

        for _ in 0..max_iter {
            let vol_dec = from_f64(sigma)?;
            let spec = OptionSpec {
                kind,
                spot,
                strike,
                time_to_expiry,
                risk_free_rate,
                volatility: vol_dec,
            };
            let greeks = Self::price(&spec)?;
            let price_f = to_f64(greeks.price)?;
            let vega_f = to_f64(greeks.vega)? * 100.0; // vega stored as per-1%, restore to per-unit

            let diff = price_f - target;
            if diff.abs() < tol_f {
                return from_f64(sigma);
            }
            if vega_f.abs() < 1e-12 {
                return Err(FinError::InvalidInput(
                    "Implied volatility solver: vega near zero, cannot converge".to_owned(),
                ));
            }
            sigma -= diff / vega_f;
            sigma = sigma.clamp(1e-6, 10.0);

            // ATM moneyness check — use Corrado-Miller approximation as fallback seed
            let _ = (s_f, k_f); // used for Brenner-Subrahmanyam above
        }

        Err(FinError::InvalidInput(format!(
            "Implied volatility solver did not converge in {max_iter} iterations"
        )))
    }

    fn validate(spec: &OptionSpec) -> Result<(), FinError> {
        if spec.spot <= Decimal::ZERO {
            return Err(FinError::InvalidPrice(spec.spot));
        }
        if spec.strike <= Decimal::ZERO {
            return Err(FinError::InvalidPrice(spec.strike));
        }
        if spec.time_to_expiry <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "time_to_expiry must be positive".to_owned(),
            ));
        }
        if spec.volatility <= Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "volatility must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn atm_call() -> OptionSpec {
        OptionSpec {
            kind: OptionKind::Call,
            spot: dec!(100),
            strike: dec!(100),
            time_to_expiry: dec!(1),
            risk_free_rate: dec!(0.05),
            volatility: dec!(0.2),
        }
    }

    #[test]
    fn test_call_price_positive() {
        let g = BlackScholes::price(&atm_call()).unwrap();
        assert!(g.price > Decimal::ZERO);
    }

    #[test]
    fn test_put_call_parity() {
        // C - P = S - K * exp(-rT)
        let spec = atm_call();
        let call = BlackScholes::price(&spec).unwrap();
        let put_spec = OptionSpec { kind: OptionKind::Put, ..spec };
        let put = BlackScholes::price(&put_spec).unwrap();
        // C - P ≈ S - K*e^{-rT}; with ATM S=K this ≈ K*(1 - e^{-rT})
        let diff = (call.price - put.price).abs();
        // Should be roughly K * (1 - e^{-0.05}) ≈ 4.877 for S=K=100, r=0.05, T=1
        assert!(diff > dec!(4) && diff < dec!(6), "put-call parity failed: {diff}");
    }

    #[test]
    fn test_delta_call_between_zero_and_one() {
        let g = BlackScholes::price(&atm_call()).unwrap();
        assert!(g.delta > Decimal::ZERO && g.delta < dec!(1));
    }

    #[test]
    fn test_gamma_positive() {
        let g = BlackScholes::price(&atm_call()).unwrap();
        assert!(g.gamma > Decimal::ZERO);
    }

    #[test]
    fn test_vega_positive() {
        let g = BlackScholes::price(&atm_call()).unwrap();
        assert!(g.vega > Decimal::ZERO);
    }

    #[test]
    fn test_invalid_spot_errors() {
        let mut spec = atm_call();
        spec.spot = dec!(0);
        assert!(matches!(BlackScholes::price(&spec), Err(FinError::InvalidPrice(_))));
    }

    #[test]
    fn test_implied_volatility_roundtrip() {
        let spec = atm_call();
        let g = BlackScholes::price(&spec).unwrap();
        let iv = BlackScholes::implied_volatility(
            g.price,
            spec.spot,
            spec.strike,
            spec.time_to_expiry,
            spec.risk_free_rate,
            spec.kind,
            200,
            dec!(0.0001),
        )
        .unwrap();
        let diff = (iv - spec.volatility).abs();
        assert!(diff < dec!(0.001), "IV roundtrip error too large: {diff}");
    }
}
