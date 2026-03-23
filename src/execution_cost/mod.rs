//! # Module: execution_cost
//!
//! ## Responsibility
//! Execution cost models including commission, bid-ask spread cost,
//! and market impact estimation (linear, square-root, Almgren-Chriss).
//!
//! ## Guarantees
//! - Zero panics on well-formed inputs; computations are pure functions
//! - `f64` is used intentionally for statistical cost estimates (not prices)
//! - Almgren-Chriss returns `(permanent_impact, temporary_impact)` as a tuple

// ─── ExecutionCostModel ───────────────────────────────────────────────────────

/// Commission model variants for execution cost estimation.
///
/// # Example
/// ```rust
/// use fin_primitives::execution_cost::ExecutionCostModel;
///
/// let fixed = ExecutionCostModel::Fixed(5.0);
/// let prop  = ExecutionCostModel::Proportional(10.0); // 10 bps
/// let zero  = ExecutionCostModel::ZeroCommission;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionCostModel {
    /// Flat commission in USD per trade.
    Fixed(f64),
    /// Commission as a proportion of notional value, in basis points.
    Proportional(f64),
    /// Tiered commission: Vec of `(notional_threshold_usd, rate_bps)`.
    /// The applicable tier is the last tier whose threshold is <= notional.
    Tiered(Vec<(f64, f64)>),
    /// No commission (e.g. payment-for-order-flow brokers).
    ZeroCommission,
}

impl ExecutionCostModel {
    /// Compute commission in USD for a given notional value.
    pub fn commission_usd(&self, notional: f64) -> f64 {
        match self {
            ExecutionCostModel::Fixed(c) => *c,
            ExecutionCostModel::Proportional(bps) => notional * bps / 10_000.0,
            ExecutionCostModel::Tiered(tiers) => {
                // Find last tier whose threshold <= notional
                let rate_bps = tiers
                    .iter()
                    .filter(|(threshold, _)| notional >= *threshold)
                    .last()
                    .map(|(_, rate)| *rate)
                    .unwrap_or_else(|| tiers.first().map(|(_, r)| *r).unwrap_or(0.0));
                notional * rate_bps / 10_000.0
            }
            ExecutionCostModel::ZeroCommission => 0.0,
        }
    }
}

// ─── SpreadCost ───────────────────────────────────────────────────────────────

/// Bid-ask spread cost estimator.
///
/// # Example
/// ```rust
/// use fin_primitives::execution_cost::SpreadCost;
///
/// // Bid = 99.90, Ask = 100.10  ->  half-spread = 10 bps
/// let hs = SpreadCost::half_spread_bps(99.90, 100.10);
/// assert!((hs - 10.0).abs() < 1e-6);
/// ```
pub struct SpreadCost;

impl SpreadCost {
    /// Half the bid-ask spread expressed in basis points.
    ///
    /// `half_spread_bps = (ask - bid) / (2 * mid) * 10_000`
    pub fn half_spread_bps(bid: f64, ask: f64) -> f64 {
        let mid = (bid + ask) / 2.0;
        if mid == 0.0 {
            return 0.0;
        }
        (ask - bid) / (2.0 * mid) * 10_000.0
    }

    /// Total spread cost in USD for a given order size.
    ///
    /// `spread_cost = size * half_spread_bps / 10_000 * mid_price`
    pub fn spread_cost(size: f64, bid: f64, ask: f64) -> f64 {
        let mid = (bid + ask) / 2.0;
        let hs_bps = Self::half_spread_bps(bid, ask);
        size * hs_bps / 10_000.0 * mid
    }
}

// ─── MarketImpact ─────────────────────────────────────────────────────────────

/// Market impact estimators: linear, square-root, and Almgren-Chriss.
pub struct MarketImpact;

impl MarketImpact {
    /// Linear market impact in USD.
    ///
    /// `impact = size / volume * impact_bps_per_pct / 10_000 * size`
    ///
    /// where `size / volume` is the participation rate in [0, 1].
    pub fn linear_impact(size: f64, volume: f64, impact_bps_per_pct: f64) -> f64 {
        if volume == 0.0 {
            return 0.0;
        }
        let participation = size / volume;
        participation * impact_bps_per_pct / 10_000.0 * size
    }

    /// Square-root market impact in USD.
    ///
    /// `impact = eta * sigma * sqrt(size / volume) * size`
    pub fn sqrt_impact(size: f64, volume: f64, sigma: f64, eta: f64) -> f64 {
        if volume == 0.0 {
            return 0.0;
        }
        eta * sigma * (size / volume).sqrt() * size
    }

    /// Almgren-Chriss permanent and temporary market impact.
    ///
    /// Returns `(permanent_impact_usd, temporary_impact_usd)`.
    ///
    /// - Permanent: `gamma * size` — persistent price shift
    /// - Temporary: `eta * sigma * sqrt(size / volume) * size`
    pub fn almgren_chriss(
        size: f64,
        volume: f64,
        sigma: f64,
        gamma: f64,
        eta: f64,
    ) -> (f64, f64) {
        let permanent = gamma * size;
        let temporary = Self::sqrt_impact(size, volume, sigma, eta);
        (permanent, temporary)
    }
}

// ─── ExecutionCostBreakdown ───────────────────────────────────────────────────

/// Breakdown of total execution cost across its components.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionCostBreakdown {
    /// Commission in USD.
    pub commission_usd: f64,
    /// Bid-ask spread cost in USD.
    pub spread_cost_usd: f64,
    /// Market impact cost in USD (square-root model).
    pub market_impact_usd: f64,
    /// Total cost in USD.
    pub total_usd: f64,
    /// Total cost in basis points of notional.
    pub total_bps: f64,
}

// ─── TotalExecutionCost ───────────────────────────────────────────────────────

/// Computes the full execution cost breakdown for an order.
///
/// # Example
/// ```rust
/// use fin_primitives::execution_cost::{TotalExecutionCost, ExecutionCostModel};
///
/// let breakdown = TotalExecutionCost::compute(
///     1000.0,   // size
///     150.0,    // price
///     &ExecutionCostModel::Proportional(5.0), // 5 bps commission
///     149.90,   // bid
///     150.10,   // ask
///     1_000_000.0, // daily volume
///     0.02,     // sigma (daily vol)
/// );
/// assert!(breakdown.total_usd > 0.0);
/// ```
pub struct TotalExecutionCost;

impl TotalExecutionCost {
    /// Compute the total execution cost breakdown.
    ///
    /// Uses the square-root (Kyle-style) model for market impact with `eta = 0.1`.
    pub fn compute(
        size: f64,
        price: f64,
        commission_model: &ExecutionCostModel,
        bid: f64,
        ask: f64,
        volume: f64,
        sigma: f64,
    ) -> ExecutionCostBreakdown {
        let notional = size * price;
        let commission_usd = commission_model.commission_usd(notional);
        let spread_cost_usd = SpreadCost::spread_cost(size, bid, ask);
        let market_impact_usd = MarketImpact::sqrt_impact(size, volume, sigma, 0.1);

        let total_usd = commission_usd + spread_cost_usd + market_impact_usd;
        let total_bps = if notional > 0.0 {
            total_usd / notional * 10_000.0
        } else {
            0.0
        };

        ExecutionCostBreakdown {
            commission_usd,
            spread_cost_usd,
            market_impact_usd,
            total_usd,
            total_bps,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ExecutionCostModel ──

    #[test]
    fn test_fixed_commission() {
        let m = ExecutionCostModel::Fixed(7.50);
        assert!((m.commission_usd(10_000.0) - 7.50).abs() < 1e-9);
        assert!((m.commission_usd(1.0) - 7.50).abs() < 1e-9);
    }

    #[test]
    fn test_proportional_commission() {
        // 10 bps on $10_000 notional = $10
        let m = ExecutionCostModel::Proportional(10.0);
        assert!((m.commission_usd(10_000.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn test_zero_commission() {
        let m = ExecutionCostModel::ZeroCommission;
        assert_eq!(m.commission_usd(100_000.0), 0.0);
    }

    #[test]
    fn test_tiered_commission() {
        // Tier 0: 0 threshold -> 20 bps
        // Tier 1: 10_000 threshold -> 10 bps
        // Tier 2: 100_000 threshold -> 5 bps
        let m = ExecutionCostModel::Tiered(vec![
            (0.0, 20.0),
            (10_000.0, 10.0),
            (100_000.0, 5.0),
        ]);
        // notional $5000 -> tier 0: 20 bps = $10
        assert!((m.commission_usd(5_000.0) - 10.0).abs() < 1e-9);
        // notional $50_000 -> tier 1: 10 bps = $50
        assert!((m.commission_usd(50_000.0) - 50.0).abs() < 1e-9);
        // notional $200_000 -> tier 2: 5 bps = $100
        assert!((m.commission_usd(200_000.0) - 100.0).abs() < 1e-9);
    }

    // ── SpreadCost ──

    #[test]
    fn test_half_spread_bps() {
        // bid=99.90, ask=100.10, mid=100.0 -> half spread = 0.10/100.0 * 5000 = 10 bps
        let hs = SpreadCost::half_spread_bps(99.90, 100.10);
        assert!((hs - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_half_spread_bps_zero_mid() {
        assert_eq!(SpreadCost::half_spread_bps(0.0, 0.0), 0.0);
    }

    #[test]
    fn test_spread_cost() {
        // size=1000, bid=99.90, ask=100.10, mid=100.0, hs=10bps
        // cost = 1000 * 10/10000 * 100 = $100
        let cost = SpreadCost::spread_cost(1000.0, 99.90, 100.10);
        assert!((cost - 100.0).abs() < 1e-6);
    }

    // ── MarketImpact ──

    #[test]
    fn test_linear_impact() {
        // size=1000, volume=100_000, participation=1%, impact_bps_per_pct=50
        // impact = 0.01 * 50/10000 * 1000 = 0.05
        let imp = MarketImpact::linear_impact(1000.0, 100_000.0, 50.0);
        assert!((imp - 0.05).abs() < 1e-9);
    }

    #[test]
    fn test_linear_impact_zero_volume() {
        assert_eq!(MarketImpact::linear_impact(1000.0, 0.0, 50.0), 0.0);
    }

    #[test]
    fn test_sqrt_impact() {
        // size=10000, volume=1_000_000, sigma=0.02, eta=0.1
        // impact = 0.1 * 0.02 * sqrt(0.01) * 10000 = 0.1 * 0.02 * 0.1 * 10000 = 2.0
        let imp = MarketImpact::sqrt_impact(10_000.0, 1_000_000.0, 0.02, 0.1);
        assert!((imp - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_sqrt_impact_zero_volume() {
        assert_eq!(MarketImpact::sqrt_impact(1000.0, 0.0, 0.02, 0.1), 0.0);
    }

    #[test]
    fn test_almgren_chriss() {
        // gamma=0.001, eta=0.1, sigma=0.02
        // permanent = 0.001 * 10000 = 10
        // temporary = sqrt_impact(10000, 1_000_000, 0.02, 0.1) = 2.0
        let (perm, temp) = MarketImpact::almgren_chriss(10_000.0, 1_000_000.0, 0.02, 0.001, 0.1);
        assert!((perm - 10.0).abs() < 1e-9);
        assert!((temp - 2.0).abs() < 1e-9);
    }

    // ── TotalExecutionCost ──

    #[test]
    fn test_total_execution_cost_components() {
        let breakdown = TotalExecutionCost::compute(
            1000.0,
            100.0,
            &ExecutionCostModel::Fixed(5.0),
            99.90,
            100.10,
            1_000_000.0,
            0.02,
        );
        // commission = $5 (fixed)
        assert!((breakdown.commission_usd - 5.0).abs() < 1e-9);
        // spread cost: size=1000, hs=10bps, mid=100 -> $100
        assert!((breakdown.spread_cost_usd - 100.0).abs() < 1e-6);
        // market impact: eta=0.1, sigma=0.02, sqrt(1000/1_000_000)=0.03162, *1000 = 3.162*0.002 = 0.0632...
        // sqrt_impact = 0.1 * 0.02 * sqrt(0.001) * 1000 = 0.1 * 0.02 * 0.031623 * 1000 = 0.063246
        assert!(breakdown.market_impact_usd > 0.0);
        assert!((breakdown.total_usd - (breakdown.commission_usd + breakdown.spread_cost_usd + breakdown.market_impact_usd)).abs() < 1e-9);
    }

    #[test]
    fn test_total_execution_cost_bps() {
        let breakdown = TotalExecutionCost::compute(
            1000.0,
            100.0,
            &ExecutionCostModel::ZeroCommission,
            99.90,
            100.10,
            1_000_000.0,
            0.02,
        );
        // notional = $100_000
        let expected_bps = breakdown.total_usd / 100_000.0 * 10_000.0;
        assert!((breakdown.total_bps - expected_bps).abs() < 1e-9);
    }

    #[test]
    fn test_total_execution_cost_zero_commission() {
        let breakdown = TotalExecutionCost::compute(
            100.0,
            50.0,
            &ExecutionCostModel::ZeroCommission,
            49.95,
            50.05,
            100_000.0,
            0.015,
        );
        assert_eq!(breakdown.commission_usd, 0.0);
        assert!(breakdown.total_usd > 0.0);
    }
}
