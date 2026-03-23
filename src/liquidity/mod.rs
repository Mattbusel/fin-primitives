//! # Module: liquidity
//!
//! ## Responsibility
//! Liquidity measures for financial markets: bid-ask spread analysis,
//! market depth, composite liquidity scoring, and Amihud illiquidity.
//!
//! ## Guarantees
//! - Zero panics; all fallible operations return `Result<_, FinError>`
//! - All rolling windows are bounded; no unbounded allocation
//! - `f64` is used intentionally for statistical computations (not prices)

use crate::error::FinError;
use std::collections::VecDeque;

// ─────────────────────────────────────────
//  BidAskSpread
// ─────────────────────────────────────────

/// Instantaneous bid-ask spread with derived metrics.
///
/// # Example
/// ```rust
/// use fin_primitives::liquidity::BidAskSpread;
///
/// let s = BidAskSpread { bid: 99.90, ask: 100.10 };
/// assert!((s.spread() - 0.20).abs() < 1e-10);
/// assert!((s.mid_price() - 100.0).abs() < 1e-10);
/// assert!((s.relative_spread() - 0.002).abs() < 1e-10);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BidAskSpread {
    /// Best bid price.
    pub bid: f64,
    /// Best ask price.
    pub ask: f64,
}

impl BidAskSpread {
    /// Constructs a new [`BidAskSpread`].
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `bid >= ask` or either is non-finite.
    pub fn new(bid: f64, ask: f64) -> Result<Self, FinError> {
        if !bid.is_finite() || !ask.is_finite() {
            return Err(FinError::InvalidInput("bid and ask must be finite".into()));
        }
        if bid >= ask {
            return Err(FinError::InvalidInput(
                "bid must be strictly less than ask".into(),
            ));
        }
        Ok(Self { bid, ask })
    }

    /// Absolute spread: `ask - bid`.
    #[must_use]
    pub fn spread(&self) -> f64 {
        self.ask - self.bid
    }

    /// Mid-price: `(bid + ask) / 2`.
    #[must_use]
    pub fn mid_price(&self) -> f64 {
        (self.bid + self.ask) / 2.0
    }

    /// Relative spread: `spread / mid_price`.
    ///
    /// Returns `0.0` when mid-price is zero to avoid division by zero.
    #[must_use]
    pub fn relative_spread(&self) -> f64 {
        let mid = self.mid_price();
        if mid == 0.0 {
            0.0
        } else {
            self.spread() / mid
        }
    }
}

// ─────────────────────────────────────────
//  MarketDepth
// ─────────────────────────────────────────

/// Order-book depth: a collection of (price, quantity) levels.
///
/// Levels are stored in the order provided — typically ascending for asks,
/// descending for bids. The caller is responsible for ordering.
///
/// # Example
/// ```rust
/// use fin_primitives::liquidity::MarketDepth;
///
/// let depth = MarketDepth {
///     levels: vec![(100.0, 10.0), (101.0, 5.0), (102.0, 3.0)],
/// };
/// // Cumulative qty up to and including price 101.0
/// assert!((depth.depth_at_price(101.0) - 15.0).abs() < 1e-10);
/// ```
#[derive(Debug, Clone)]
pub struct MarketDepth {
    /// (price, quantity) pairs. Quantity must be non-negative.
    pub levels: Vec<(f64, f64)>,
}

impl MarketDepth {
    /// Cumulative quantity at or below `price`.
    ///
    /// Sums quantities for all levels whose price is `<= price`.
    #[must_use]
    pub fn depth_at_price(&self, price: f64) -> f64 {
        self.levels
            .iter()
            .filter(|(p, _)| *p <= price)
            .map(|(_, q)| q)
            .sum()
    }

    /// Volume-weighted average price across all depth levels.
    ///
    /// Returns `None` if there are no levels or total quantity is zero.
    #[must_use]
    pub fn weighted_mid_price(&self) -> Option<f64> {
        let total_qty: f64 = self.levels.iter().map(|(_, q)| q).sum();
        if total_qty == 0.0 || self.levels.is_empty() {
            return None;
        }
        let vwap: f64 = self
            .levels
            .iter()
            .map(|(p, q)| p * q)
            .sum::<f64>()
            / total_qty;
        Some(vwap)
    }

    /// Total quantity across all levels.
    #[must_use]
    pub fn total_quantity(&self) -> f64 {
        self.levels.iter().map(|(_, q)| q).sum()
    }
}

// ─────────────────────────────────────────
//  LiquidityScore
// ─────────────────────────────────────────

/// Composite liquidity score combining spread, depth, and turnover components.
///
/// Each sub-score is in `[0.0, 1.0]`; higher is more liquid.
/// `combined_score` is an equal-weight average of the three.
///
/// # Example
/// ```rust
/// use fin_primitives::liquidity::LiquidityScore;
///
/// let score = LiquidityScore::new(0.8, 0.6, 0.7);
/// assert!((score.combined_score() - 0.7).abs() < 1e-10);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct LiquidityScore {
    /// Score derived from bid-ask spread tightness (1 = very tight spread).
    pub spread_score: f64,
    /// Score derived from order-book depth (1 = deep book).
    pub depth_score: f64,
    /// Score derived from trading turnover / volume (1 = high turnover).
    pub turnover_score: f64,
}

impl LiquidityScore {
    /// Constructs a [`LiquidityScore`].
    ///
    /// All inputs are clamped to `[0.0, 1.0]`.
    #[must_use]
    pub fn new(spread_score: f64, depth_score: f64, turnover_score: f64) -> Self {
        Self {
            spread_score: spread_score.clamp(0.0, 1.0),
            depth_score: depth_score.clamp(0.0, 1.0),
            turnover_score: turnover_score.clamp(0.0, 1.0),
        }
    }

    /// Equal-weight combined score: `(spread + depth + turnover) / 3`.
    #[must_use]
    pub fn combined_score(&self) -> f64 {
        (self.spread_score + self.depth_score + self.turnover_score) / 3.0
    }

    /// Build a [`LiquidityScore`] from raw market observables.
    ///
    /// - `spread_bps`: bid-ask spread in basis points; lower is better.
    ///   Score = `exp(-spread_bps / 10.0)`.
    /// - `depth`: total depth quantity; scored as `1 - exp(-depth / reference_depth)`.
    /// - `turnover`: daily dollar turnover; scored as `1 - exp(-turnover / reference_turnover)`.
    ///
    /// `reference_depth` and `reference_turnover` are the "typical" values
    /// for the instrument — the scores decay exponentially away from them.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if any reference value is `<= 0.0`.
    pub fn from_observables(
        spread_bps: f64,
        depth: f64,
        turnover: f64,
        reference_depth: f64,
        reference_turnover: f64,
    ) -> Result<Self, FinError> {
        if reference_depth <= 0.0 || reference_turnover <= 0.0 {
            return Err(FinError::InvalidInput(
                "reference_depth and reference_turnover must be positive".into(),
            ));
        }
        let spread_score = (-spread_bps / 10.0).exp();
        let depth_score = 1.0 - (-depth / reference_depth).exp();
        let turnover_score = 1.0 - (-turnover / reference_turnover).exp();
        Ok(Self::new(spread_score, depth_score, turnover_score))
    }
}

// ─────────────────────────────────────────
//  AmihudIlliquidity
// ─────────────────────────────────────────

/// Amihud (2002) illiquidity measure: rolling average of `|return| / dollar_volume`.
///
/// Higher values indicate less liquidity (larger price impact per dollar traded).
///
/// # Example
/// ```rust
/// use fin_primitives::liquidity::AmihudIlliquidity;
///
/// let mut amihud = AmihudIlliquidity::new(3).unwrap();
/// amihud.update(0.01, 1_000_000.0);
/// amihud.update(0.005, 500_000.0);
/// amihud.update(0.02, 2_000_000.0);
/// let ratio = amihud.average().unwrap();
/// assert!(ratio > 0.0);
/// ```
#[derive(Debug)]
pub struct AmihudIlliquidity {
    window: usize,
    buf: VecDeque<f64>,
}

impl AmihudIlliquidity {
    /// Constructs an [`AmihudIlliquidity`] tracker with a rolling `window`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `window == 0`.
    pub fn new(window: usize) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        Ok(Self {
            window,
            buf: VecDeque::with_capacity(window),
        })
    }

    /// Add an observation.
    ///
    /// - `abs_return`: absolute value of the period return `|r_t|`.
    /// - `dollar_volume`: dollar trading volume for the period.
    ///
    /// If `dollar_volume <= 0` the observation is skipped (no valid ratio).
    pub fn update(&mut self, abs_return: f64, dollar_volume: f64) {
        if dollar_volume <= 0.0 {
            return;
        }
        let ratio = abs_return / dollar_volume;
        if self.buf.len() == self.window {
            self.buf.pop_front();
        }
        self.buf.push_back(ratio);
    }

    /// Rolling average of `|return| / dollar_volume`.
    ///
    /// Returns `None` until the window is full.
    #[must_use]
    pub fn average(&self) -> Option<f64> {
        if self.buf.len() < self.window {
            return None;
        }
        let sum: f64 = self.buf.iter().sum();
        Some(sum / self.buf.len() as f64)
    }

    /// Current number of observations in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` if no observations have been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

// ─────────────────────────────────────────
//  Unit Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // BidAskSpread tests
    #[test]
    fn bid_ask_spread_basic() {
        let s = BidAskSpread::new(99.90, 100.10).unwrap();
        assert!((s.spread() - 0.20).abs() < 1e-10);
        assert!((s.mid_price() - 100.0).abs() < 1e-10);
        assert!((s.relative_spread() - 0.002).abs() < 1e-10);
    }

    #[test]
    fn bid_ask_spread_rejects_inverted() {
        assert!(BidAskSpread::new(100.0, 99.0).is_err());
        assert!(BidAskSpread::new(100.0, 100.0).is_err());
    }

    #[test]
    fn bid_ask_spread_rejects_non_finite() {
        assert!(BidAskSpread::new(f64::NAN, 100.0).is_err());
        assert!(BidAskSpread::new(100.0, f64::INFINITY).is_err());
    }

    // MarketDepth tests
    #[test]
    fn market_depth_at_price() {
        let d = MarketDepth {
            levels: vec![(100.0, 10.0), (101.0, 5.0), (102.0, 3.0)],
        };
        assert!((d.depth_at_price(100.0) - 10.0).abs() < 1e-10);
        assert!((d.depth_at_price(101.0) - 15.0).abs() < 1e-10);
        assert!((d.depth_at_price(102.0) - 18.0).abs() < 1e-10);
        assert!((d.depth_at_price(99.0) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn market_depth_weighted_mid_price() {
        // Levels: (100, 2), (102, 2) → VWAP = (200 + 204) / 4 = 101
        let d = MarketDepth {
            levels: vec![(100.0, 2.0), (102.0, 2.0)],
        };
        let vwap = d.weighted_mid_price().unwrap();
        assert!((vwap - 101.0).abs() < 1e-10);
    }

    #[test]
    fn market_depth_weighted_mid_price_empty() {
        let d = MarketDepth { levels: vec![] };
        assert!(d.weighted_mid_price().is_none());
    }

    // LiquidityScore tests
    #[test]
    fn liquidity_score_combined() {
        let s = LiquidityScore::new(0.8, 0.6, 0.7);
        assert!((s.combined_score() - 0.7).abs() < 1e-10);
    }

    #[test]
    fn liquidity_score_clamped() {
        let s = LiquidityScore::new(1.5, -0.1, 0.5);
        assert!(s.spread_score <= 1.0);
        assert!(s.depth_score >= 0.0);
    }

    #[test]
    fn liquidity_score_from_observables() {
        let s = LiquidityScore::from_observables(5.0, 1_000.0, 1_000_000.0, 1_000.0, 1_000_000.0)
            .unwrap();
        assert!(s.combined_score() > 0.0);
        assert!(s.combined_score() <= 1.0);
    }

    #[test]
    fn liquidity_score_from_observables_invalid_reference() {
        assert!(
            LiquidityScore::from_observables(5.0, 1000.0, 1_000_000.0, 0.0, 1_000_000.0).is_err()
        );
    }

    // AmihudIlliquidity tests
    #[test]
    fn amihud_window_zero_rejected() {
        assert!(AmihudIlliquidity::new(0).is_err());
    }

    #[test]
    fn amihud_not_ready_until_full() {
        let mut a = AmihudIlliquidity::new(3).unwrap();
        a.update(0.01, 1_000_000.0);
        assert!(a.average().is_none());
        a.update(0.005, 500_000.0);
        assert!(a.average().is_none());
        a.update(0.02, 2_000_000.0);
        assert!(a.average().is_some());
    }

    #[test]
    fn amihud_rolling_window() {
        let mut a = AmihudIlliquidity::new(2).unwrap();
        // ratio 1 = 0.01 / 1_000_000
        a.update(0.01, 1_000_000.0);
        // ratio 2 = 0.01 / 1_000_000
        a.update(0.01, 1_000_000.0);
        let avg1 = a.average().unwrap();
        // ratio 3 = 0.02 / 1_000_000 → window now [ratio2, ratio3]
        a.update(0.02, 1_000_000.0);
        let avg2 = a.average().unwrap();
        assert!(avg2 > avg1);
    }

    #[test]
    fn amihud_skips_zero_volume() {
        let mut a = AmihudIlliquidity::new(1).unwrap();
        a.update(0.01, 0.0); // skipped
        assert!(a.average().is_none());
        a.update(0.01, 1_000.0);
        assert!(a.average().is_some());
    }
}
