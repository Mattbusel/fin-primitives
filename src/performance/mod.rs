//! Portfolio performance metrics.
//!
//! Provides [`PerformanceMetrics`] and [`PerformanceCalculator`] for computing
//! risk-adjusted return metrics including Sharpe, Sortino, Calmar, Omega,
//! Information Ratio, Max Drawdown, and CAGR.

/// Aggregate portfolio performance metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct PerformanceMetrics {
    /// Sharpe ratio: excess return per unit of total volatility (annualised).
    pub sharpe_ratio: f64,
    /// Sortino ratio: excess return per unit of downside deviation (annualised).
    pub sortino_ratio: f64,
    /// Calmar ratio: annualised return divided by maximum drawdown.
    pub calmar_ratio: f64,
    /// Omega ratio: probability-weighted gains above threshold vs losses below.
    pub omega_ratio: f64,
    /// Information ratio: active return divided by tracking error.
    pub information_ratio: f64,
    /// Maximum peak-to-trough decline on the cumulative return series.
    pub max_drawdown: f64,
    /// Compound annual growth rate.
    pub cagr: f64,
}

/// Stateless calculator for portfolio performance metrics.
pub struct PerformanceCalculator;

impl PerformanceCalculator {
    /// Compute the annualised Sharpe ratio.
    ///
    /// `(mean_return - risk_free_rate) / std_return * sqrt(252)`
    ///
    /// Returns `0.0` if `returns` has fewer than 2 elements or std dev is zero.
    pub fn sharpe_ratio(returns: &[f64], risk_free_rate: f64) -> f64 {
        if returns.len() < 2 {
            return 0.0;
        }
        let mean = mean(returns);
        let std = std_dev(returns);
        if std == 0.0 {
            return 0.0;
        }
        (mean - risk_free_rate) / std * 252_f64.sqrt()
    }

    /// Compute the annualised Sortino ratio using downside deviation only.
    ///
    /// Downside deviation is computed over returns below `target`.
    ///
    /// Returns `0.0` if `returns` is empty or downside deviation is zero.
    pub fn sortino_ratio(returns: &[f64], risk_free_rate: f64, target: f64) -> f64 {
        if returns.is_empty() {
            return 0.0;
        }
        let mean = mean(returns);
        let downside_var: f64 = returns
            .iter()
            .map(|&r| {
                let diff = r - target;
                if diff < 0.0 { diff * diff } else { 0.0 }
            })
            .sum::<f64>()
            / returns.len() as f64;
        let downside_dev = downside_var.sqrt();
        if downside_dev == 0.0 {
            return 0.0;
        }
        (mean - risk_free_rate) / downside_dev * 252_f64.sqrt()
    }

    /// Compute the Calmar ratio: annualised return / |max_drawdown|.
    ///
    /// Uses `252` periods per year.  Returns `0.0` if `returns` is empty or
    /// max drawdown is zero.
    pub fn calmar_ratio(returns: &[f64]) -> f64 {
        if returns.is_empty() {
            return 0.0;
        }
        let ann_return = Self::cagr(returns, 252.0);
        let md = Self::max_drawdown(returns);
        if md == 0.0 {
            return 0.0;
        }
        ann_return / md.abs()
    }

    /// Compute the Omega ratio at the given `threshold`.
    ///
    /// `sum(max(r - threshold, 0)) / sum(max(threshold - r, 0))`
    ///
    /// Returns `f64::INFINITY` if the loss sum is zero and there are gains,
    /// or `0.0` if both sums are zero.
    pub fn omega_ratio(returns: &[f64], threshold: f64) -> f64 {
        let gains: f64 = returns.iter().map(|&r| (r - threshold).max(0.0)).sum();
        let losses: f64 = returns.iter().map(|&r| (threshold - r).max(0.0)).sum();
        if losses == 0.0 {
            if gains > 0.0 { f64::INFINITY } else { 0.0 }
        } else {
            gains / losses
        }
    }

    /// Compute the Information Ratio: active return / tracking error.
    ///
    /// Active return = mean(returns - benchmark_returns).
    /// Tracking error = std dev of (returns - benchmark_returns).
    ///
    /// Returns `0.0` if lengths differ, fewer than 2 observations, or
    /// tracking error is zero.
    pub fn information_ratio(returns: &[f64], benchmark_returns: &[f64]) -> f64 {
        if returns.len() != benchmark_returns.len() || returns.len() < 2 {
            return 0.0;
        }
        let active: Vec<f64> = returns
            .iter()
            .zip(benchmark_returns.iter())
            .map(|(&r, &b)| r - b)
            .collect();
        let mean_active = mean(&active);
        let te = std_dev(&active);
        if te == 0.0 {
            return 0.0;
        }
        mean_active / te
    }

    /// Compute the maximum peak-to-trough drawdown on the cumulative return series.
    ///
    /// Cumulative returns are computed as product of `(1 + r)` factors.
    /// Returns a positive value representing the magnitude of the worst decline,
    /// or `0.0` if `returns` is empty.
    pub fn max_drawdown(returns: &[f64]) -> f64 {
        if returns.is_empty() {
            return 0.0;
        }
        let mut peak = 1.0_f64;
        let mut cum = 1.0_f64;
        let mut max_dd = 0.0_f64;
        for &r in returns {
            cum *= 1.0 + r;
            if cum > peak {
                peak = cum;
            }
            let dd = (peak - cum) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
        max_dd
    }

    /// Compute the Compound Annual Growth Rate.
    ///
    /// `(product(1 + r))^(periods_per_year / n) - 1`
    ///
    /// Returns `0.0` if `returns` is empty.
    pub fn cagr(returns: &[f64], periods_per_year: f64) -> f64 {
        if returns.is_empty() {
            return 0.0;
        }
        let n = returns.len() as f64;
        let total: f64 = returns.iter().fold(1.0, |acc, &r| acc * (1.0 + r));
        total.powf(periods_per_year / n) - 1.0
    }

    /// Compute all performance metrics in one pass.
    ///
    /// If `benchmark` is `None` the information ratio is set to `0.0`.
    /// `risk_free_rate` is a per-period (not annualised) rate, matching the
    /// frequency of `returns`.
    pub fn compute_all(
        returns: &[f64],
        benchmark: Option<&[f64]>,
        risk_free_rate: f64,
    ) -> PerformanceMetrics {
        let sharpe_ratio = Self::sharpe_ratio(returns, risk_free_rate);
        let sortino_ratio = Self::sortino_ratio(returns, risk_free_rate, risk_free_rate);
        let calmar_ratio = Self::calmar_ratio(returns);
        let omega_ratio = Self::omega_ratio(returns, risk_free_rate);
        let information_ratio = benchmark
            .map(|b| Self::information_ratio(returns, b))
            .unwrap_or(0.0);
        let max_drawdown = Self::max_drawdown(returns);
        let cagr = Self::cagr(returns, 252.0);

        PerformanceMetrics {
            sharpe_ratio,
            sortino_ratio,
            calmar_ratio,
            omega_ratio,
            information_ratio,
            max_drawdown,
            cagr,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sample standard deviation (n-1 denominator).
fn std_dev(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let var = xs.iter().map(|&x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
    var.sqrt()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple daily returns: +1 %, -0.5 %, +2 %, -1 %, +1.5 %
    fn sample_returns() -> Vec<f64> {
        vec![0.01, -0.005, 0.02, -0.01, 0.015]
    }

    #[test]
    fn test_sharpe_positive_returns() {
        let r = sample_returns();
        let s = PerformanceCalculator::sharpe_ratio(&r, 0.0);
        // Mean is positive, std > 0, result should be positive
        assert!(s > 0.0, "Sharpe should be positive for net-positive returns");
    }

    #[test]
    fn test_sharpe_empty() {
        assert_eq!(PerformanceCalculator::sharpe_ratio(&[], 0.0), 0.0);
    }

    #[test]
    fn test_sharpe_single_element() {
        assert_eq!(PerformanceCalculator::sharpe_ratio(&[0.01], 0.0), 0.0);
    }

    #[test]
    fn test_sharpe_zero_std() {
        // All identical returns → std == 0
        let r = vec![0.01, 0.01, 0.01];
        assert_eq!(PerformanceCalculator::sharpe_ratio(&r, 0.0), 0.0);
    }

    #[test]
    fn test_sortino_positive() {
        let r = sample_returns();
        let s = PerformanceCalculator::sortino_ratio(&r, 0.0, 0.0);
        assert!(s > 0.0);
    }

    #[test]
    fn test_sortino_empty() {
        assert_eq!(PerformanceCalculator::sortino_ratio(&[], 0.0, 0.0), 0.0);
    }

    #[test]
    fn test_sortino_no_downside() {
        // All returns > target → downside dev == 0 → 0.0
        let r = vec![0.01, 0.02, 0.03];
        assert_eq!(PerformanceCalculator::sortino_ratio(&r, 0.0, 0.0), 0.0);
    }

    #[test]
    fn test_max_drawdown_known() {
        // Cumulative: 1.10, 1.05, 1.20, 0.96, 1.20
        // Peak at 1.20, trough at 0.96 → dd = (1.20-0.96)/1.20 = 0.20
        let r = vec![0.10, -0.045_454, 0.142_857, -0.20, 0.25];
        let md = PerformanceCalculator::max_drawdown(&r);
        assert!(md > 0.0 && md < 1.0, "Max drawdown should be (0, 1)");
    }

    #[test]
    fn test_max_drawdown_monotone_up() {
        let r = vec![0.01, 0.02, 0.03];
        assert_eq!(PerformanceCalculator::max_drawdown(&r), 0.0);
    }

    #[test]
    fn test_max_drawdown_empty() {
        assert_eq!(PerformanceCalculator::max_drawdown(&[]), 0.0);
    }

    #[test]
    fn test_cagr_flat() {
        // All-zero returns → CAGR = 0
        let r = vec![0.0; 252];
        let cagr = PerformanceCalculator::cagr(&r, 252.0);
        assert!((cagr).abs() < 1e-10);
    }

    #[test]
    fn test_cagr_known() {
        // 252 daily returns of 0.001 → ~+28.% annualised
        let r = vec![0.001; 252];
        let cagr = PerformanceCalculator::cagr(&r, 252.0);
        assert!(cagr > 0.0);
        // (1.001)^252 - 1 ≈ 0.2848
        let expected = 1.001_f64.powi(252) - 1.0;
        assert!((cagr - expected).abs() < 1e-8);
    }

    #[test]
    fn test_omega_all_above_threshold() {
        let r = vec![0.01, 0.02, 0.03];
        let o = PerformanceCalculator::omega_ratio(&r, 0.0);
        assert!(o.is_infinite(), "Omega should be +Inf when no losses");
    }

    #[test]
    fn test_omega_all_below_threshold() {
        let r = vec![-0.01, -0.02, -0.03];
        let o = PerformanceCalculator::omega_ratio(&r, 0.0);
        assert_eq!(o, 0.0);
    }

    #[test]
    fn test_omega_mixed() {
        // returns [0.02, -0.01] threshold 0.0 → gains=0.02, losses=0.01 → omega=2.0
        let r = vec![0.02, -0.01];
        let o = PerformanceCalculator::omega_ratio(&r, 0.0);
        assert!((o - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_information_ratio_length_mismatch() {
        let r = vec![0.01, 0.02];
        let b = vec![0.01];
        assert_eq!(PerformanceCalculator::information_ratio(&r, &b), 0.0);
    }

    #[test]
    fn test_information_ratio_identical() {
        let r = vec![0.01, 0.02, 0.03];
        let b = r.clone();
        // active returns all 0 → IR = 0
        assert_eq!(PerformanceCalculator::information_ratio(&r, &b), 0.0);
    }

    #[test]
    fn test_information_ratio_positive() {
        let r = vec![0.02, 0.03, 0.04];
        let b = vec![0.01, 0.01, 0.01];
        let ir = PerformanceCalculator::information_ratio(&r, &b);
        assert!(ir > 0.0);
    }

    #[test]
    fn test_calmar_empty() {
        assert_eq!(PerformanceCalculator::calmar_ratio(&[]), 0.0);
    }

    #[test]
    fn test_calmar_no_drawdown() {
        // All positive → max_drawdown = 0 → calmar = 0
        let r = vec![0.01; 252];
        assert_eq!(PerformanceCalculator::calmar_ratio(&r), 0.0);
    }

    #[test]
    fn test_compute_all_returns_struct() {
        let r = sample_returns();
        let bench = vec![0.005, 0.005, 0.005, 0.005, 0.005];
        let m = PerformanceCalculator::compute_all(&r, Some(&bench), 0.0001);
        // Just verify the struct is populated (not NaN)
        assert!(!m.sharpe_ratio.is_nan());
        assert!(!m.sortino_ratio.is_nan());
        assert!(!m.calmar_ratio.is_nan());
        assert!(!m.omega_ratio.is_nan());
        assert!(!m.information_ratio.is_nan());
        assert!(!m.max_drawdown.is_nan());
        assert!(!m.cagr.is_nan());
    }

    #[test]
    fn test_compute_all_no_benchmark() {
        let r = sample_returns();
        let m = PerformanceCalculator::compute_all(&r, None, 0.0);
        assert_eq!(m.information_ratio, 0.0);
    }
}
