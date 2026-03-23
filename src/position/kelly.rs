//! Kelly Criterion position sizing.
//!
//! Provides single-asset full/fractional Kelly and a multi-asset
//! [`KellyPortfolio`] allocator that applies a correlation penalty.

use crate::portfolio::optimizer::CovarianceMatrix;

/// Inputs required to compute a Kelly fraction for a single bet.
#[derive(Debug, Clone)]
pub struct KellyInput {
    /// Probability of a winning outcome (in `[0, 1]`).
    pub win_probability: f64,
    /// Return per unit staked on a win (odds, e.g. `2.0` = double money).
    pub win_return: f64,
    /// Fractional loss per unit staked on a loss (positive, e.g. `1.0` = lose all).
    pub loss_return: f64,
    /// Total bankroll / account equity in currency units.
    pub bankroll: f64,
}

/// The result of a Kelly calculation.
#[derive(Debug, Clone)]
pub struct KellyResult {
    /// Optimal fraction of bankroll to wager (`0.0` when edge is negative).
    pub fraction: f64,
    /// Position size in currency units (`fraction * bankroll`).
    pub position_size_usd: f64,
    /// Maximum loss on this position (`position_size_usd * loss_return`).
    pub max_loss_usd: f64,
    /// Expected logarithmic growth rate of wealth per bet.
    pub expected_growth: f64,
}

/// Compute the full Kelly fraction.
///
/// Classic formula: `f* = (b*p - q) / b`
/// where `b = win_return`, `p = win_probability`, `q = 1 - p`.
///
/// Returns `0.0` when the edge is non-positive (i.e. `b*p - q <= 0`).
pub fn full_kelly(input: &KellyInput) -> KellyResult {
    let b = input.win_return;
    let p = input.win_probability.clamp(0.0, 1.0);
    let q = 1.0 - p;

    let edge = b * p - q;
    let fraction = if edge <= 0.0 || b <= 0.0 {
        0.0
    } else {
        (edge / b).clamp(0.0, 1.0)
    };

    build_result(input, fraction)
}

/// Compute a fractional Kelly fraction.
///
/// Applies `fraction * full_kelly` to reduce variance at the cost of lower
/// expected growth. A `fraction` of `0.5` is the "half-Kelly" strategy.
///
/// The `fraction` parameter is clamped to `[0, 1]`.
pub fn fractional_kelly(input: &KellyInput, fraction: f64) -> KellyResult {
    let full = full_kelly(input);
    let f = fraction.clamp(0.0, 1.0);
    let adjusted = full.fraction * f;
    build_result(input, adjusted)
}

fn build_result(input: &KellyInput, fraction: f64) -> KellyResult {
    let position_size_usd = fraction * input.bankroll;
    let max_loss_usd = position_size_usd * input.loss_return;

    // Expected log growth: p * ln(1 + b*f) + q * ln(1 - f)
    let p = input.win_probability.clamp(0.0, 1.0);
    let q = 1.0 - p;
    let b = input.win_return;
    let expected_growth = if fraction <= 0.0 {
        0.0
    } else {
        let win_term = 1.0 + b * fraction;
        let loss_term = 1.0 - fraction * input.loss_return;
        if win_term <= 0.0 || loss_term <= 0.0 {
            f64::NEG_INFINITY
        } else {
            p * win_term.ln() + q * loss_term.ln()
        }
    };

    KellyResult {
        fraction,
        position_size_usd,
        max_loss_usd,
        expected_growth,
    }
}

/// Multi-asset Kelly portfolio allocator.
///
/// Computes per-asset full Kelly fractions, then applies a correlation penalty
/// by scaling each fraction down proportionally to the portfolio's off-diagonal
/// covariance contribution, and finally normalises so the total allocation does
/// not exceed `max_total`.
pub struct KellyPortfolio;

impl KellyPortfolio {
    /// Allocate Kelly fractions across multiple assets.
    ///
    /// # Arguments
    ///
    /// * `assets` — per-asset Kelly inputs (must match the dimension of `correlations`)
    /// * `correlations` — covariance/correlation matrix; off-diagonal elements
    ///   represent pairwise correlation and are used to penalise over-concentration
    /// * `max_total` — maximum total fraction of bankroll that may be allocated
    ///   (e.g. `1.0` means fully invested; `0.5` means half-Kelly portfolio-wide)
    ///
    /// Returns a `Vec<f64>` of allocated fractions (one per asset, same order as
    /// `assets`).  All values are in `[0, max_total]` and their sum `<= max_total`.
    pub fn allocate(
        assets: &[KellyInput],
        correlations: &CovarianceMatrix,
        max_total: f64,
    ) -> Vec<f64> {
        let n = assets.len();
        if n == 0 {
            return vec![];
        }

        // Step 1: individual full-Kelly fractions.
        let raw: Vec<f64> = assets.iter().map(|a| full_kelly(a).fraction).collect();

        // Step 2: correlation penalty.
        // penalty_i = sum_{j != i} |corr(i,j)| * raw_j
        // penalised_i = raw_i / (1 + penalty_i)
        let mut penalised: Vec<f64> = (0..n)
            .map(|i| {
                let penalty: f64 = (0..n)
                    .filter(|&j| j != i)
                    .map(|j| correlations.get(i, j).abs() * raw[j])
                    .sum();
                raw[i] / (1.0 + penalty)
            })
            .collect();

        // Step 3: scale so total <= max_total.
        let total: f64 = penalised.iter().sum();
        if total > max_total && total > 0.0 {
            let scale = max_total / total;
            for f in penalised.iter_mut() {
                *f *= scale;
            }
        }

        penalised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btc_input() -> KellyInput {
        KellyInput {
            win_probability: 0.6,
            win_return: 1.0,   // win = +100 %
            loss_return: 1.0,  // lose = -100 %
            bankroll: 10_000.0,
        }
    }

    // --- full_kelly ---

    #[test]
    fn full_kelly_classic_example() {
        // f* = (1*0.6 - 0.4) / 1 = 0.2
        let r = full_kelly(&btc_input());
        assert!((r.fraction - 0.20).abs() < 1e-10);
    }

    #[test]
    fn full_kelly_position_size_usd() {
        let r = full_kelly(&btc_input());
        assert!((r.position_size_usd - 2_000.0).abs() < 1e-6);
    }

    #[test]
    fn full_kelly_max_loss_usd() {
        let r = full_kelly(&btc_input());
        // max_loss = position_size * loss_return = 2000 * 1.0
        assert!((r.max_loss_usd - 2_000.0).abs() < 1e-6);
    }

    #[test]
    fn full_kelly_negative_edge_returns_zero() {
        let input = KellyInput {
            win_probability: 0.3,
            win_return: 1.0,
            loss_return: 1.0,
            bankroll: 10_000.0,
        };
        let r = full_kelly(&input);
        assert_eq!(r.fraction, 0.0);
        assert_eq!(r.position_size_usd, 0.0);
    }

    #[test]
    fn full_kelly_zero_edge_returns_zero() {
        let input = KellyInput {
            win_probability: 0.5,
            win_return: 1.0,
            loss_return: 1.0,
            bankroll: 5_000.0,
        };
        let r = full_kelly(&input);
        assert_eq!(r.fraction, 0.0);
    }

    #[test]
    fn full_kelly_expected_growth_positive_edge() {
        let r = full_kelly(&btc_input());
        assert!(r.expected_growth > 0.0, "growth = {}", r.expected_growth);
    }

    #[test]
    fn full_kelly_expected_growth_zero_fraction() {
        let input = KellyInput {
            win_probability: 0.3,
            win_return: 1.0,
            loss_return: 1.0,
            bankroll: 10_000.0,
        };
        let r = full_kelly(&input);
        assert_eq!(r.expected_growth, 0.0);
    }

    #[test]
    fn full_kelly_high_odds_asset() {
        // b=4 (4:1 odds), p=0.3 → f* = (4*0.3 - 0.7)/4 = 0.5/4 = 0.125
        let input = KellyInput {
            win_probability: 0.3,
            win_return: 4.0,
            loss_return: 1.0,
            bankroll: 1_000.0,
        };
        let r = full_kelly(&input);
        assert!((r.fraction - 0.125).abs() < 1e-10);
    }

    // --- fractional_kelly ---

    #[test]
    fn half_kelly_is_half_of_full() {
        let full = full_kelly(&btc_input()).fraction;
        let half = fractional_kelly(&btc_input(), 0.5).fraction;
        assert!((half - full * 0.5).abs() < 1e-10);
    }

    #[test]
    fn fractional_kelly_zero_fraction_gives_zero() {
        let r = fractional_kelly(&btc_input(), 0.0);
        assert_eq!(r.fraction, 0.0);
        assert_eq!(r.position_size_usd, 0.0);
    }

    #[test]
    fn fractional_kelly_one_fraction_equals_full() {
        let full = full_kelly(&btc_input()).fraction;
        let frac = fractional_kelly(&btc_input(), 1.0).fraction;
        assert!((frac - full).abs() < 1e-10);
    }

    #[test]
    fn fractional_kelly_fraction_clamped_above_one() {
        let full = full_kelly(&btc_input()).fraction;
        let frac = fractional_kelly(&btc_input(), 2.0).fraction; // clamps to 1.0
        assert!((frac - full).abs() < 1e-10);
    }

    #[test]
    fn fractional_kelly_negative_edge_still_zero() {
        let input = KellyInput {
            win_probability: 0.3,
            win_return: 1.0,
            loss_return: 1.0,
            bankroll: 10_000.0,
        };
        let r = fractional_kelly(&input, 0.5);
        assert_eq!(r.fraction, 0.0);
    }

    // --- KellyPortfolio ---

    #[test]
    fn portfolio_allocate_single_asset() {
        let assets = vec![btc_input()];
        let cov = CovarianceMatrix::new(vec!["BTC".into()]);
        let alloc = KellyPortfolio::allocate(&assets, &cov, 1.0);
        assert_eq!(alloc.len(), 1);
        // Single asset: no cross-penalty, full Kelly = 0.20.
        assert!((alloc[0] - 0.20).abs() < 1e-10);
    }

    #[test]
    fn portfolio_allocate_total_below_max() {
        let assets = vec![
            btc_input(),
            KellyInput { win_probability: 0.55, win_return: 1.0, loss_return: 1.0, bankroll: 10_000.0 },
        ];
        let cov = CovarianceMatrix::new(vec!["BTC".into(), "ETH".into()]);
        let alloc = KellyPortfolio::allocate(&assets, &cov, 0.5);
        let total: f64 = alloc.iter().sum();
        assert!(total <= 0.5 + 1e-10, "total = {total}");
    }

    #[test]
    fn portfolio_allocate_correlated_assets_penalised() {
        let a = KellyInput { win_probability: 0.6, win_return: 1.0, loss_return: 1.0, bankroll: 10_000.0 };
        let b = a.clone();
        let mut cov = CovarianceMatrix::new(vec!["A".into(), "B".into()]);
        // High positive correlation → penalty.
        cov.set(0, 1, 0.9);
        let alloc = KellyPortfolio::allocate(&[a.clone(), b], &cov, 1.0);
        let uncorrelated_alloc = KellyPortfolio::allocate(&[a], &CovarianceMatrix::new(vec!["A".into()]), 1.0);
        // With high correlation, individual allocations should be lower.
        assert!(alloc[0] < uncorrelated_alloc[0] + 1e-10);
    }

    #[test]
    fn portfolio_allocate_empty_returns_empty() {
        let cov = CovarianceMatrix::new(vec![]);
        let alloc = KellyPortfolio::allocate(&[], &cov, 1.0);
        assert!(alloc.is_empty());
    }

    #[test]
    fn portfolio_allocate_negative_edge_asset_gets_zero() {
        let bad = KellyInput { win_probability: 0.3, win_return: 1.0, loss_return: 1.0, bankroll: 10_000.0 };
        let good = btc_input();
        let cov = CovarianceMatrix::new(vec!["BAD".into(), "BTC".into()]);
        let alloc = KellyPortfolio::allocate(&[bad, good], &cov, 1.0);
        assert_eq!(alloc[0], 0.0, "bad asset should get zero allocation");
        assert!(alloc[1] > 0.0, "good asset should get positive allocation");
    }
}
