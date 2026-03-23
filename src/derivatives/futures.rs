//! Futures pricing and calendar spread analytics.
//!
//! Provides fair-value calculation, basis metrics, roll yield, and
//! calendar-spread analytics for commodity and financial futures.

/// A futures contract with all cost-of-carry components.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct FuturesContract {
    /// Price of the underlying asset.
    pub underlying_price: f64,
    /// Annualised risk-free interest rate (e.g. 0.05 = 5 %).
    pub risk_free_rate: f64,
    /// Annualised convenience yield (e.g. 0.02 = 2 %).
    pub convenience_yield: f64,
    /// Annualised storage cost (e.g. 0.01 = 1 %).
    pub storage_cost: f64,
    /// Time to expiry in years.
    pub time_to_expiry: f64,
    /// Current spot price (may equal `underlying_price`).
    pub spot_price: f64,
}

/// Theoretical futures fair value: F = S · exp((r − q + u) · T).
///
/// - `r` = risk-free rate
/// - `q` = convenience yield
/// - `u` = storage cost
/// - `T` = time to expiry
pub fn fair_value(contract: &FuturesContract) -> f64 {
    let carry = contract.risk_free_rate - contract.convenience_yield + contract.storage_cost;
    contract.spot_price * (carry * contract.time_to_expiry).exp()
}

/// Basis = spot − futures.
pub fn basis(contract: &FuturesContract, futures_price: f64) -> f64 {
    contract.spot_price - futures_price
}

/// Annualised basis = (spot − futures) / spot / T × 365.
pub fn annualized_basis(contract: &FuturesContract, futures_price: f64) -> f64 {
    let b = basis(contract, futures_price);
    if contract.spot_price == 0.0 || contract.time_to_expiry == 0.0 {
        return 0.0;
    }
    b / contract.spot_price / contract.time_to_expiry * 365.0
}

/// Cost of carry = (r + u − q) · T.
pub fn cost_of_carry(contract: &FuturesContract) -> f64 {
    (contract.risk_free_rate + contract.storage_cost - contract.convenience_yield)
        * contract.time_to_expiry
}

/// Implied repo rate: solve for r from F = S · exp((r − q + u) · T).
///
/// Returns `r = ln(F/S)/T − u + q`.
pub fn implied_repo_rate(
    spot: f64,
    futures: f64,
    t: f64,
    convenience_yield: f64,
    storage: f64,
) -> f64 {
    if spot <= 0.0 || t == 0.0 {
        return 0.0;
    }
    (futures / spot).ln() / t - storage + convenience_yield
}

// ─── Calendar spread ──────────────────────────────────────────────────────────

/// A futures calendar spread: two contracts with different expiries.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct FuturesCalendarSpread {
    /// Price of the near-dated contract.
    pub near_price: f64,
    /// Price of the far-dated contract.
    pub far_price: f64,
    /// Time to expiry of the near leg (years).
    pub near_expiry: f64,
    /// Time to expiry of the far leg (years).
    pub far_expiry: f64,
}

/// Calendar spread value = far − near.
pub fn spread_value(cs: &FuturesCalendarSpread) -> f64 {
    cs.far_price - cs.near_price
}

/// Theoretical calendar spread = fair_value(far) − fair_value(near).
pub fn theoretical_spread(
    spot: f64,
    r: f64,
    q: f64,
    u: f64,
    cs: &FuturesCalendarSpread,
) -> f64 {
    let make = |t: f64| {
        let c = FuturesContract {
            underlying_price: spot,
            risk_free_rate: r,
            convenience_yield: q,
            storage_cost: u,
            time_to_expiry: t,
            spot_price: spot,
        };
        fair_value(&c)
    };
    make(cs.far_expiry) - make(cs.near_expiry)
}

/// Carry-trade P&L = (exit_spread − entry_spread) × notional.
pub fn carry_trade_pnl(entry_spread: f64, exit_spread: f64, notional: f64) -> f64 {
    (exit_spread - entry_spread) * notional
}

// ─── Roll yield ───────────────────────────────────────────────────────────────

/// Input data for roll-yield calculation.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RollYield {
    /// Near contract price.
    pub near_price: f64,
    /// Far contract price.
    pub far_price: f64,
    /// Calendar days between the two contract expiries.
    pub days_between: f64,
}

/// Annualised roll yield = (near − far) / far × 365 / days_between.
///
/// Positive in backwardation, negative in contango.
pub fn roll_yield(ry: &RollYield) -> f64 {
    if ry.far_price == 0.0 || ry.days_between == 0.0 {
        return 0.0;
    }
    (ry.near_price - ry.far_price) / ry.far_price * 365.0 / ry.days_between
}

/// Returns `"contango"` if far > near, otherwise `"backwardation"`.
pub fn contango_or_backwardation(ry: &RollYield) -> &'static str {
    if ry.far_price > ry.near_price {
        "contango"
    } else {
        "backwardation"
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_contract(r: f64, q: f64, u: f64, t: f64, spot: f64) -> FuturesContract {
        FuturesContract {
            underlying_price: spot,
            risk_free_rate: r,
            convenience_yield: q,
            storage_cost: u,
            time_to_expiry: t,
            spot_price: spot,
        }
    }

    #[test]
    fn fair_value_zero_rates_equals_spot() {
        let c = make_contract(0.0, 0.0, 0.0, 0.5, 100.0);
        let fv = fair_value(&c);
        assert!((fv - 100.0).abs() < 1e-10, "expected 100.0 got {fv}");
    }

    #[test]
    fn fair_value_positive_carry() {
        let c = make_contract(0.05, 0.02, 0.01, 1.0, 100.0);
        let fv = fair_value(&c);
        // F = 100 * exp(0.04) ≈ 104.081
        let expected = 100.0_f64 * 0.04_f64.exp();
        assert!((fv - expected).abs() < 1e-8);
    }

    #[test]
    fn basis_calculation() {
        let c = make_contract(0.05, 0.0, 0.0, 1.0, 100.0);
        // futures at 102
        let b = basis(&c, 102.0);
        assert!((b - (-2.0)).abs() < 1e-10);
    }

    #[test]
    fn annualized_basis_calculation() {
        let c = make_contract(0.05, 0.0, 0.0, 0.5, 100.0);
        // futures at 102, basis = -2
        let ab = annualized_basis(&c, 102.0);
        // -2 / 100 / 0.5 * 365 = -14.6
        let expected = -2.0 / 100.0 / 0.5 * 365.0;
        assert!((ab - expected).abs() < 1e-8);
    }

    #[test]
    fn cost_of_carry_calculation() {
        let c = make_contract(0.05, 0.02, 0.01, 2.0, 100.0);
        // (0.05 + 0.01 - 0.02) * 2.0 = 0.08
        let coc = cost_of_carry(&c);
        assert!((coc - 0.08).abs() < 1e-10);
    }

    #[test]
    fn implied_repo_rate_round_trip() {
        let spot = 100.0;
        let r = 0.05;
        let q = 0.02;
        let u = 0.01;
        let t = 1.0;
        let f = spot * ((r - q + u) * t).exp();
        let r2 = implied_repo_rate(spot, f, t, q, u);
        assert!((r2 - r).abs() < 1e-10, "r={r} r2={r2}");
    }

    #[test]
    fn calendar_spread_value() {
        let cs = FuturesCalendarSpread {
            near_price: 100.0,
            far_price: 105.0,
            near_expiry: 0.25,
            far_expiry: 0.5,
        };
        assert!((spread_value(&cs) - 5.0).abs() < 1e-10);
    }

    #[test]
    fn theoretical_spread_zero_carry() {
        let cs = FuturesCalendarSpread {
            near_price: 100.0,
            far_price: 100.0,
            near_expiry: 0.25,
            far_expiry: 0.5,
        };
        // zero carry => both fair values equal spot => spread = 0
        let ts = theoretical_spread(100.0, 0.0, 0.0, 0.0, &cs);
        assert!(ts.abs() < 1e-10);
    }

    #[test]
    fn carry_trade_pnl_calculation() {
        let pnl = carry_trade_pnl(5.0, 7.0, 1_000_000.0);
        assert!((pnl - 2_000_000.0).abs() < 1e-6);
    }

    #[test]
    fn roll_yield_sign_contango() {
        let ry = RollYield { near_price: 100.0, far_price: 105.0, days_between: 30.0 };
        let ry_val = roll_yield(&ry);
        assert!(ry_val < 0.0, "contango => negative roll yield, got {ry_val}");
        assert_eq!(contango_or_backwardation(&ry), "contango");
    }

    #[test]
    fn roll_yield_sign_backwardation() {
        let ry = RollYield { near_price: 105.0, far_price: 100.0, days_between: 30.0 };
        let ry_val = roll_yield(&ry);
        assert!(ry_val > 0.0, "backwardation => positive roll yield, got {ry_val}");
        assert_eq!(contango_or_backwardation(&ry), "backwardation");
    }
}
