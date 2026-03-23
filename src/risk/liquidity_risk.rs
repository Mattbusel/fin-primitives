//! Liquidity risk measurement: market depth, Amihud illiquidity, liquidation cost,
//! and a portfolio-level liquidity-adjusted VaR engine.

use std::fmt;

// ---------------------------------------------------------------------------
// Side
// ---------------------------------------------------------------------------

/// Trade side for market impact calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Buy order — walks up the ask side.
    Buy,
    /// Sell order — walks down the bid side.
    Sell,
}

// ---------------------------------------------------------------------------
// MarketDepth
// ---------------------------------------------------------------------------

/// L2 order book depth snapshot.
///
/// `bid_levels` and `ask_levels` are (price, size) tuples sorted by price:
/// bids descending, asks ascending.
#[derive(Debug, Clone)]
pub struct MarketDepth {
    /// Bid levels: (price, size), highest price first.
    pub bid_levels: Vec<(f64, f64)>,
    /// Ask levels: (price, size), lowest price first.
    pub ask_levels: Vec<(f64, f64)>,
}

impl MarketDepth {
    /// Mid-price between best bid and best ask.
    pub fn mid_price(&self) -> f64 {
        let best_bid = self.bid_levels.first().map(|l| l.0).unwrap_or(0.0);
        let best_ask = self.ask_levels.first().map(|l| l.0).unwrap_or(0.0);
        (best_bid + best_ask) / 2.0
    }

    /// Bid-ask spread (best ask minus best bid).
    pub fn bid_ask_spread(&self) -> f64 {
        let best_bid = self.bid_levels.first().map(|l| l.0).unwrap_or(0.0);
        let best_ask = self.ask_levels.first().map(|l| l.0).unwrap_or(0.0);
        (best_ask - best_bid).max(0.0)
    }

    /// Total bid and ask volume within `bps` basis points of the mid-price.
    ///
    /// Returns `(bid_volume, ask_volume)`.
    pub fn depth_at_bps(&self, bps: f64) -> (f64, f64) {
        let mid = self.mid_price();
        if mid <= 0.0 {
            return (0.0, 0.0);
        }
        let factor = bps / 10_000.0;
        let lower = mid * (1.0 - factor);
        let upper = mid * (1.0 + factor);

        let bid_vol: f64 = self.bid_levels.iter()
            .filter(|(p, _)| *p >= lower)
            .map(|(_, s)| s)
            .sum();
        let ask_vol: f64 = self.ask_levels.iter()
            .filter(|(p, _)| *p <= upper)
            .map(|(_, s)| s)
            .sum();

        (bid_vol, ask_vol)
    }

    /// Estimated market impact cost to execute `quantity` on the given `side`.
    ///
    /// Walks the book and returns the total dollar cost (fills * prices) minus
    /// the cost at mid-price, i.e. the slippage.
    pub fn market_impact_cost(&self, quantity: f64, side: Side) -> f64 {
        let mid = self.mid_price();
        if mid <= 0.0 || quantity <= 0.0 {
            return 0.0;
        }

        let levels: &Vec<(f64, f64)> = match side {
            Side::Buy => &self.ask_levels,
            Side::Sell => &self.bid_levels,
        };

        let mut remaining = quantity;
        let mut total_cost = 0.0;

        for (price, size) in levels {
            if remaining <= 0.0 { break; }
            let fill = remaining.min(*size);
            total_cost += fill * price;
            remaining -= fill;
        }

        // If book is exhausted, charge last-level price for remainder.
        if remaining > 0.0 {
            if let Some((last_price, _)) = levels.last() {
                total_cost += remaining * last_price;
            }
        }

        let mid_cost = quantity * mid;
        match side {
            Side::Buy => total_cost - mid_cost,
            Side::Sell => mid_cost - total_cost,
        }
    }

    /// Resilience score: total volume per basis point of spread.
    ///
    /// Higher is more resilient / liquid.
    pub fn resilience_score(&self) -> f64 {
        let spread = self.bid_ask_spread();
        let mid = self.mid_price();
        if mid <= 0.0 || spread <= 0.0 {
            return 0.0;
        }
        let spread_bps = (spread / mid) * 10_000.0;
        let total_vol: f64 = self.bid_levels.iter().map(|(_, s)| s).sum::<f64>()
            + self.ask_levels.iter().map(|(_, s)| s).sum::<f64>();
        total_vol / spread_bps
    }
}

// ---------------------------------------------------------------------------
// LiquidityRating
// ---------------------------------------------------------------------------

/// Qualitative liquidity rating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiquidityRating {
    /// Composite score >= 80.
    HighlyLiquid,
    /// Composite score in [60, 80).
    Liquid,
    /// Composite score in [40, 60).
    ModeratelyLiquid,
    /// Composite score in [20, 40).
    Illiquid,
    /// Composite score < 20.
    HighlyIlliquid,
}

impl fmt::Display for LiquidityRating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiquidityRating::HighlyLiquid => write!(f, "Highly Liquid"),
            LiquidityRating::Liquid => write!(f, "Liquid"),
            LiquidityRating::ModeratelyLiquid => write!(f, "Moderately Liquid"),
            LiquidityRating::Illiquid => write!(f, "Illiquid"),
            LiquidityRating::HighlyIlliquid => write!(f, "Highly Illiquid"),
        }
    }
}

impl LiquidityRating {
    fn from_score(score: f64) -> Self {
        if score >= 80.0 {
            LiquidityRating::HighlyLiquid
        } else if score >= 60.0 {
            LiquidityRating::Liquid
        } else if score >= 40.0 {
            LiquidityRating::ModeratelyLiquid
        } else if score >= 20.0 {
            LiquidityRating::Illiquid
        } else {
            LiquidityRating::HighlyIlliquid
        }
    }
}

// ---------------------------------------------------------------------------
// LiquidityScore
// ---------------------------------------------------------------------------

/// Composite liquidity score for an asset.
#[derive(Debug, Clone)]
pub struct LiquidityScore {
    /// Asset identifier.
    pub asset: String,
    /// Bid-ask spread in basis points.
    pub bid_ask_spread_bps: f64,
    /// Market depth score (0–100).
    pub market_depth_score: f64,
    /// Turnover ratio (volume / outstanding shares or notional).
    pub turnover_ratio: f64,
    /// Amihud illiquidity ratio.
    pub amihud_illiquidity: f64,
    /// Composite score (0–100).
    pub composite_score: f64,
    /// Qualitative rating.
    pub rating: LiquidityRating,
}

// ---------------------------------------------------------------------------
// AmihudIlliquidity
// ---------------------------------------------------------------------------

/// Amihud (2002) illiquidity ratio: mean(|return| / dollar_volume).
pub struct AmihudIlliquidity;

impl AmihudIlliquidity {
    /// Compute the Amihud illiquidity ratio over the full sample.
    ///
    /// `returns` and `volumes` must have the same length. Zero-volume
    /// observations are skipped to avoid division by zero.
    pub fn compute(returns: &[f64], volumes: &[f64]) -> f64 {
        let n = returns.len().min(volumes.len());
        if n == 0 {
            return 0.0;
        }
        let mut sum = 0.0;
        let mut count = 0u64;
        for i in 0..n {
            if volumes[i] > 0.0 {
                sum += returns[i].abs() / volumes[i];
                count += 1;
            }
        }
        if count == 0 { 0.0 } else { sum / count as f64 }
    }

    /// Rolling Amihud illiquidity with a sliding `window` of observations.
    pub fn rolling(returns: &[f64], volumes: &[f64], window: usize) -> Vec<f64> {
        let n = returns.len().min(volumes.len());
        if n == 0 || window == 0 {
            return Vec::new();
        }
        let mut result = Vec::with_capacity(n.saturating_sub(window) + 1);
        for start in 0..=n.saturating_sub(window) {
            let end = (start + window).min(n);
            let val = Self::compute(&returns[start..end], &volumes[start..end]);
            result.push(val);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// LiquidationCost
// ---------------------------------------------------------------------------

/// Liquidation cost models.
pub struct LiquidationCost;

impl LiquidationCost {
    /// Simple linear liquidation cost model.
    ///
    /// Cost = quantity * (bid_ask_bps/2 + market_impact_bps_per_unit * quantity) / 10_000
    pub fn linear_cost(
        quantity: f64,
        bid_ask_bps: f64,
        market_impact_bps_per_unit: f64,
    ) -> f64 {
        quantity * (bid_ask_bps / 2.0 + market_impact_bps_per_unit * quantity) / 10_000.0
    }

    /// Almgren-Chriss optimal liquidation schedule.
    ///
    /// Returns the number of shares to sell each day over `horizon_days`.
    /// `lambda` is the risk-aversion parameter (higher = faster liquidation).
    ///
    /// The continuous-time AC solution gives an exponential decay rate;
    /// we discretise it into `horizon_days` buckets.
    pub fn optimal_liquidation_schedule(
        quantity: f64,
        adv: f64,
        horizon_days: u32,
        sigma: f64,
        lambda: f64,
    ) -> Vec<f64> {
        if horizon_days == 0 || adv <= 0.0 {
            return Vec::new();
        }
        let t = horizon_days as f64;
        // Almgren-Chriss: kappa = sqrt(lambda * sigma^2 / eta) where we set eta=1
        // For simplicity we use kappa = sqrt(lambda) * sigma as a proxy.
        let kappa = (lambda.max(0.0) * sigma * sigma).sqrt().max(1e-9);
        let mut schedule = Vec::with_capacity(horizon_days as usize);
        let mut remaining = quantity;

        for day in 0..horizon_days {
            let days_left = t - day as f64;
            // AC formula: x(t) = X * sinh(kappa*(T-t)) / sinh(kappa*T)
            let sell_rate = if kappa * t > 700.0 {
                // Large kappa: sell everything immediately
                if day == 0 { quantity } else { 0.0 }
            } else {
                let sinh_remaining = (kappa * days_left).sinh();
                let sinh_total = (kappa * t).sinh();
                if sinh_total < 1e-15 {
                    quantity / t
                } else {
                    quantity * (1.0 - sinh_remaining / sinh_total)
                        - if day == 0 {
                            0.0
                        } else {
                            let days_left_prev = t - (day as f64 - 1.0);
                            quantity * (1.0 - (kappa * days_left_prev).sinh() / sinh_total)
                        }
                }
            };
            let to_sell = sell_rate.max(0.0).min(remaining).min(adv);
            schedule.push(to_sell);
            remaining -= to_sell;
        }

        // Any residual is sold on the last day.
        if remaining > 0.0 {
            if let Some(last) = schedule.last_mut() {
                *last += remaining;
            }
        }

        schedule
    }

    /// VWAP-based liquidation cost estimate (Kissell-Glantz model approximation).
    ///
    /// Cost ≈ 0.5 * spread + market_impact where
    /// market_impact ≈ sigma * sqrt(Q / (ADV * days)).
    pub fn vwap_liquidation_cost(
        quantity: f64,
        daily_volume: f64,
        volatility: f64,
        days: u32,
    ) -> f64 {
        if daily_volume <= 0.0 || days == 0 {
            return 0.0;
        }
        let participation = quantity / (daily_volume * days as f64);
        // Market impact ≈ sigma * sqrt(participation_rate)
        volatility * participation.sqrt() * quantity
    }
}

// ---------------------------------------------------------------------------
// LiquidityRiskEngine
// ---------------------------------------------------------------------------

/// Portfolio-level liquidity risk engine.
pub struct LiquidityRiskEngine;

impl LiquidityRiskEngine {
    /// Score a single asset's liquidity.
    ///
    /// Composite score is a weighted average of:
    /// - Spread component (40%): lower spread → higher score
    /// - Depth component (30%): from `depth.resilience_score()`
    /// - Amihud component (30%): lower illiquidity → higher score
    pub fn score_asset(
        &self,
        asset: &str,
        returns: &[f64],
        volumes: &[f64],
        depth: &MarketDepth,
    ) -> LiquidityScore {
        let mid = depth.mid_price();
        let spread = depth.bid_ask_spread();
        let spread_bps = if mid > 0.0 { (spread / mid) * 10_000.0 } else { 9999.0 };

        // Spread score: 100 at 0 bps, 0 at 100 bps.
        let spread_score = (100.0 - spread_bps).clamp(0.0, 100.0);

        // Depth score: normalise resilience (cap at 100).
        let resilience = depth.resilience_score();
        let depth_score = (resilience * 10.0).clamp(0.0, 100.0);

        // Amihud score: convert to 0-100 (lower illiquidity → higher score).
        let amihud = AmihudIlliquidity::compute(returns, volumes);
        let amihud_score = (100.0 / (1.0 + amihud * 1e6)).clamp(0.0, 100.0);

        // Approximate turnover ratio.
        let avg_volume: f64 = if volumes.is_empty() {
            0.0
        } else {
            volumes.iter().sum::<f64>() / volumes.len() as f64
        };
        let turnover_ratio = avg_volume / (mid * 1e6).max(1.0);

        let composite = 0.4 * spread_score + 0.3 * depth_score + 0.3 * amihud_score;

        LiquidityScore {
            asset: asset.to_string(),
            bid_ask_spread_bps: spread_bps,
            market_depth_score: depth_score,
            turnover_ratio,
            amihud_illiquidity: amihud,
            composite_score: composite,
            rating: LiquidityRating::from_score(composite),
        }
    }

    /// Liquidity-adjusted portfolio VaR.
    ///
    /// Each position contributes a liquidity haircut based on its spread and depth score.
    /// Haircut = position_value * (1 - composite_score/100) * confidence_multiplier.
    pub fn portfolio_liquidity_var(
        positions: &[(f64, LiquidityScore)],
        confidence: f64,
    ) -> f64 {
        // z-score for given confidence (approximate).
        let z = if confidence >= 0.99 { 2.326 } else { 1.645 };
        positions.iter().map(|(value, score)| {
            let haircut = 1.0 - score.composite_score / 100.0;
            value.abs() * haircut * z
        }).sum()
    }

    /// Number of trading days required to liquidate a position.
    ///
    /// `position_value` in currency units, `adv` is average daily volume in same units,
    /// `max_participation` is the maximum fraction of ADV to trade per day (e.g. 0.20).
    pub fn days_to_liquidate(
        position_value: f64,
        adv: f64,
        max_participation: f64,
    ) -> f64 {
        if adv <= 0.0 || max_participation <= 0.0 {
            return f64::INFINITY;
        }
        let daily_capacity = adv * max_participation;
        (position_value / daily_capacity).ceil()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_depth() -> MarketDepth {
        MarketDepth {
            bid_levels: vec![(99.9, 100.0), (99.8, 200.0), (99.7, 300.0)],
            ask_levels: vec![(100.1, 100.0), (100.2, 200.0), (100.3, 300.0)],
        }
    }

    #[test]
    fn mid_and_spread() {
        let d = sample_depth();
        assert!((d.mid_price() - 100.0).abs() < 1e-9);
        assert!((d.bid_ask_spread() - 0.2).abs() < 1e-9);
    }

    #[test]
    fn depth_at_bps() {
        let d = sample_depth();
        let (bv, av) = d.depth_at_bps(10.0); // 10 bps = 0.1%
        assert!(bv > 0.0 && av > 0.0);
    }

    #[test]
    fn market_impact_positive() {
        let d = sample_depth();
        let impact = d.market_impact_cost(50.0, Side::Buy);
        assert!(impact >= 0.0);
    }

    #[test]
    fn amihud_compute() {
        let returns = vec![0.01, -0.02, 0.015];
        let volumes = vec![1_000_000.0, 2_000_000.0, 500_000.0];
        let val = AmihudIlliquidity::compute(&returns, &volumes);
        assert!(val > 0.0);
    }

    #[test]
    fn rolling_amihud_length() {
        let returns: Vec<f64> = (0..20).map(|i| (i as f64) * 0.001).collect();
        let volumes: Vec<f64> = (0..20).map(|_| 1_000_000.0).collect();
        let result = AmihudIlliquidity::rolling(&returns, &volumes, 5);
        assert_eq!(result.len(), 16);
    }

    #[test]
    fn optimal_schedule_sums_to_quantity() {
        let schedule = LiquidationCost::optimal_liquidation_schedule(
            1000.0, 200.0, 5, 0.02, 1e-4,
        );
        let total: f64 = schedule.iter().sum();
        assert!((total - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn days_to_liquidate_basic() {
        let days = LiquidityRiskEngine::days_to_liquidate(1_000_000.0, 500_000.0, 0.2);
        assert!((days - 10.0).abs() < 1e-9);
    }

    #[test]
    fn score_asset_rating() {
        let engine = LiquidityRiskEngine;
        let d = sample_depth();
        let returns = vec![0.001, -0.002, 0.001];
        let volumes = vec![1_000_000.0; 3];
        let score = engine.score_asset("TEST", &returns, &volumes, &d);
        assert!(score.composite_score >= 0.0 && score.composite_score <= 100.0);
    }

    #[test]
    fn liquidity_rating_display() {
        assert_eq!(LiquidityRating::HighlyLiquid.to_string(), "Highly Liquid");
        assert_eq!(LiquidityRating::Illiquid.to_string(), "Illiquid");
    }
}
