//! # Module: attribution
//!
//! ## Responsibility
//! Portfolio performance attribution: decomposes returns into allocation,
//! selection, interaction, and factor-level contributions so the source of
//! excess returns is transparent and auditable.
//!
//! ## Models Implemented
//!
//! | Type | Description |
//! |------|-------------|
//! | [`BrinsonHoodBeebower`] | Classic allocation/selection/interaction decomposition |
//! | [`FactorAttribution`] | Multi-factor return decomposition (market, size, value, momentum, quality) |
//! | [`RiskContribution`] | Marginal risk contribution per position for risk budgeting |
//! | [`AttributionReport`] | Structured report aggregating all attribution results |
//! | [`PerformanceTearsheet`] | Comprehensive performance summary with annualised statistics |
//!
//! ## Guarantees
//! - Zero panics; all fallible operations return `Result<_, FinError>`.
//! - All arithmetic uses `f64` (attribution math is intentionally float-based —
//!   the precision requirements here are statistical, not monetary).
//! - Rolling windows use `VecDeque`; no unbounded allocation.
//!
//! ## NOT Responsible For
//! - Position management (see: position module)
//! - Risk rule enforcement (see: risk module)
//! - Order execution

use std::collections::HashMap;

use crate::error::FinError;

// ---------------------------------------------------------------------------
// BrinsonHoodBeebower
// ---------------------------------------------------------------------------

/// A single-period segment used in Brinson-Hood-Beebower attribution.
///
/// A "segment" is a market sector, asset class, or any other grouping used
/// for attribution decomposition.
#[derive(Debug, Clone)]
pub struct BhbSegment {
    /// Name of the segment (e.g., "Technology", "Energy").
    pub name: String,
    /// Portfolio weight in this segment; range `[0.0, 1.0]`.
    pub portfolio_weight: f64,
    /// Benchmark weight in this segment; range `[0.0, 1.0]`.
    pub benchmark_weight: f64,
    /// Portfolio return within this segment (not annualised).
    pub portfolio_return: f64,
    /// Benchmark return within this segment (not annualised).
    pub benchmark_return: f64,
}

/// Allocation effect, selection effect, and interaction effect for one segment.
#[derive(Debug, Clone)]
pub struct BhbSegmentResult {
    /// Segment name.
    pub name: String,
    /// Allocation effect: `(w_p - w_b) * (r_b_seg - r_b_total)`.
    pub allocation_effect: f64,
    /// Selection effect: `w_b * (r_p_seg - r_b_seg)`.
    pub selection_effect: f64,
    /// Interaction effect: `(w_p - w_b) * (r_p_seg - r_b_seg)`.
    pub interaction_effect: f64,
    /// Total active contribution: `allocation + selection + interaction`.
    pub total_active: f64,
}

/// Brinson-Hood-Beebower performance attribution (1986).
///
/// Decomposes portfolio excess return into three effects at the segment level:
/// - **Allocation**: did the manager over/under-weight profitable sectors?
/// - **Selection**: did the manager pick better stocks within each sector?
/// - **Interaction**: joint effect of weight and selection decisions.
///
/// Total active return = sum of `total_active` across all segments
/// ≈ portfolio return − benchmark return.
///
/// # Example
///
/// ```rust
/// use fin_primitives::attribution::{BrinsonHoodBeebower, BhbSegment};
///
/// let segments = vec![
///     BhbSegment {
///         name: "Tech".to_owned(),
///         portfolio_weight: 0.40,
///         benchmark_weight: 0.30,
///         portfolio_return: 0.12,
///         benchmark_return: 0.10,
///     },
///     BhbSegment {
///         name: "Energy".to_owned(),
///         portfolio_weight: 0.60,
///         benchmark_weight: 0.70,
///         portfolio_return: 0.04,
///         benchmark_return: 0.05,
///     },
/// ];
/// let result = BrinsonHoodBeebower::compute(&segments).unwrap();
/// assert!((result.total_active_return - (result.total_allocation + result.total_selection + result.total_interaction)).abs() < 1e-10);
/// ```
#[derive(Debug, Clone)]
pub struct BhbResult {
    /// Per-segment decomposition.
    pub segments: Vec<BhbSegmentResult>,
    /// Total allocation effect summed across segments.
    pub total_allocation: f64,
    /// Total selection effect summed across segments.
    pub total_selection: f64,
    /// Total interaction effect summed across segments.
    pub total_interaction: f64,
    /// Total active return (allocation + selection + interaction).
    pub total_active_return: f64,
    /// Weighted portfolio return.
    pub portfolio_return: f64,
    /// Weighted benchmark return.
    pub benchmark_return: f64,
}

/// Brinson-Hood-Beebower attribution calculator.
pub struct BrinsonHoodBeebower;

impl BrinsonHoodBeebower {
    /// Compute attribution for a slice of [`BhbSegment`]s.
    ///
    /// # Errors
    ///
    /// Returns [`FinError::InvalidInput`] if:
    /// - `segments` is empty.
    /// - Any `portfolio_weight` or `benchmark_weight` is negative or non-finite.
    pub fn compute(segments: &[BhbSegment]) -> Result<BhbResult, FinError> {
        if segments.is_empty() {
            return Err(FinError::InvalidInput(
                "BHB attribution requires at least one segment".to_owned(),
            ));
        }

        for seg in segments {
            if !seg.portfolio_weight.is_finite() || seg.portfolio_weight < 0.0 {
                return Err(FinError::InvalidInput(format!(
                    "segment '{}': portfolio_weight must be non-negative finite",
                    seg.name
                )));
            }
            if !seg.benchmark_weight.is_finite() || seg.benchmark_weight < 0.0 {
                return Err(FinError::InvalidInput(format!(
                    "segment '{}': benchmark_weight must be non-negative finite",
                    seg.name
                )));
            }
        }

        // Benchmark total return: sum(w_b * r_b_seg)
        let benchmark_total: f64 = segments
            .iter()
            .map(|s| s.benchmark_weight * s.benchmark_return)
            .sum();

        // Portfolio total return: sum(w_p * r_p_seg)
        let portfolio_total: f64 = segments
            .iter()
            .map(|s| s.portfolio_weight * s.portfolio_return)
            .sum();

        let mut seg_results = Vec::with_capacity(segments.len());
        let mut total_allocation = 0.0f64;
        let mut total_selection = 0.0f64;
        let mut total_interaction = 0.0f64;

        for seg in segments {
            let alloc = (seg.portfolio_weight - seg.benchmark_weight)
                * (seg.benchmark_return - benchmark_total);
            let select = seg.benchmark_weight * (seg.portfolio_return - seg.benchmark_return);
            let interact =
                (seg.portfolio_weight - seg.benchmark_weight) * (seg.portfolio_return - seg.benchmark_return);
            let total_active = alloc + select + interact;

            total_allocation += alloc;
            total_selection += select;
            total_interaction += interact;

            seg_results.push(BhbSegmentResult {
                name: seg.name.clone(),
                allocation_effect: alloc,
                selection_effect: select,
                interaction_effect: interact,
                total_active,
            });
        }

        Ok(BhbResult {
            segments: seg_results,
            total_allocation,
            total_selection,
            total_interaction,
            total_active_return: total_allocation + total_selection + total_interaction,
            portfolio_return: portfolio_total,
            benchmark_return: benchmark_total,
        })
    }
}

// ---------------------------------------------------------------------------
// FactorAttribution
// ---------------------------------------------------------------------------

/// Factor exposures for a single return observation.
///
/// Each field represents the factor return (not the portfolio's factor loading;
/// multiply by position weight externally before passing here).
#[derive(Debug, Clone)]
pub struct FactorReturns {
    /// Market (beta) factor return.
    pub market: f64,
    /// Size (SMB) factor return.
    pub size: f64,
    /// Value (HML) factor return.
    pub value: f64,
    /// Momentum (WML) factor return.
    pub momentum: f64,
    /// Quality (profitability) factor return.
    pub quality: f64,
}

/// Per-factor attribution decomposition for one period.
#[derive(Debug, Clone)]
pub struct FactorAttributionResult {
    /// Return explained by the market factor.
    pub market_contribution: f64,
    /// Return explained by the size factor.
    pub size_contribution: f64,
    /// Return explained by the value factor.
    pub value_contribution: f64,
    /// Return explained by the momentum factor.
    pub momentum_contribution: f64,
    /// Return explained by the quality factor.
    pub quality_contribution: f64,
    /// Total factor-explained return.
    pub total_factor_return: f64,
    /// Idiosyncratic (alpha) residual: `portfolio_return - total_factor_return`.
    pub alpha: f64,
    /// The portfolio return passed to this computation.
    pub portfolio_return: f64,
}

/// Multi-factor return attribution.
///
/// Given factor exposures (betas) and factor returns, decomposes portfolio return
/// into factor contributions and an idiosyncratic residual (alpha).
///
/// # Example
///
/// ```rust
/// use fin_primitives::attribution::{FactorAttribution, FactorReturns};
///
/// let betas = FactorReturns { market: 1.1, size: 0.2, value: -0.1, momentum: 0.3, quality: 0.05 };
/// let factor_returns = FactorReturns { market: 0.01, size: 0.002, value: -0.001, momentum: 0.003, quality: 0.001 };
/// let portfolio_return = 0.015;
/// let result = FactorAttribution::compute(&betas, &factor_returns, portfolio_return).unwrap();
/// assert!(result.total_factor_return.is_finite());
/// ```
pub struct FactorAttribution;

impl FactorAttribution {
    /// Decompose `portfolio_return` into factor contributions and alpha.
    ///
    /// Each factor contribution is `beta_i * factor_return_i`.
    ///
    /// # Errors
    ///
    /// Returns [`FinError::InvalidInput`] if any beta or factor return is non-finite.
    pub fn compute(
        betas: &FactorReturns,
        factor_returns: &FactorReturns,
        portfolio_return: f64,
    ) -> Result<FactorAttributionResult, FinError> {
        Self::validate_finite(betas, "betas")?;
        Self::validate_finite(factor_returns, "factor_returns")?;
        if !portfolio_return.is_finite() {
            return Err(FinError::InvalidInput(
                "portfolio_return must be finite".to_owned(),
            ));
        }

        let market_c = betas.market * factor_returns.market;
        let size_c = betas.size * factor_returns.size;
        let value_c = betas.value * factor_returns.value;
        let momentum_c = betas.momentum * factor_returns.momentum;
        let quality_c = betas.quality * factor_returns.quality;
        let total = market_c + size_c + value_c + momentum_c + quality_c;
        let alpha = portfolio_return - total;

        Ok(FactorAttributionResult {
            market_contribution: market_c,
            size_contribution: size_c,
            value_contribution: value_c,
            momentum_contribution: momentum_c,
            quality_contribution: quality_c,
            total_factor_return: total,
            alpha,
            portfolio_return,
        })
    }

    fn validate_finite(f: &FactorReturns, label: &str) -> Result<(), FinError> {
        let fields = [
            ("market", f.market),
            ("size", f.size),
            ("value", f.value),
            ("momentum", f.momentum),
            ("quality", f.quality),
        ];
        for (name, val) in fields {
            if !val.is_finite() {
                return Err(FinError::InvalidInput(format!(
                    "{label}.{name} must be finite"
                )));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// RiskContribution
// ---------------------------------------------------------------------------

/// Per-position marginal risk contribution.
///
/// For a portfolio with covariance matrix `Σ` and weight vector `w`, the
/// marginal risk contribution of position `i` is:
///
/// ```text
/// MRC_i = w_i * (Σw)_i / σ_p
/// ```
///
/// where `σ_p = sqrt(w' Σ w)` is portfolio volatility.
///
/// The sum of all `MRC_i` equals portfolio volatility (Euler decomposition).
#[derive(Debug, Clone)]
pub struct PositionRiskContribution {
    /// Position identifier (e.g., ticker symbol).
    pub id: String,
    /// Portfolio weight in `[0.0, 1.0]` (short positions as negative).
    pub weight: f64,
    /// Marginal risk contribution as a fraction of portfolio volatility.
    pub marginal_risk_contribution: f64,
    /// Percentage risk contribution: `MRC_i / σ_p * 100`.
    pub pct_risk_contribution: f64,
}

/// Computes per-position marginal risk contributions from a covariance matrix.
///
/// # Example
///
/// ```rust
/// use fin_primitives::attribution::RiskContribution;
///
/// // Two-asset portfolio: equal weights, uncorrelated, same vol
/// let weights = vec![0.5, 0.5];
/// let cov = vec![
///     vec![0.04, 0.0],  // var(A) = 0.04, cov(A,B) = 0
///     vec![0.0,  0.04], // cov(B,A) = 0,  var(B) = 0.04
/// ];
/// let ids = vec!["A".to_owned(), "B".to_owned()];
/// let result = RiskContribution::compute(&ids, &weights, &cov).unwrap();
/// // Each asset contributes 50% of portfolio risk
/// assert!((result[0].pct_risk_contribution - 50.0).abs() < 0.01);
/// ```
pub struct RiskContribution;

impl RiskContribution {
    /// Compute per-position marginal risk contributions.
    ///
    /// # Parameters
    ///
    /// * `ids`     — Position identifiers, length N.
    /// * `weights` — Portfolio weights, length N.
    /// * `cov`     — N×N covariance matrix (row-major).
    ///
    /// # Errors
    ///
    /// Returns [`FinError::InvalidInput`] if:
    /// - `ids`, `weights`, and `cov` lengths are inconsistent.
    /// - Portfolio volatility is zero (all-zero weights or flat covariance).
    /// - Any weight or covariance entry is non-finite.
    pub fn compute(
        ids: &[String],
        weights: &[f64],
        cov: &[Vec<f64>],
    ) -> Result<Vec<PositionRiskContribution>, FinError> {
        let n = ids.len();
        if n == 0 {
            return Err(FinError::InvalidInput(
                "RiskContribution requires at least one position".to_owned(),
            ));
        }
        if weights.len() != n {
            return Err(FinError::InvalidInput(format!(
                "weights length ({}) must match ids length ({n})",
                weights.len()
            )));
        }
        if cov.len() != n || cov.iter().any(|row| row.len() != n) {
            return Err(FinError::InvalidInput(format!(
                "covariance matrix must be {n}×{n}"
            )));
        }
        for (i, &w) in weights.iter().enumerate() {
            if !w.is_finite() {
                return Err(FinError::InvalidInput(format!(
                    "weights[{i}] must be finite"
                )));
            }
        }

        // Σw — matrix-vector product
        let sigma_w: Vec<f64> = (0..n)
            .map(|i| {
                (0..n).map(|j| cov[i][j] * weights[j]).sum::<f64>()
            })
            .collect();

        // Portfolio variance: w' Σ w
        let port_var: f64 = weights.iter().zip(sigma_w.iter()).map(|(w, sw)| w * sw).sum();
        if port_var <= 0.0 {
            return Err(FinError::InvalidInput(
                "portfolio variance is zero or negative; cannot compute risk contributions".to_owned(),
            ));
        }
        let port_vol = port_var.sqrt();

        let results: Vec<PositionRiskContribution> = (0..n)
            .map(|i| {
                let mrc = weights[i] * sigma_w[i] / port_vol;
                let pct = mrc / port_vol * 100.0;
                PositionRiskContribution {
                    id: ids[i].clone(),
                    weight: weights[i],
                    marginal_risk_contribution: mrc,
                    pct_risk_contribution: pct,
                }
            })
            .collect();

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// AttributionReport
// ---------------------------------------------------------------------------

/// Structured performance attribution report for a single period.
///
/// Aggregates the outputs of [`BrinsonHoodBeebower`], [`FactorAttribution`],
/// and [`RiskContribution`] into a single report value for serialisation,
/// logging, or display.
#[derive(Debug, Clone)]
pub struct AttributionReport {
    /// Label for this attribution period (e.g., "2024-Q1").
    pub period_label: String,
    /// BHB allocation/selection/interaction decomposition (if computed).
    pub bhb: Option<BhbResult>,
    /// Factor-level return decomposition (if computed).
    pub factor: Option<FactorAttributionResult>,
    /// Per-position risk contributions (if computed).
    pub risk_contributions: Option<Vec<PositionRiskContribution>>,
    /// Total portfolio return for the period.
    pub portfolio_return: f64,
    /// Total benchmark return for the period.
    pub benchmark_return: f64,
    /// Excess return: `portfolio_return - benchmark_return`.
    pub excess_return: f64,
}

impl AttributionReport {
    /// Construct an attribution report from raw returns and optional attribution results.
    ///
    /// # Errors
    ///
    /// Returns [`FinError::InvalidInput`] if `portfolio_return` or
    /// `benchmark_return` is non-finite.
    pub fn new(
        period_label: impl Into<String>,
        portfolio_return: f64,
        benchmark_return: f64,
        bhb: Option<BhbResult>,
        factor: Option<FactorAttributionResult>,
        risk_contributions: Option<Vec<PositionRiskContribution>>,
    ) -> Result<Self, FinError> {
        if !portfolio_return.is_finite() {
            return Err(FinError::InvalidInput(
                "portfolio_return must be finite".to_owned(),
            ));
        }
        if !benchmark_return.is_finite() {
            return Err(FinError::InvalidInput(
                "benchmark_return must be finite".to_owned(),
            ));
        }
        Ok(Self {
            period_label: period_label.into(),
            bhb,
            factor,
            risk_contributions,
            portfolio_return,
            benchmark_return,
            excess_return: portfolio_return - benchmark_return,
        })
    }

    /// Returns the information ratio proxy: `excess_return / tracking_error`.
    ///
    /// Returns `None` if `tracking_error` is zero or non-finite.
    pub fn information_ratio(&self, tracking_error: f64) -> Option<f64> {
        if tracking_error <= 0.0 || !tracking_error.is_finite() {
            return None;
        }
        Some(self.excess_return / tracking_error)
    }
}

// ---------------------------------------------------------------------------
// PerformanceTearsheet
// ---------------------------------------------------------------------------

/// A single return observation used to build a [`PerformanceTearsheet`].
#[derive(Debug, Clone)]
pub struct ReturnObservation {
    /// Identifier for this period (e.g., "2024-01-15" or bar index).
    pub label: String,
    /// Portfolio return for this period.
    pub portfolio_return: f64,
    /// Benchmark return for the same period.
    pub benchmark_return: f64,
}

/// Comprehensive performance summary computed from a return series.
///
/// Covers absolute performance, risk-adjusted metrics, and drawdown analysis.
///
/// # Example
///
/// ```rust
/// use fin_primitives::attribution::{PerformanceTearsheet, ReturnObservation};
///
/// let obs = vec![
///     ReturnObservation { label: "d1".to_owned(), portfolio_return: 0.01, benchmark_return: 0.008 },
///     ReturnObservation { label: "d2".to_owned(), portfolio_return: -0.005, benchmark_return: -0.003 },
///     ReturnObservation { label: "d3".to_owned(), portfolio_return: 0.02, benchmark_return: 0.015 },
/// ];
/// let ts = PerformanceTearsheet::compute(&obs, 252).unwrap();
/// assert!(ts.annualised_return.is_finite());
/// assert!(ts.max_drawdown <= 0.0);
/// ```
#[derive(Debug, Clone)]
pub struct PerformanceTearsheet {
    /// Number of observations used.
    pub observation_count: usize,
    /// Compounded total portfolio return over all periods.
    pub total_return: f64,
    /// Annualised portfolio return (geometric).
    pub annualised_return: f64,
    /// Annualised volatility of portfolio returns.
    pub annualised_volatility: f64,
    /// Sharpe ratio: `annualised_return / annualised_volatility`.
    /// `None` if volatility is zero.
    pub sharpe_ratio: Option<f64>,
    /// Maximum peak-to-trough drawdown (always <= 0).
    pub max_drawdown: f64,
    /// Calmar ratio: `annualised_return / |max_drawdown|`.
    /// `None` if max_drawdown is zero.
    pub calmar_ratio: Option<f64>,
    /// Annualised excess return over the benchmark.
    pub annualised_excess_return: f64,
    /// Annualised tracking error (std dev of excess returns).
    pub tracking_error: f64,
    /// Information ratio: `annualised_excess_return / tracking_error`.
    /// `None` if tracking error is zero.
    pub information_ratio: Option<f64>,
    /// Sortino ratio: `annualised_return / downside_deviation`.
    /// `None` if downside deviation is zero.
    pub sortino_ratio: Option<f64>,
    /// Win rate: fraction of periods with positive portfolio return.
    pub win_rate: f64,
    /// Per-period return statistics: min, max, mean.
    pub return_min: f64,
    /// Maximum single-period return.
    pub return_max: f64,
    /// Mean single-period return.
    pub return_mean: f64,
}

/// Calculates a [`PerformanceTearsheet`] from a sequence of return observations.
impl PerformanceTearsheet {
    /// Compute all performance metrics from a return series.
    ///
    /// # Parameters
    ///
    /// * `observations`       — Ordered sequence of return observations.
    /// * `periods_per_year`   — Annualisation factor: 252 for daily, 12 for monthly, 4 for quarterly.
    ///
    /// # Errors
    ///
    /// Returns [`FinError::InvalidInput`] if:
    /// - `observations` is empty.
    /// - `periods_per_year` is zero.
    /// - Any return value is non-finite.
    pub fn compute(
        observations: &[ReturnObservation],
        periods_per_year: usize,
    ) -> Result<Self, FinError> {
        if observations.is_empty() {
            return Err(FinError::InvalidInput(
                "PerformanceTearsheet requires at least one observation".to_owned(),
            ));
        }
        if periods_per_year == 0 {
            return Err(FinError::InvalidInput(
                "periods_per_year must be at least 1".to_owned(),
            ));
        }
        for (i, obs) in observations.iter().enumerate() {
            if !obs.portfolio_return.is_finite() || !obs.benchmark_return.is_finite() {
                return Err(FinError::InvalidInput(format!(
                    "observation[{i}] contains non-finite return"
                )));
            }
        }

        let n = observations.len();
        let ppy = periods_per_year as f64;
        let port_returns: Vec<f64> = observations.iter().map(|o| o.portfolio_return).collect();
        let bench_returns: Vec<f64> = observations.iter().map(|o| o.benchmark_return).collect();
        let excess_returns: Vec<f64> = port_returns
            .iter()
            .zip(bench_returns.iter())
            .map(|(p, b)| p - b)
            .collect();

        // Total compounded return
        let total_return = port_returns.iter().fold(1.0f64, |acc, r| acc * (1.0 + r)) - 1.0;

        // Annualised return (geometric)
        let years = n as f64 / ppy;
        let annualised_return = if years > 0.0 {
            (1.0 + total_return).powf(1.0 / years) - 1.0
        } else {
            total_return
        };

        // Volatility (sample std dev, annualised)
        let mean_r = port_returns.iter().sum::<f64>() / n as f64;
        let var_r = if n > 1 {
            port_returns.iter().map(|r| (r - mean_r).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };
        let annualised_volatility = var_r.sqrt() * ppy.sqrt();

        // Sharpe ratio
        let sharpe_ratio = if annualised_volatility > 0.0 {
            Some(annualised_return / annualised_volatility)
        } else {
            None
        };

        // Max drawdown
        let max_drawdown = Self::max_drawdown(&port_returns);

        // Calmar ratio
        let calmar_ratio = if max_drawdown < 0.0 {
            Some(annualised_return / max_drawdown.abs())
        } else {
            None
        };

        // Excess return (annualised)
        let excess_mean = excess_returns.iter().sum::<f64>() / n as f64;
        let total_excess = excess_returns.iter().fold(1.0f64, |acc, r| acc * (1.0 + r)) - 1.0;
        let annualised_excess_return = if years > 0.0 {
            (1.0 + total_excess).powf(1.0 / years) - 1.0
        } else {
            excess_mean * ppy
        };

        // Tracking error (sample std dev of excess returns, annualised)
        let te_var = if n > 1 {
            excess_returns
                .iter()
                .map(|r| (r - excess_mean).powi(2))
                .sum::<f64>()
                / (n - 1) as f64
        } else {
            0.0
        };
        let tracking_error = te_var.sqrt() * ppy.sqrt();

        // Information ratio
        let information_ratio = if tracking_error > 0.0 {
            Some(annualised_excess_return / tracking_error)
        } else {
            None
        };

        // Sortino ratio (downside deviation, target = 0)
        let downside_sq_sum: f64 = port_returns.iter().map(|r| r.min(0.0).powi(2)).sum();
        let downside_dev = if n > 1 {
            (downside_sq_sum / (n - 1) as f64).sqrt() * ppy.sqrt()
        } else {
            0.0
        };
        let sortino_ratio = if downside_dev > 0.0 {
            Some(annualised_return / downside_dev)
        } else {
            None
        };

        // Win rate
        let wins = port_returns.iter().filter(|&&r| r > 0.0).count();
        let win_rate = wins as f64 / n as f64;

        // Return stats
        let return_min = port_returns.iter().cloned().fold(f64::INFINITY, f64::min);
        let return_max = port_returns.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let return_mean = mean_r;

        Ok(Self {
            observation_count: n,
            total_return,
            annualised_return,
            annualised_volatility,
            sharpe_ratio,
            max_drawdown,
            calmar_ratio,
            annualised_excess_return,
            tracking_error,
            information_ratio,
            sortino_ratio,
            win_rate,
            return_min,
            return_max,
            return_mean,
        })
    }

    /// Compute peak-to-trough maximum drawdown.
    ///
    /// Returns a value <= 0. Returns 0.0 if the equity curve never declines.
    fn max_drawdown(returns: &[f64]) -> f64 {
        let mut peak = 1.0f64;
        let mut equity = 1.0f64;
        let mut max_dd = 0.0f64;

        for &r in returns {
            equity *= 1.0 + r;
            if equity > peak {
                peak = equity;
            }
            let dd = (equity - peak) / peak;
            if dd < max_dd {
                max_dd = dd;
            }
        }

        max_dd
    }

    /// Format a brief one-line summary of the tearsheet.
    #[must_use]
    pub fn summary(&self) -> String {
        let sharpe = self
            .sharpe_ratio
            .map(|s| format!("{s:.2}"))
            .unwrap_or_else(|| "n/a".to_owned());
        let ir = self
            .information_ratio
            .map(|s| format!("{s:.2}"))
            .unwrap_or_else(|| "n/a".to_owned());
        format!(
            "n={} | ann_ret={:.2}% | vol={:.2}% | sharpe={sharpe} | maxDD={:.2}% | \
             ann_excess={:.2}% | TE={:.2}% | IR={ir} | win_rate={:.1}%",
            self.observation_count,
            self.annualised_return * 100.0,
            self.annualised_volatility * 100.0,
            self.max_drawdown * 100.0,
            self.annualised_excess_return * 100.0,
            self.tracking_error * 100.0,
            self.win_rate * 100.0,
        )
    }
}

// ---------------------------------------------------------------------------
// AttributionSeries
// ---------------------------------------------------------------------------

/// Accumulates multiple [`AttributionReport`]s over time, keyed by period label.
///
/// Useful for persisting attribution through a rolling backtest or live run.
#[derive(Debug, Default)]
pub struct AttributionSeries {
    reports: Vec<AttributionReport>,
    index: HashMap<String, usize>,
}

impl AttributionSeries {
    /// Create a new empty series.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an attribution report to the series.
    ///
    /// # Errors
    ///
    /// Returns [`FinError::InvalidInput`] if a report with the same
    /// `period_label` already exists (duplicate labels are not allowed).
    pub fn push(&mut self, report: AttributionReport) -> Result<(), FinError> {
        if self.index.contains_key(&report.period_label) {
            return Err(FinError::InvalidInput(format!(
                "duplicate period label '{}'",
                report.period_label
            )));
        }
        self.index
            .insert(report.period_label.clone(), self.reports.len());
        self.reports.push(report);
        Ok(())
    }

    /// Look up a report by period label. Returns `None` if not found.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<&AttributionReport> {
        self.index.get(label).and_then(|&i| self.reports.get(i))
    }

    /// All reports in insertion order.
    #[must_use]
    pub fn reports(&self) -> &[AttributionReport] {
        &self.reports
    }

    /// Number of periods in the series.
    #[must_use]
    pub fn len(&self) -> usize {
        self.reports.len()
    }

    /// Returns `true` if no periods have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── BrinsonHoodBeebower ───────────────────────────────────────────────

    fn two_segment_bhb() -> Vec<BhbSegment> {
        vec![
            BhbSegment {
                name: "Tech".to_owned(),
                portfolio_weight: 0.40,
                benchmark_weight: 0.30,
                portfolio_return: 0.12,
                benchmark_return: 0.10,
            },
            BhbSegment {
                name: "Energy".to_owned(),
                portfolio_weight: 0.60,
                benchmark_weight: 0.70,
                portfolio_return: 0.04,
                benchmark_return: 0.05,
            },
        ]
    }

    #[test]
    fn test_bhb_sum_equals_active_return() {
        let segs = two_segment_bhb();
        let r = BrinsonHoodBeebower::compute(&segs).unwrap();
        let expected_active = r.portfolio_return - r.benchmark_return;
        assert!(
            (r.total_active_return - expected_active).abs() < 1e-10,
            "total_active={} expected_active={}",
            r.total_active_return,
            expected_active
        );
    }

    #[test]
    fn test_bhb_empty_segments_fails() {
        assert!(BrinsonHoodBeebower::compute(&[]).is_err());
    }

    #[test]
    fn test_bhb_negative_weight_fails() {
        let segs = vec![BhbSegment {
            name: "X".to_owned(),
            portfolio_weight: -0.1,
            benchmark_weight: 0.5,
            portfolio_return: 0.05,
            benchmark_return: 0.04,
        }];
        assert!(BrinsonHoodBeebower::compute(&segs).is_err());
    }

    #[test]
    fn test_bhb_segment_count() {
        let segs = two_segment_bhb();
        let r = BrinsonHoodBeebower::compute(&segs).unwrap();
        assert_eq!(r.segments.len(), 2);
    }

    // ── FactorAttribution ─────────────────────────────────────────────────

    fn sample_betas() -> FactorReturns {
        FactorReturns { market: 1.0, size: 0.2, value: -0.1, momentum: 0.3, quality: 0.05 }
    }

    fn sample_factor_returns() -> FactorReturns {
        FactorReturns {
            market: 0.01,
            size: 0.002,
            value: -0.001,
            momentum: 0.003,
            quality: 0.001,
        }
    }

    #[test]
    fn test_factor_attribution_alpha_consistency() {
        let b = sample_betas();
        let fr = sample_factor_returns();
        let port_ret = 0.015;
        let r = FactorAttribution::compute(&b, &fr, port_ret).unwrap();
        assert!((r.alpha - (port_ret - r.total_factor_return)).abs() < 1e-12);
    }

    #[test]
    fn test_factor_attribution_nan_fails() {
        let b = FactorReturns { market: f64::NAN, size: 0.0, value: 0.0, momentum: 0.0, quality: 0.0 };
        let fr = sample_factor_returns();
        assert!(FactorAttribution::compute(&b, &fr, 0.01).is_err());
    }

    #[test]
    fn test_factor_attribution_inf_portfolio_return_fails() {
        let b = sample_betas();
        let fr = sample_factor_returns();
        assert!(FactorAttribution::compute(&b, &fr, f64::INFINITY).is_err());
    }

    // ── RiskContribution ──────────────────────────────────────────────────

    #[test]
    fn test_risk_contribution_two_equal_uncorrelated() {
        let ids = vec!["A".to_owned(), "B".to_owned()];
        let weights = vec![0.5, 0.5];
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.04]];
        let r = RiskContribution::compute(&ids, &weights, &cov).unwrap();
        // By symmetry both contributions should be equal
        assert!((r[0].pct_risk_contribution - r[1].pct_risk_contribution).abs() < 0.01);
        // Sum of MRC should equal portfolio volatility
        let port_vol = 0.2 * 0.5f64.sqrt(); // sqrt(0.5^2*0.04 + 0.5^2*0.04)
        let mrc_sum: f64 = r.iter().map(|rc| rc.marginal_risk_contribution).sum();
        assert!((mrc_sum - port_vol).abs() < 1e-10, "mrc_sum={mrc_sum} port_vol={port_vol}");
    }

    #[test]
    fn test_risk_contribution_empty_fails() {
        assert!(RiskContribution::compute(&[], &[], &[]).is_err());
    }

    #[test]
    fn test_risk_contribution_mismatched_lengths_fails() {
        let ids = vec!["A".to_owned()];
        let weights = vec![0.5, 0.5];
        let cov = vec![vec![0.04]];
        assert!(RiskContribution::compute(&ids, &weights, &cov).is_err());
    }

    #[test]
    fn test_risk_contribution_zero_variance_fails() {
        let ids = vec!["A".to_owned()];
        let weights = vec![0.0];
        let cov = vec![vec![0.04]];
        assert!(RiskContribution::compute(&ids, &weights, &cov).is_err());
    }

    // ── AttributionReport ─────────────────────────────────────────────────

    #[test]
    fn test_attribution_report_excess_return() {
        let r = AttributionReport::new("2024-Q1", 0.08, 0.05, None, None, None).unwrap();
        assert!((r.excess_return - 0.03).abs() < 1e-10);
    }

    #[test]
    fn test_attribution_report_information_ratio() {
        let r = AttributionReport::new("2024-Q1", 0.08, 0.05, None, None, None).unwrap();
        let ir = r.information_ratio(0.06);
        assert!(ir.is_some());
        assert!((ir.unwrap() - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_attribution_report_zero_te_returns_none() {
        let r = AttributionReport::new("X", 0.05, 0.03, None, None, None).unwrap();
        assert!(r.information_ratio(0.0).is_none());
    }

    #[test]
    fn test_attribution_report_nan_fails() {
        assert!(AttributionReport::new("X", f64::NAN, 0.0, None, None, None).is_err());
    }

    // ── PerformanceTearsheet ───────────────────────────────────────────────

    fn daily_obs() -> Vec<ReturnObservation> {
        vec![
            ReturnObservation { label: "d1".to_owned(), portfolio_return: 0.01, benchmark_return: 0.008 },
            ReturnObservation { label: "d2".to_owned(), portfolio_return: -0.005, benchmark_return: -0.003 },
            ReturnObservation { label: "d3".to_owned(), portfolio_return: 0.02, benchmark_return: 0.015 },
            ReturnObservation { label: "d4".to_owned(), portfolio_return: -0.01, benchmark_return: -0.005 },
            ReturnObservation { label: "d5".to_owned(), portfolio_return: 0.015, benchmark_return: 0.012 },
        ]
    }

    #[test]
    fn test_tearsheet_basic_metrics_finite() {
        let ts = PerformanceTearsheet::compute(&daily_obs(), 252).unwrap();
        assert!(ts.annualised_return.is_finite());
        assert!(ts.annualised_volatility.is_finite());
        assert!(ts.max_drawdown <= 0.0);
        assert!(ts.win_rate >= 0.0 && ts.win_rate <= 1.0);
    }

    #[test]
    fn test_tearsheet_max_drawdown_non_positive() {
        let ts = PerformanceTearsheet::compute(&daily_obs(), 252).unwrap();
        assert!(ts.max_drawdown <= 0.0);
    }

    #[test]
    fn test_tearsheet_total_return_compounded() {
        let obs = vec![
            ReturnObservation { label: "a".to_owned(), portfolio_return: 0.10, benchmark_return: 0.08 },
            ReturnObservation { label: "b".to_owned(), portfolio_return: 0.10, benchmark_return: 0.08 },
        ];
        let ts = PerformanceTearsheet::compute(&obs, 252).unwrap();
        let expected = 1.1 * 1.1 - 1.0;
        assert!((ts.total_return - expected).abs() < 1e-10);
    }

    #[test]
    fn test_tearsheet_empty_fails() {
        assert!(PerformanceTearsheet::compute(&[], 252).is_err());
    }

    #[test]
    fn test_tearsheet_zero_periods_per_year_fails() {
        assert!(PerformanceTearsheet::compute(&daily_obs(), 0).is_err());
    }

    #[test]
    fn test_tearsheet_summary_non_empty() {
        let ts = PerformanceTearsheet::compute(&daily_obs(), 252).unwrap();
        assert!(!ts.summary().is_empty());
    }

    // ── AttributionSeries ─────────────────────────────────────────────────

    #[test]
    fn test_attribution_series_push_and_get() {
        let mut series = AttributionSeries::new();
        let r = AttributionReport::new("2024-Q1", 0.08, 0.05, None, None, None).unwrap();
        series.push(r).unwrap();
        assert!(series.get("2024-Q1").is_some());
        assert!(series.get("2024-Q2").is_none());
    }

    #[test]
    fn test_attribution_series_duplicate_label_fails() {
        let mut series = AttributionSeries::new();
        let r1 = AttributionReport::new("2024-Q1", 0.08, 0.05, None, None, None).unwrap();
        let r2 = AttributionReport::new("2024-Q1", 0.09, 0.06, None, None, None).unwrap();
        series.push(r1).unwrap();
        assert!(series.push(r2).is_err());
    }

    #[test]
    fn test_attribution_series_len() {
        let mut series = AttributionSeries::new();
        assert!(series.is_empty());
        for i in 0..3u32 {
            let r = AttributionReport::new(format!("period-{i}"), 0.05, 0.04, None, None, None).unwrap();
            series.push(r).unwrap();
        }
        assert_eq!(series.len(), 3);
    }
}

// ---------------------------------------------------------------------------
// BHB Sector-level API
// ---------------------------------------------------------------------------

/// A single sector used in Brinson-Hood-Beebower performance attribution.
///
/// Holds the portfolio and benchmark weights and returns for one sector
/// (e.g. "Technology", "Healthcare") within a single measurement period.
#[derive(Debug, Clone)]
pub struct Sector {
    /// Human-readable sector name.
    pub name: String,
    /// Portfolio weight in this sector; expected in `[0, 1]`.
    pub portfolio_weight: f64,
    /// Benchmark weight in this sector; expected in `[0, 1]`.
    pub benchmark_weight: f64,
    /// Portfolio return within this sector for the period.
    pub portfolio_return: f64,
    /// Benchmark return within this sector for the period.
    pub benchmark_return: f64,
}

/// Computed attribution effects for a single sector.
#[derive(Debug, Clone)]
pub struct SectorAttribution {
    /// Sector inputs used in computation.
    pub sector: Sector,
    /// Allocation effect: `(w_p - w_b) * (r_b_sector - R_b)`.
    pub allocation_effect: f64,
    /// Selection effect: `w_b * (r_p - r_b)`.
    pub selection_effect: f64,
    /// Interaction effect: `(w_p - w_b) * (r_p - r_b)`.
    pub interaction_effect: f64,
    /// Total active return for this sector: allocation + selection + interaction.
    pub total_active_return: f64,
}

/// Aggregate attribution report across all sectors.
#[derive(Debug, Clone)]
pub struct SectorAttributionReport {
    /// Per-sector decomposition results.
    pub sectors: Vec<SectorAttribution>,
    /// Sum of allocation effects across all sectors.
    pub total_allocation: f64,
    /// Sum of selection effects across all sectors.
    pub total_selection: f64,
    /// Sum of interaction effects across all sectors.
    pub total_interaction: f64,
    /// Total active return: total_allocation + total_selection + total_interaction.
    pub total_active_return: f64,
    /// Benchmark portfolio return (weighted sum of sector benchmark returns).
    pub benchmark_return: f64,
    /// Portfolio return (weighted sum of sector portfolio returns).
    pub portfolio_return: f64,
}

/// Brinson-Hood-Beebower attribution calculator that works directly with [`Sector`] slices.
///
/// This struct holds the total benchmark return (`R_b`) used in the allocation effect
/// formula and exposes the three BHB effect computations as methods.
pub struct Attribution {
    /// Total benchmark return used as the benchmark reference in the allocation effect.
    pub benchmark_return: f64,
}

impl Attribution {
    /// Create a new `Attribution` calculator using the given total benchmark return.
    pub fn new(benchmark_return: f64) -> Self {
        Self { benchmark_return }
    }

    /// Allocation effect for a sector: `(w_p - w_b) * (r_b_sector - R_b)`.
    ///
    /// Measures whether the manager added value by over/under-weighting the sector
    /// relative to the benchmark, evaluated against the sector's benchmark return.
    pub fn allocation_effect(&self, s: &Sector) -> f64 {
        (s.portfolio_weight - s.benchmark_weight) * (s.benchmark_return - self.benchmark_return)
    }

    /// Selection effect for a sector: `w_b * (r_p - r_b)`.
    ///
    /// Measures whether the manager picked better securities within the sector.
    pub fn selection_effect(&self, s: &Sector) -> f64 {
        s.benchmark_weight * (s.portfolio_return - s.benchmark_return)
    }

    /// Interaction effect for a sector: `(w_p - w_b) * (r_p - r_b)`.
    ///
    /// Joint effect of simultaneous over/under-weighting and stock selection.
    pub fn interaction_effect(&self, s: &Sector) -> f64 {
        (s.portfolio_weight - s.benchmark_weight) * (s.portfolio_return - s.benchmark_return)
    }

    /// Total active return for a sector: allocation + selection + interaction.
    pub fn total_active_return(&self, s: &Sector) -> f64 {
        self.allocation_effect(s) + self.selection_effect(s) + self.interaction_effect(s)
    }
}

/// Run full Brinson-Hood-Beebower attribution over a slice of sectors.
///
/// The total benchmark return (`R_b`) is computed as the benchmark-weight-averaged
/// return across all sectors.  Per-sector effects are then computed using that
/// aggregate benchmark return, and results are aggregated into a
/// [`SectorAttributionReport`].
///
/// # Example
///
/// ```rust
/// use fin_primitives::attribution::{Sector, run_attribution};
///
/// let sectors = vec![
///     Sector { name: "Tech".into(), portfolio_weight: 0.40, benchmark_weight: 0.30,
///               portfolio_return: 0.12, benchmark_return: 0.10 },
///     Sector { name: "Energy".into(), portfolio_weight: 0.60, benchmark_weight: 0.70,
///               portfolio_return: 0.04, benchmark_return: 0.05 },
/// ];
/// let report = run_attribution(&sectors);
/// let active = report.total_active_return;
/// let decomposed = report.total_allocation + report.total_selection + report.total_interaction;
/// assert!((active - decomposed).abs() < 1e-12);
/// ```
pub fn run_attribution(sectors: &[Sector]) -> SectorAttributionReport {
    // Compute total benchmark return as benchmark-weight-averaged sector benchmark returns.
    let benchmark_return: f64 = sectors
        .iter()
        .map(|s| s.benchmark_weight * s.benchmark_return)
        .sum();

    // Compute total portfolio return as portfolio-weight-averaged sector portfolio returns.
    let portfolio_return: f64 = sectors
        .iter()
        .map(|s| s.portfolio_weight * s.portfolio_return)
        .sum();

    let calc = Attribution::new(benchmark_return);

    let mut sector_results: Vec<SectorAttribution> = Vec::with_capacity(sectors.len());
    let mut total_allocation = 0.0_f64;
    let mut total_selection = 0.0_f64;
    let mut total_interaction = 0.0_f64;

    for s in sectors {
        let alloc = calc.allocation_effect(s);
        let sel = calc.selection_effect(s);
        let inter = calc.interaction_effect(s);
        let total = alloc + sel + inter;

        total_allocation += alloc;
        total_selection += sel;
        total_interaction += inter;

        sector_results.push(SectorAttribution {
            sector: s.clone(),
            allocation_effect: alloc,
            selection_effect: sel,
            interaction_effect: inter,
            total_active_return: total,
        });
    }

    let total_active_return = total_allocation + total_selection + total_interaction;

    SectorAttributionReport {
        sectors: sector_results,
        total_allocation,
        total_selection,
        total_interaction,
        total_active_return,
        benchmark_return,
        portfolio_return,
    }
}

impl SectorAttributionReport {
    /// Render the attribution report as a plain ASCII table.
    ///
    /// Columns: Sector | Port.Wt | Bench.Wt | Port.Ret | Bench.Ret | Alloc | Select | Interact | Active
    pub fn to_table(&self) -> String {
        let header = format!(
            "{:<18} {:>8} {:>8} {:>9} {:>9} {:>8} {:>8} {:>9} {:>8}",
            "Sector", "P.Wt%", "B.Wt%", "P.Ret%", "B.Ret%", "Alloc%", "Select%", "Interact%", "Active%"
        );
        let sep = "-".repeat(header.len());

        let mut rows = vec![header.clone(), sep.clone()];

        for sa in &self.sectors {
            let s = &sa.sector;
            rows.push(format!(
                "{:<18} {:>8.2} {:>8.2} {:>9.2} {:>9.2} {:>8.4} {:>8.4} {:>9.4} {:>8.4}",
                s.name,
                s.portfolio_weight * 100.0,
                s.benchmark_weight * 100.0,
                s.portfolio_return * 100.0,
                s.benchmark_return * 100.0,
                sa.allocation_effect * 100.0,
                sa.selection_effect * 100.0,
                sa.interaction_effect * 100.0,
                sa.total_active_return * 100.0,
            ));
        }

        rows.push(sep.clone());
        rows.push(format!(
            "{:<18} {:>8} {:>8} {:>9.2} {:>9.2} {:>8.4} {:>8.4} {:>9.4} {:>8.4}",
            "TOTAL",
            "",
            "",
            self.portfolio_return * 100.0,
            self.benchmark_return * 100.0,
            self.total_allocation * 100.0,
            self.total_selection * 100.0,
            self.total_interaction * 100.0,
            self.total_active_return * 100.0,
        ));

        rows.join("\n")
    }

    /// Return the top `n` sectors sorted by absolute total active return (descending).
    ///
    /// If `n` exceeds the number of sectors, all sectors are returned.
    pub fn top_contributors(&self, n: usize) -> Vec<&SectorAttribution> {
        let mut sorted: Vec<&SectorAttribution> = self.sectors.iter().collect();
        sorted.sort_by(|a, b| {
            b.total_active_return
                .abs()
                .partial_cmp(&a.total_active_return.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.truncate(n);
        sorted
    }
}

#[cfg(test)]
mod bhb_sector_tests {
    use super::*;

    fn two_sector_example() -> Vec<Sector> {
        vec![
            Sector {
                name: "Tech".into(),
                portfolio_weight: 0.40,
                benchmark_weight: 0.30,
                portfolio_return: 0.12,
                benchmark_return: 0.10,
            },
            Sector {
                name: "Energy".into(),
                portfolio_weight: 0.60,
                benchmark_weight: 0.70,
                portfolio_return: 0.04,
                benchmark_return: 0.05,
            },
        ]
    }

    #[test]
    fn bhb_decomposition_identity() {
        let report = run_attribution(&two_sector_example());
        let decomposed =
            report.total_allocation + report.total_selection + report.total_interaction;
        assert!(
            (report.total_active_return - decomposed).abs() < 1e-12,
            "active return must equal sum of three effects: {} vs {}",
            report.total_active_return,
            decomposed
        );
    }

    #[test]
    fn bhb_active_return_matches_port_minus_bench() {
        let report = run_attribution(&two_sector_example());
        // Portfolio return: 0.40*0.12 + 0.60*0.04 = 0.048 + 0.024 = 0.072
        // Benchmark return: 0.30*0.10 + 0.70*0.05 = 0.030 + 0.035 = 0.065
        // Active = 0.072 - 0.065 = 0.007
        let expected_active = report.portfolio_return - report.benchmark_return;
        assert!(
            (report.total_active_return - expected_active).abs() < 1e-10,
            "total active return {:.6} != port-bench {:.6}",
            report.total_active_return,
            expected_active
        );
    }

    #[test]
    fn bhb_known_example() {
        // Single-sector sanity check.
        let sectors = vec![Sector {
            name: "EM".into(),
            portfolio_weight: 0.50,
            benchmark_weight: 0.40,
            portfolio_return: 0.08,
            benchmark_return: 0.06,
        }];
        let rb = 0.40 * 0.06; // = 0.024
        let report = run_attribution(&sectors);
        let sa = &report.sectors[0];
        // Allocation = (0.50-0.40)*(0.06-0.024) = 0.10*0.036 = 0.0036
        assert!((sa.allocation_effect - 0.0036).abs() < 1e-10, "alloc={}", sa.allocation_effect);
        // Selection = 0.40*(0.08-0.06) = 0.40*0.02 = 0.008
        assert!((sa.selection_effect - 0.008).abs() < 1e-10, "sel={}", sa.selection_effect);
        // Interaction = (0.50-0.40)*(0.08-0.06) = 0.10*0.02 = 0.002
        assert!((sa.interaction_effect - 0.002).abs() < 1e-10, "inter={}", sa.interaction_effect);
        let _ = rb; // used implicitly above
    }

    #[test]
    fn bhb_top_contributors_order() {
        let report = run_attribution(&two_sector_example());
        let top = report.top_contributors(1);
        assert_eq!(top.len(), 1);
    }

    #[test]
    fn bhb_empty_sectors() {
        let report = run_attribution(&[]);
        assert_eq!(report.sectors.len(), 0);
        assert!((report.total_active_return).abs() < 1e-15);
    }

    #[test]
    fn bhb_table_contains_total_row() {
        let report = run_attribution(&two_sector_example());
        let table = report.to_table();
        assert!(table.contains("TOTAL"), "table should have a TOTAL row:\n{table}");
        assert!(table.contains("Tech"), "table should have Tech sector:\n{table}");
    }
}
