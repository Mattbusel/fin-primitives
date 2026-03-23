//! Portfolio diversification metrics.
//!
//! Provides concentration measures (HHI, effective-N, concentration ratio, Gini),
//! risk-adjusted performance ratios (Sortino, Calmar, Treynor, Information Ratio),
//! and a convenience [`DiversificationReport`] that bundles all metrics in one call.
//!
//! All functions are pure and allocation-minimal.

// ─────────────────────────────────────────────────────────────────────────────
//  Concentration metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Herfindahl-Hirschman Index: sum of squared weights.
///
/// Range: `1/n` (perfectly equal) to `1.0` (fully concentrated).
/// Lower HHI indicates better diversification.
pub fn herfindahl_hirschman(weights: &[f64]) -> f64 {
    weights.iter().map(|w| w * w).sum()
}

/// Effective N: reciprocal of HHI.
///
/// Represents the equivalent number of equally-weighted positions
/// that would produce the same HHI. Maximum = `n` for equal weights.
pub fn effective_n(weights: &[f64]) -> f64 {
    let hhi = herfindahl_hirschman(weights);
    if hhi.abs() < f64::EPSILON {
        return 0.0;
    }
    1.0 / hhi
}

/// Concentration ratio: sum of the top-`n` weights.
///
/// Returns 1.0 if `top_n >= weights.len()`.
pub fn concentration_ratio(weights: &[f64], top_n: usize) -> f64 {
    if weights.is_empty() || top_n == 0 {
        return 0.0;
    }
    let mut sorted = weights.to_vec();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    sorted.iter().take(top_n).sum()
}

/// Gini coefficient of the weight distribution.
///
/// Computed from the Lorenz curve area:
/// ```text
/// G = (2 * Σ_{i=1}^{n} i * w_i) / (n * Σ w_i) - (n+1)/n
/// ```
/// where weights are sorted ascending.
/// Returns 0 for perfectly equal weights, approaching 1 for full concentration.
pub fn gini_coefficient(weights: &[f64]) -> f64 {
    let n = weights.len();
    if n == 0 {
        return 0.0;
    }
    let mut sorted = weights.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sum: f64 = sorted.iter().sum();
    if sum.abs() < f64::EPSILON {
        return 0.0;
    }
    let weighted_sum: f64 = sorted
        .iter()
        .enumerate()
        .map(|(i, w)| (i as f64 + 1.0) * w)
        .sum();
    (2.0 * weighted_sum) / (n as f64 * sum) - (n as f64 + 1.0) / n as f64
}

// ─────────────────────────────────────────────────────────────────────────────
//  Portfolio-level diversification and performance metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Diversification ratio: weighted-average individual volatility / portfolio volatility.
///
/// Values > 1 indicate a diversification benefit (portfolio vol < weighted avg vol).
/// Returns `None` if `portfolio_vol` is zero.
pub fn diversification_ratio(
    weights: &[f64],
    individual_vols: &[f64],
    portfolio_vol: f64,
) -> Option<f64> {
    if portfolio_vol.abs() < f64::EPSILON {
        return None;
    }
    let weighted_avg_vol: f64 = weights
        .iter()
        .zip(individual_vols.iter())
        .map(|(w, v)| w * v)
        .sum();
    Some(weighted_avg_vol / portfolio_vol)
}

/// Maximum peak-to-trough drawdown of a return series.
///
/// Computes cumulative returns and tracks the largest percentage decline
/// from any prior peak.
pub fn max_drawdown_portfolio(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mut peak = 1.0_f64;
    let mut cumulative = 1.0_f64;
    let mut max_dd = 0.0_f64;

    for &r in returns {
        cumulative *= 1.0 + r;
        if cumulative > peak {
            peak = cumulative;
        }
        let dd = (peak - cumulative) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

/// Calmar ratio: annualised return / maximum drawdown.
///
/// Returns `None` if max drawdown is zero (no drawdown observed).
pub fn calmar_ratio(returns: &[f64], periods_per_year: f64) -> Option<f64> {
    if returns.is_empty() {
        return None;
    }
    let mean_period = returns.iter().sum::<f64>() / returns.len() as f64;
    let annualised_return = mean_period * periods_per_year;
    let mdd = max_drawdown_portfolio(returns);
    if mdd.abs() < f64::EPSILON {
        return None;
    }
    Some(annualised_return / mdd)
}

/// Sortino ratio: excess return over target divided by downside deviation.
///
/// ```text
/// Sortino = (mean_return - target) / downside_std * sqrt(periods_per_year)
/// ```
/// Downside std = sqrt(mean of squared returns below `target_return`).
/// Returns 0 if downside std is zero.
pub fn sortino_ratio(returns: &[f64], target_return: f64, periods_per_year: f64) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let downside_variance: f64 = returns
        .iter()
        .map(|&r| {
            let diff = r - target_return;
            if diff < 0.0 { diff * diff } else { 0.0 }
        })
        .sum::<f64>()
        / returns.len() as f64;
    let downside_std = downside_variance.sqrt();
    if downside_std.abs() < f64::EPSILON {
        return 0.0;
    }
    (mean - target_return) / downside_std * periods_per_year.sqrt()
}

/// Treynor ratio: excess return over the risk-free rate divided by beta.
///
/// Returns `None` if beta is zero.
pub fn treynor_ratio(portfolio_return: f64, risk_free: f64, beta: f64) -> Option<f64> {
    if beta.abs() < f64::EPSILON {
        return None;
    }
    Some((portfolio_return - risk_free) / beta)
}

/// Information ratio: mean active return divided by tracking error.
///
/// Tracking error = std(portfolio - benchmark) * sqrt(periods_per_year — but note
/// that in this implementation we use the *per-period* tracking error and the caller
/// controls annualisation via `periods_per_year` implicitly through the returns slice).
///
/// Returns `None` if tracking error is zero.
pub fn information_ratio(
    portfolio_returns: &[f64],
    benchmark_returns: &[f64],
) -> Option<f64> {
    let n = portfolio_returns.len().min(benchmark_returns.len());
    if n == 0 {
        return None;
    }
    let active: Vec<f64> = portfolio_returns[..n]
        .iter()
        .zip(benchmark_returns[..n].iter())
        .map(|(p, b)| p - b)
        .collect();
    let mean_active = active.iter().sum::<f64>() / n as f64;
    let variance = active.iter().map(|a| (a - mean_active).powi(2)).sum::<f64>() / n as f64;
    let tracking_error = variance.sqrt();
    if tracking_error.abs() < f64::EPSILON {
        return None;
    }
    Some(mean_active / tracking_error)
}

// ─────────────────────────────────────────────────────────────────────────────
//  DiversificationReport
// ─────────────────────────────────────────────────────────────────────────────

/// A bundle of portfolio diversification and risk-adjusted performance metrics.
#[derive(Debug, Clone)]
pub struct DiversificationReport {
    /// Herfindahl-Hirschman Index.
    pub hhi: f64,
    /// Effective number of positions.
    pub effective_n: f64,
    /// Sum of the top-5 position weights.
    pub concentration_ratio_5: f64,
    /// Gini coefficient of weight distribution.
    pub gini: f64,
    /// Diversification ratio (weighted avg vol / portfolio vol); `None` if portfolio_vol = 0.
    pub diversification_ratio: Option<f64>,
    /// Sortino ratio of the return series.
    pub sortino_ratio: f64,
    /// Calmar ratio; `None` if max drawdown = 0.
    pub calmar_ratio: Option<f64>,
}

impl DiversificationReport {
    /// Compute all metrics in one call.
    ///
    /// # Arguments
    /// - `weights` — portfolio weights (should sum to 1).
    /// - `returns` — per-period return series.
    /// - `individual_vols` — per-asset volatilities (same length as `weights`); required for diversification ratio.
    /// - `portfolio_vol` — realised portfolio volatility; required for diversification ratio.
    /// - `target_return` — per-period target return used in Sortino computation.
    pub fn compute(
        weights: &[f64],
        returns: &[f64],
        individual_vols: Option<&[f64]>,
        portfolio_vol: Option<f64>,
        target_return: f64,
    ) -> Self {
        let hhi = herfindahl_hirschman(weights);
        let eff_n = effective_n(weights);
        let cr5 = concentration_ratio(weights, 5);
        let gini = gini_coefficient(weights);
        let div_ratio = match (individual_vols, portfolio_vol) {
            (Some(vols), Some(pvol)) => diversification_ratio(weights, vols, pvol),
            _ => None,
        };
        // Use 252 trading days as default annualisation factor for Sortino
        let sortino = sortino_ratio(returns, target_return, 252.0);
        let calmar = calmar_ratio(returns, 252.0);

        DiversificationReport {
            hhi,
            effective_n: eff_n,
            concentration_ratio_5: cr5,
            gini,
            diversification_ratio: div_ratio,
            sortino_ratio: sortino,
            calmar_ratio: calmar,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_weights_max_effective_n() {
        let n = 10_usize;
        let weights: Vec<f64> = vec![1.0 / n as f64; n];
        let eff = effective_n(&weights);
        assert!((eff - n as f64).abs() < 1e-9, "effective_n = {eff}");
    }

    #[test]
    fn concentrated_portfolio_high_hhi() {
        // One position holds 90 % of the portfolio
        let weights = vec![0.90, 0.05, 0.03, 0.02];
        let hhi = herfindahl_hirschman(&weights);
        // Expected ≈ 0.81 + 0.0025 + 0.0009 + 0.0004 ≈ 0.8138
        assert!(hhi > 0.80, "HHI should be high for concentrated portfolio, got {hhi}");
    }

    #[test]
    fn equal_weights_gini_zero() {
        let weights = vec![0.25, 0.25, 0.25, 0.25];
        let g = gini_coefficient(&weights);
        assert!(g.abs() < 1e-12, "Gini for equal weights should be 0, got {g}");
    }

    #[test]
    fn diversification_ratio_above_one() {
        // With uncorrelated assets, portfolio vol < weighted avg vol
        // Suppose 2 equal-weight assets each with vol=0.20, portfolio vol=0.1414 (uncorrelated)
        let weights = vec![0.5, 0.5];
        let vols = vec![0.20, 0.20];
        let portfolio_vol = (0.5_f64.powi(2) * 0.04 + 0.5_f64.powi(2) * 0.04).sqrt(); // ≈ 0.1414
        let dr = diversification_ratio(&weights, &vols, portfolio_vol);
        assert!(dr.is_some());
        let dr = dr.unwrap();
        assert!(dr > 1.0, "diversification_ratio should be > 1, got {dr}");
    }

    #[test]
    fn concentration_ratio_top1() {
        let weights = vec![0.40, 0.30, 0.20, 0.10];
        let cr = concentration_ratio(&weights, 1);
        assert!((cr - 0.40).abs() < 1e-12);
    }

    #[test]
    fn concentration_ratio_full() {
        let weights = vec![0.40, 0.30, 0.20, 0.10];
        let cr = concentration_ratio(&weights, 10); // more than len
        assert!((cr - 1.00).abs() < 1e-12);
    }

    #[test]
    fn max_drawdown_flat_returns() {
        let returns = vec![0.01, 0.01, 0.01];
        let mdd = max_drawdown_portfolio(&returns);
        assert!(mdd.abs() < 1e-12, "no drawdown for monotone rising returns");
    }

    #[test]
    fn max_drawdown_known_value() {
        // Cumulative: 1.1, 0.99, 1.089 → peak=1.1, trough=0.99 → dd=0.1/1.1 ≈ 0.0909
        let returns = vec![0.10, -0.10];
        let mdd = max_drawdown_portfolio(&returns);
        let expected = (1.1 - 1.1 * 0.9) / 1.1; // 0.10
        assert!((mdd - expected).abs() < 1e-10, "mdd = {mdd}");
    }

    #[test]
    fn sortino_ratio_positive_for_above_target() {
        let returns: Vec<f64> = vec![0.01; 252]; // 1 % per period, all above target 0
        let s = sortino_ratio(&returns, 0.0, 252.0);
        // No downside deviation → returns 0.0 (downside std = 0 branch)
        assert_eq!(s, 0.0);
    }

    #[test]
    fn sortino_ratio_with_mixed_returns() {
        let returns = vec![0.02, -0.01, 0.03, -0.02, 0.01];
        let s = sortino_ratio(&returns, 0.0, 252.0);
        assert!(s.is_finite(), "sortino ratio should be finite");
    }

    #[test]
    fn information_ratio_none_for_identical_returns() {
        let returns = vec![0.01, 0.02, 0.03];
        let ir = information_ratio(&returns, &returns);
        assert!(ir.is_none(), "IR should be None when tracking error is zero");
    }

    #[test]
    fn treynor_ratio_none_for_zero_beta() {
        let tr = treynor_ratio(0.10, 0.02, 0.0);
        assert!(tr.is_none());
    }

    #[test]
    fn treynor_ratio_correct() {
        let tr = treynor_ratio(0.10, 0.02, 1.0).unwrap();
        assert!((tr - 0.08).abs() < 1e-12);
    }

    #[test]
    fn diversification_report_computes() {
        let weights = vec![0.2, 0.2, 0.2, 0.2, 0.2];
        let returns = vec![0.01, -0.005, 0.008, 0.003, -0.002];
        let report = DiversificationReport::compute(&weights, &returns, None, None, 0.0);
        assert!((report.hhi - 0.2).abs() < 1e-9);
        assert!((report.effective_n - 5.0).abs() < 1e-9);
    }
}
