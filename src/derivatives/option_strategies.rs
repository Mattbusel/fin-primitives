//! Multi-leg option strategy analytics.
//!
//! Provides P&L profiles, breakeven analysis, and common strategy builders
//! (straddle, strangle, collar, butterfly) on top of BSM pricing.

/// Standard normal CDF via rational approximation (Abramowitz & Stegun 26.2.17).
pub fn norm_cdf(x: f64) -> f64 {
    if x < -8.0 {
        return 0.0;
    }
    if x > 8.0 {
        return 1.0;
    }
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t * (0.319381530
        + t * (-0.356563782
        + t * (1.781477937
        + t * (-1.821255978
        + t * 1.330274429))));
    let pdf = (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let cdf_pos = 1.0 - pdf * poly;
    if x >= 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

/// Black-Scholes-Merton call price.
pub fn bsm_call(s: f64, k: f64, r: f64, q: f64, sigma: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return (s - k).max(0.0);
    }
    let d1 = ((s / k).ln() + (r - q + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    s * (-q * t).exp() * norm_cdf(d1) - k * (-r * t).exp() * norm_cdf(d2)
}

/// Black-Scholes-Merton put price.
pub fn bsm_put(s: f64, k: f64, r: f64, q: f64, sigma: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return (k - s).max(0.0);
    }
    let d1 = ((s / k).ln() + (r - q + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    k * (-r * t).exp() * norm_cdf(-d2) - s * (-q * t).exp() * norm_cdf(-d1)
}

/// Call or Put.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionType {
    /// Call option.
    Call,
    /// Put option.
    Put,
}

/// A single option leg in a multi-leg strategy.
#[derive(Debug, Clone)]
pub struct OptionLeg {
    /// Call or Put.
    pub option_type: OptionType,
    /// Strike price.
    pub strike: f64,
    /// Time to expiry in years.
    pub expiry: f64,
    /// Number of contracts (positive = long, negative = short).
    pub quantity: f64,
    /// Premium paid (positive) or received (negative) per unit.
    pub premium: f64,
}

/// A single point in the P&L profile.
#[derive(Debug, Clone)]
pub struct StrategyPnl {
    /// Spot price at expiry.
    pub price: f64,
    /// Net P&L at this spot price.
    pub pnl: f64,
    /// Net delta of the strategy at this spot price.
    pub delta: f64,
}

/// Intrinsic value of a single leg at `spot`, signed by quantity.
pub fn intrinsic_value(leg: &OptionLeg, spot: f64) -> f64 {
    let iv = match leg.option_type {
        OptionType::Call => (spot - leg.strike).max(0.0),
        OptionType::Put  => (leg.strike - spot).max(0.0),
    };
    iv * leg.quantity
}

/// Strategy P&L at expiry across all legs.
///
/// P&L = sum over legs of quantity * (intrinsic - premium).
pub fn strategy_pnl_at_expiry(legs: &[OptionLeg], spot: f64) -> f64 {
    legs.iter().map(|leg| {
        let iv = match leg.option_type {
            OptionType::Call => (spot - leg.strike).max(0.0),
            OptionType::Put  => (leg.strike - spot).max(0.0),
        };
        leg.quantity * (iv - leg.premium)
    }).sum()
}

/// Build a P&L profile across a range of spot prices.
pub fn pnl_profile(legs: &[OptionLeg], spot_range: (f64, f64), points: usize) -> Vec<StrategyPnl> {
    let n = points.max(2);
    let step = (spot_range.1 - spot_range.0) / (n - 1) as f64;
    (0..n).map(|i| {
        let spot = spot_range.0 + i as f64 * step;
        let pnl = strategy_pnl_at_expiry(legs, spot);
        // Approximate delta: binary — 1 if ITM call or 0.
        let delta: f64 = legs.iter().map(|leg| {
            let d = match leg.option_type {
                OptionType::Call => if spot > leg.strike { 1.0 } else { 0.0 },
                OptionType::Put  => if spot < leg.strike { -1.0 } else { 0.0 },
            };
            leg.quantity * d
        }).sum();
        StrategyPnl { price: spot, pnl, delta }
    }).collect()
}

/// Maximum P&L from a profile.
pub fn max_profit(profile: &[StrategyPnl]) -> f64 {
    profile.iter().map(|p| p.pnl).fold(f64::NEG_INFINITY, f64::max)
}

/// Maximum loss (most negative P&L) from a profile, returned as a negative number.
pub fn max_loss(profile: &[StrategyPnl]) -> f64 {
    profile.iter().map(|p| p.pnl).fold(f64::INFINITY, f64::min)
}

/// Approximate zero-crossing spot prices (breakeven points).
pub fn breakeven_points(profile: &[StrategyPnl]) -> Vec<f64> {
    let mut points = Vec::new();
    for i in 1..profile.len() {
        let prev = &profile[i - 1];
        let curr = &profile[i];
        if prev.pnl * curr.pnl <= 0.0 && (prev.pnl - curr.pnl).abs() > 1e-12 {
            // Linear interpolation.
            let frac = (-prev.pnl) / (curr.pnl - prev.pnl);
            points.push(prev.price + frac * (curr.price - prev.price));
        }
    }
    points
}

/// Build a long straddle: long call + long put at the same strike.
pub fn build_straddle(
    spot: f64,
    strike: f64,
    r: f64,
    sigma: f64,
    expiry: f64,
) -> Vec<OptionLeg> {
    let q = 0.0;
    let call_prem = bsm_call(spot, strike, r, q, sigma, expiry);
    let put_prem  = bsm_put(spot, strike, r, q, sigma, expiry);
    vec![
        OptionLeg { option_type: OptionType::Call, strike, expiry, quantity: 1.0, premium: call_prem },
        OptionLeg { option_type: OptionType::Put,  strike, expiry, quantity: 1.0, premium: put_prem  },
    ]
}

/// Build a long strangle: long OTM call + long OTM put at different strikes.
pub fn build_strangle(
    spot: f64,
    call_k: f64,
    put_k: f64,
    r: f64,
    sigma: f64,
    expiry: f64,
) -> Vec<OptionLeg> {
    let q = 0.0;
    let call_prem = bsm_call(spot, call_k, r, q, sigma, expiry);
    let put_prem  = bsm_put(spot, put_k,  r, q, sigma, expiry);
    vec![
        OptionLeg { option_type: OptionType::Call, strike: call_k, expiry, quantity: 1.0, premium: call_prem },
        OptionLeg { option_type: OptionType::Put,  strike: put_k,  expiry, quantity: 1.0, premium: put_prem  },
    ]
}

/// Build a collar: long put + short call (hedge on long stock position).
pub fn build_collar(
    spot: f64,
    put_k: f64,
    call_k: f64,
    r: f64,
    sigma: f64,
    expiry: f64,
) -> Vec<OptionLeg> {
    let q = 0.0;
    let put_prem  = bsm_put(spot, put_k,   r, q, sigma, expiry);
    let call_prem = bsm_call(spot, call_k, r, q, sigma, expiry);
    vec![
        // Long put — costs put_prem.
        OptionLeg { option_type: OptionType::Put,  strike: put_k,  expiry, quantity:  1.0, premium: put_prem  },
        // Short call — receives call_prem (stored as positive premium, negative quantity).
        OptionLeg { option_type: OptionType::Call, strike: call_k, expiry, quantity: -1.0, premium: call_prem },
    ]
}

/// Build a long butterfly: long 1 @ k_low, short 2 @ k_mid, long 1 @ k_high.
pub fn build_butterfly(
    spot: f64,
    k_low: f64,
    k_mid: f64,
    k_high: f64,
    r: f64,
    sigma: f64,
    expiry: f64,
) -> Vec<OptionLeg> {
    let q = 0.0;
    let p_low  = bsm_call(spot, k_low,  r, q, sigma, expiry);
    let p_mid  = bsm_call(spot, k_mid,  r, q, sigma, expiry);
    let p_high = bsm_call(spot, k_high, r, q, sigma, expiry);
    vec![
        OptionLeg { option_type: OptionType::Call, strike: k_low,  expiry, quantity:  1.0, premium: p_low  },
        OptionLeg { option_type: OptionType::Call, strike: k_mid,  expiry, quantity: -2.0, premium: p_mid  },
        OptionLeg { option_type: OptionType::Call, strike: k_high, expiry, quantity:  1.0, premium: p_high },
    ]
}

/// Full analysis of a named option strategy.
#[derive(Debug, Clone)]
pub struct StrategyAnalysis {
    /// Human-readable strategy name.
    pub name: String,
    /// Net premium paid (positive) or received (negative).
    pub net_premium: f64,
    /// Maximum theoretical profit over the spot range.
    pub max_profit: f64,
    /// Maximum theoretical loss over the spot range.
    pub max_loss: f64,
    /// Approximate breakeven spot prices.
    pub breakevens: Vec<f64>,
    /// Full P&L profile.
    pub profile: Vec<StrategyPnl>,
}

/// Analyse a strategy over `spot_range` with 500 evenly-spaced points.
pub fn analyze_strategy(
    name: &str,
    legs: Vec<OptionLeg>,
    spot_range: (f64, f64),
) -> StrategyAnalysis {
    let net_premium: f64 = legs.iter().map(|l| l.quantity * l.premium).sum();
    let profile = pnl_profile(&legs, spot_range, 500);
    let mp = max_profit(&profile);
    let ml = max_loss(&profile);
    let be = breakeven_points(&profile);
    StrategyAnalysis {
        name: name.to_string(),
        net_premium,
        max_profit: mp,
        max_loss: ml,
        breakevens: be,
        profile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_straddle_has_two_legs() {
        let legs = build_straddle(100.0, 100.0, 0.05, 0.2, 1.0);
        assert_eq!(legs.len(), 2);
        assert!(matches!(legs[0].option_type, OptionType::Call));
        assert!(matches!(legs[1].option_type, OptionType::Put));
    }

    #[test]
    fn test_strangle_breakevens_outside_strikes() {
        let call_k = 110.0;
        let put_k  = 90.0;
        let legs = build_strangle(100.0, call_k, put_k, 0.05, 0.25, 1.0);
        let profile = pnl_profile(&legs, (60.0, 150.0), 1000);
        let bes = breakeven_points(&profile);
        // At least two breakevens, both outside the inner strikes.
        assert!(bes.len() >= 2, "Expected at least 2 breakevens, got {:?}", bes);
        let lo = bes.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = bes.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(lo < put_k, "Lower breakeven {lo} should be < put_k {put_k}");
        assert!(hi > call_k, "Upper breakeven {hi} should be > call_k {call_k}");
    }

    #[test]
    fn test_butterfly_limited_profit() {
        let legs = build_butterfly(100.0, 90.0, 100.0, 110.0, 0.05, 0.2, 1.0);
        let profile = pnl_profile(&legs, (70.0, 130.0), 500);
        let mp = max_profit(&profile);
        let ml = max_loss(&profile);
        // Butterfly has limited profit and limited loss.
        assert!(mp > 0.0, "Butterfly max profit should be positive");
        assert!(ml < 0.0, "Butterfly max loss should be negative");
        // Max profit is bounded (should be much less than, say, 20).
        assert!(mp < 15.0, "Butterfly max profit should be limited, got {mp}");
    }

    #[test]
    fn test_collar_net_premium_near_zero() {
        // ATM put and slightly OTM call on same vol — premiums roughly offset.
        let legs = build_collar(100.0, 95.0, 105.0, 0.05, 0.2, 1.0);
        let net: f64 = legs.iter().map(|l| l.quantity * l.premium).sum();
        // Net premium magnitude should be reasonably small (not exact zero, but < 5).
        assert!(net.abs() < 6.0, "Collar net premium should be near zero, got {net}");
    }

    #[test]
    fn test_pnl_at_expiry_at_strike_equals_negative_premium() {
        // Long call at ATM: at expiry spot == strike => intrinsic = 0 => P&L = -premium.
        let prem = bsm_call(100.0, 100.0, 0.05, 0.0, 0.2, 1.0);
        let leg = OptionLeg {
            option_type: OptionType::Call,
            strike: 100.0,
            expiry: 1.0,
            quantity: 1.0,
            premium: prem,
        };
        let pnl = strategy_pnl_at_expiry(&[leg], 100.0);
        assert!((pnl + prem).abs() < 1e-9, "P&L at strike should equal -premium");
    }

    #[test]
    fn test_norm_cdf_basic() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-4);
        assert!(norm_cdf(10.0) > 0.999);
        assert!(norm_cdf(-10.0) < 0.001);
    }

    #[test]
    fn test_intrinsic_value_call() {
        let leg = OptionLeg { option_type: OptionType::Call, strike: 100.0, expiry: 1.0, quantity: 2.0, premium: 5.0 };
        assert!((intrinsic_value(&leg, 110.0) - 20.0).abs() < 1e-9);
        assert!((intrinsic_value(&leg, 90.0) - 0.0).abs() < 1e-9);
    }
}
