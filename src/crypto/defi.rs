//! DeFi AMM pricing, liquidity pools, and impermanent loss.
//!
//! Provides constant-product AMM mechanics (UniswapV2-style), spot pricing,
//! impermanent loss calculations, LP position tracking, and arbitrage detection.

// ─────────────────────────────────────────
//  AmmType
// ─────────────────────────────────────────

/// The type of automated market maker (AMM) curve.
#[derive(Debug, Clone, PartialEq)]
pub enum AmmType {
    /// Constant-product formula: x * y = k (Uniswap V2-style).
    UniswapV2,
    /// Stable-swap curve optimized for assets near parity (Curve-style).
    StableSwap,
    /// Concentrated liquidity within a price range (Uniswap V3-style).
    Concentrated,
}

// ─────────────────────────────────────────
//  LiquidityPool
// ─────────────────────────────────────────

/// A two-token liquidity pool.
///
/// Tracks reserves, fee tier, AMM type, and total LP token supply.
#[derive(Debug, Clone)]
pub struct LiquidityPool {
    /// Token A identifier.
    pub token_a: String,
    /// Token B identifier.
    pub token_b: String,
    /// Reserve of token A in the pool.
    pub reserve_a: f64,
    /// Reserve of token B in the pool.
    pub reserve_b: f64,
    /// Fee in basis points (e.g. 30 = 0.30%).
    pub fee_bps: u32,
    /// AMM curve type.
    pub amm_type: AmmType,
    /// Total LP tokens outstanding.
    pub total_supply: f64,
}

impl LiquidityPool {
    /// Create a new liquidity pool.
    ///
    /// `total_supply` is initialized to `reserve_a` (arbitrary but consistent
    /// as a bootstrapping convention — matches Uniswap V2's sqrt(a*b) only for
    /// new pools; here we keep it simple and start at reserve_a).
    #[must_use]
    pub fn new(
        token_a: impl Into<String>,
        token_b: impl Into<String>,
        reserve_a: f64,
        reserve_b: f64,
        fee_bps: u32,
        amm_type: AmmType,
    ) -> Self {
        // Bootstrap total supply as geometric mean of reserves (Uniswap V2 convention).
        let total_supply = (reserve_a * reserve_b).sqrt().max(0.0);
        Self {
            token_a: token_a.into(),
            token_b: token_b.into(),
            reserve_a,
            reserve_b,
            fee_bps,
            amm_type,
            total_supply,
        }
    }

    /// Spot price of token A in terms of token B: `reserve_b / reserve_a`.
    ///
    /// Returns `0.0` if `reserve_a` is zero.
    #[must_use]
    pub fn spot_price(&self) -> f64 {
        if self.reserve_a == 0.0 {
            return 0.0;
        }
        self.reserve_b / self.reserve_a
    }

    /// Compute the output amount for a swap using the constant-product formula.
    ///
    /// Fee is deducted from `amount_in` before applying the AMM formula:
    /// `fee = amount_in * fee_bps / 10_000`.
    ///
    /// If `a_to_b` is `true`, the user swaps token A for token B.
    /// Returns `0.0` if reserves are zero.
    #[must_use]
    pub fn get_amount_out(&self, amount_in: f64, a_to_b: bool) -> f64 {
        if amount_in <= 0.0 {
            return 0.0;
        }
        let fee = amount_in * self.fee_bps as f64 / 10_000.0;
        let amount_in_after_fee = amount_in - fee;

        if a_to_b {
            // Swap A → B: new_reserve_a = reserve_a + amount_in_after_fee
            // k = reserve_a * reserve_b
            // new_reserve_b = k / new_reserve_a
            // out = reserve_b - new_reserve_b
            if self.reserve_a == 0.0 || self.reserve_b == 0.0 {
                return 0.0;
            }
            let k = self.reserve_a * self.reserve_b;
            let new_reserve_a = self.reserve_a + amount_in_after_fee;
            let new_reserve_b = k / new_reserve_a;
            (self.reserve_b - new_reserve_b).max(0.0)
        } else {
            // Swap B → A
            if self.reserve_a == 0.0 || self.reserve_b == 0.0 {
                return 0.0;
            }
            let k = self.reserve_a * self.reserve_b;
            let new_reserve_b = self.reserve_b + amount_in_after_fee;
            let new_reserve_a = k / new_reserve_b;
            (self.reserve_a - new_reserve_a).max(0.0)
        }
    }

    /// Add liquidity to the pool and return the number of LP tokens minted.
    ///
    /// LP tokens minted = `min(amount_a / reserve_a, amount_b / reserve_b) * total_supply`.
    /// Returns `0.0` if reserves are zero (initial liquidity case not handled here).
    pub fn add_liquidity(&mut self, amount_a: f64, amount_b: f64) -> f64 {
        if self.reserve_a == 0.0 || self.reserve_b == 0.0 || self.total_supply == 0.0 {
            // Bootstrap: first liquidity addition
            let lp = (amount_a * amount_b).sqrt();
            self.reserve_a += amount_a;
            self.reserve_b += amount_b;
            self.total_supply += lp;
            return lp;
        }
        let ratio_a = amount_a / self.reserve_a;
        let ratio_b = amount_b / self.reserve_b;
        let lp_tokens = ratio_a.min(ratio_b) * self.total_supply;
        self.reserve_a += amount_a;
        self.reserve_b += amount_b;
        self.total_supply += lp_tokens;
        lp_tokens
    }

    /// Remove liquidity proportional to `lp_tokens` and return `(amount_a, amount_b)`.
    ///
    /// Returns `(0.0, 0.0)` if `total_supply` is zero.
    pub fn remove_liquidity(&mut self, lp_tokens: f64) -> (f64, f64) {
        if self.total_supply == 0.0 || lp_tokens <= 0.0 {
            return (0.0, 0.0);
        }
        let share = (lp_tokens / self.total_supply).min(1.0);
        let amount_a = self.reserve_a * share;
        let amount_b = self.reserve_b * share;
        self.reserve_a -= amount_a;
        self.reserve_b -= amount_b;
        self.total_supply -= lp_tokens.min(self.total_supply);
        (amount_a, amount_b)
    }
}

// ─────────────────────────────────────────
//  Standalone functions
// ─────────────────────────────────────────

/// Compute the price impact of a swap as a fraction of the current spot price.
///
/// Price impact = `|new_spot - old_spot| / old_spot`.
/// Returns `0.0` if spot price is zero.
#[must_use]
pub fn price_impact(amount_in: f64, a_to_b: bool, pool: &LiquidityPool) -> f64 {
    let old_price = pool.spot_price();
    if old_price == 0.0 {
        return 0.0;
    }
    let fee = amount_in * pool.fee_bps as f64 / 10_000.0;
    let amount_in_after_fee = amount_in - fee;

    // Simulate new reserves after swap.
    let new_price = if a_to_b {
        let new_reserve_a = pool.reserve_a + amount_in_after_fee;
        if new_reserve_a == 0.0 {
            return 0.0;
        }
        pool.reserve_b / new_reserve_a
    } else {
        let new_reserve_b = pool.reserve_b + amount_in_after_fee;
        if new_reserve_b == 0.0 {
            return 0.0;
        }
        pool.reserve_b / pool.reserve_a // approximate: use old ratio shifted
        // More precise: new spot = new_reserve_b_after_swap / reserve_a
        // but reserve_a changes for B→A swaps. Recalculate:
    };

    // Recompute more precisely for B→A direction.
    let new_price = if !a_to_b {
        let k = pool.reserve_a * pool.reserve_b;
        let new_reserve_b = pool.reserve_b + amount_in_after_fee;
        if new_reserve_b == 0.0 {
            return 0.0;
        }
        let new_reserve_a = k / new_reserve_b;
        if new_reserve_a == 0.0 {
            return 0.0;
        }
        new_reserve_b / new_reserve_a
    } else {
        new_price
    };

    (new_price - old_price).abs() / old_price
}

/// Impermanent loss for a given `price_ratio_change` (new_price / initial_price).
///
/// Formula: `IL = 2 * sqrt(r) / (1 + r) - 1` where `r = price_ratio_change`.
///
/// Returns `0.0` if `r <= 0`.
#[must_use]
pub fn impermanent_loss(price_ratio_change: f64) -> f64 {
    let r = price_ratio_change;
    if r <= 0.0 {
        return 0.0;
    }
    2.0 * r.sqrt() / (1.0 + r) - 1.0
}

// ─────────────────────────────────────────
//  LpPosition
// ─────────────────────────────────────────

/// Tracks an LP position's entry state.
#[derive(Debug, Clone)]
pub struct LpPosition {
    /// Pool identifier string.
    pub pool_id: String,
    /// Number of LP tokens held.
    pub lp_tokens: f64,
    /// Price ratio (token_b / token_a) at entry.
    pub entry_price_ratio: f64,
    /// USD value of the position at entry.
    pub entry_value_usd: f64,
}

/// Estimate current USD value of an LP position.
///
/// Assumes the position's share of the pool scales with the square root of
/// the price ratio change (constant-product AMM property).
/// `current_value = entry_value_usd * sqrt(current_price_ratio / entry_price_ratio) * 2 / (1 + ratio_change_sqrt_ratio)`
///
/// Simplified: `value = 2 * entry_value_usd * sqrt(r) / (1 + r)`
/// where `r = current_price_ratio / entry_price_ratio`.
///
/// Returns `entry_value_usd` if `entry_price_ratio` is zero.
#[must_use]
pub fn current_value(pos: &LpPosition, current_price_ratio: f64, token_a_price_usd: f64) -> f64 {
    if pos.entry_price_ratio == 0.0 {
        return pos.entry_value_usd;
    }
    let r = current_price_ratio / pos.entry_price_ratio;
    if r <= 0.0 {
        return 0.0;
    }
    // Value of LP position under constant-product: proportional to 2*sqrt(r)/(1+r) of initial value
    // relative to hodl. Here we approximate current value as a function of USD price shift.
    let il = impermanent_loss(r); // negative number
    let hodl_value = pos.entry_value_usd * (current_price_ratio / pos.entry_price_ratio + 1.0) / 2.0
        * (token_a_price_usd / (pos.entry_value_usd / 2.0).max(1e-12));
    // Simpler: current_value = entry_value * (1 + IL) scaled by price change
    // Use: entry amount A = entry_value_usd / 2 / token_a_price_at_entry
    // token_a_price_at_entry = entry_price_ratio * token_b_price; but we don't have that.
    // Use straightforward approach: LP value tracks sqrt(price_ratio) * constant
    // v(r) = entry_value * sqrt(r) * 2/(1+r)  (this gives the AMM LP value vs hodl)
    // but we want absolute value: just apply (1+il) to entry_value:
    let _ = hodl_value;
    pos.entry_value_usd * (1.0 + il) * (r).sqrt()
}

/// Impermanent loss percentage for an LP position at the current price ratio.
///
/// Returns `impermanent_loss(current_price_ratio / entry_price_ratio)`.
#[must_use]
pub fn il_pct(pos: &LpPosition, current_price_ratio: f64) -> f64 {
    if pos.entry_price_ratio == 0.0 {
        return 0.0;
    }
    let r = current_price_ratio / pos.entry_price_ratio;
    impermanent_loss(r)
}

// ─────────────────────────────────────────
//  ArbitrageOpportunity
// ─────────────────────────────────────────

/// A detected arbitrage opportunity between two pools.
#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    /// Spot price in pool A.
    pub pool_a_price: f64,
    /// Spot price in pool B.
    pub pool_b_price: f64,
    /// Profit percentage after accounting for fees.
    pub profit_pct: f64,
    /// `true` = buy in pool A, sell in pool B; `false` = reverse.
    pub direction: bool,
}

/// Find an arbitrage opportunity between two pools if the price difference
/// exceeds the combined fee cost.
///
/// Threshold: `|price_a - price_b| / min(price_a, price_b) > (fee_a + fee_b) / 10_000`.
///
/// Returns `None` if no profitable arbitrage exists.
#[must_use]
pub fn find_arbitrage(
    pool_a: &LiquidityPool,
    pool_b: &LiquidityPool,
) -> Option<ArbitrageOpportunity> {
    let price_a = pool_a.spot_price();
    let price_b = pool_b.spot_price();
    if price_a == 0.0 || price_b == 0.0 {
        return None;
    }

    let combined_fee = (pool_a.fee_bps + pool_b.fee_bps) as f64 / 10_000.0;
    let min_price = price_a.min(price_b);
    let price_diff_pct = (price_a - price_b).abs() / min_price;

    if price_diff_pct > combined_fee {
        let profit_pct = price_diff_pct - combined_fee;
        let direction = price_a < price_b; // buy in A (cheaper), sell in B (dearer)
        Some(ArbitrageOpportunity {
            pool_a_price: price_a,
            pool_b_price: price_b,
            profit_pct,
            direction,
        })
    } else {
        None
    }
}

// ─────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pool(ra: f64, rb: f64, fee_bps: u32) -> LiquidityPool {
        LiquidityPool::new("ETH", "USDC", ra, rb, fee_bps, AmmType::UniswapV2)
    }

    #[test]
    fn spot_price_basic() {
        let pool = make_pool(100.0, 200_000.0, 30);
        // 200000 / 100 = 2000
        assert!((pool.spot_price() - 2000.0).abs() < 1e-9);
    }

    #[test]
    fn spot_price_zero_reserve() {
        let pool = make_pool(0.0, 100.0, 30);
        assert_eq!(pool.spot_price(), 0.0);
    }

    #[test]
    fn constant_product_formula() {
        // k = 100 * 200_000 = 20_000_000
        // Swap 1 ETH in (fee = 0.3% → 0.003 ETH fee → 0.997 ETH in after fee)
        // new_reserve_a = 100 + 0.997 = 100.997
        // new_reserve_b = 20_000_000 / 100.997 ≈ 198_024.16
        // out = 200_000 - 198_024.16 ≈ 1975.84
        let pool = make_pool(100.0, 200_000.0, 30);
        let out = pool.get_amount_out(1.0, true);
        assert!(out > 0.0);
        assert!(out < 2000.0, "output must be less than spot * amount_in");
        // Verify constant product is maintained approximately
        let k_before = 100.0 * 200_000.0;
        let fee = 1.0 * 30.0 / 10_000.0;
        let amount_in_net = 1.0 - fee;
        let new_ra = 100.0 + amount_in_net;
        let new_rb = 200_000.0 - out;
        assert!((new_ra * new_rb - k_before).abs() < 1e-3);
    }

    #[test]
    fn get_amount_out_zero_input() {
        let pool = make_pool(100.0, 200_000.0, 30);
        assert_eq!(pool.get_amount_out(0.0, true), 0.0);
    }

    #[test]
    fn price_impact_increases_with_size() {
        let pool = make_pool(1000.0, 1_000_000.0, 30);
        let small = price_impact(1.0, true, &pool);
        let large = price_impact(100.0, true, &pool);
        assert!(large > small, "larger trade should have more price impact");
    }

    #[test]
    fn impermanent_loss_at_2x_price_change() {
        // At r=2 (price doubles): IL = 2*sqrt(2)/(1+2) - 1 ≈ -0.05719...
        let il = impermanent_loss(2.0);
        assert!((il - (-0.05719_f64)).abs() < 1e-4, "IL at 2x ≈ -5.72%, got {il}");
        assert!(il < 0.0, "IL must be negative");
    }

    #[test]
    fn impermanent_loss_no_change() {
        // At r=1: IL = 2*sqrt(1)/(1+1) - 1 = 2*1/2 - 1 = 0
        let il = impermanent_loss(1.0);
        assert!(il.abs() < 1e-10);
    }

    #[test]
    fn impermanent_loss_zero_ratio() {
        assert_eq!(impermanent_loss(0.0), 0.0);
    }

    #[test]
    fn add_and_remove_liquidity() {
        let mut pool = make_pool(1000.0, 1_000_000.0, 30);
        let initial_supply = pool.total_supply;
        let lp = pool.add_liquidity(10.0, 10_000.0);
        assert!(lp > 0.0);
        assert!(pool.total_supply > initial_supply);
        let (a, b) = pool.remove_liquidity(lp);
        assert!(a > 0.0 && b > 0.0);
    }

    #[test]
    fn arbitrage_detection_significant_spread() {
        // Pool A: ETH at 2000, Pool B: ETH at 2100 → 5% spread > 0.6% combined fee
        let pool_a = make_pool(100.0, 200_000.0, 30);
        let pool_b = make_pool(100.0, 210_000.0, 30);
        let arb = find_arbitrage(&pool_a, &pool_b);
        assert!(arb.is_some());
        let arb = arb.unwrap();
        assert!(arb.profit_pct > 0.0);
        assert!(arb.direction, "should buy in pool_a (cheaper)");
    }

    #[test]
    fn arbitrage_detection_no_opportunity() {
        // Same pool → no arbitrage
        let pool_a = make_pool(100.0, 200_000.0, 30);
        let pool_b = make_pool(100.0, 200_060.0, 30); // tiny spread < combined fee
        let arb = find_arbitrage(&pool_a, &pool_b);
        // 60/200000 = 0.03% spread vs 0.06% combined fee → no arb
        assert!(arb.is_none());
    }
}
