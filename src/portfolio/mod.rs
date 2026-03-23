//! Portfolio construction and optimization.
//!
//! ## Modules
//!
//! - [`optimizer`]: Markowitz mean-variance optimization (MinVariance, MaxSharpe,
//!   RiskParity, EqualWeight) via projected gradient descent.

pub mod optimizer;

/// Portfolio diversification metrics: HHI, effective-N, Gini, concentration ratio,
/// diversification ratio, Sortino, Calmar, Treynor, information ratio.
pub mod diversification;

pub use optimizer::{
    Asset, Constraint, CovarianceMatrix, OptimizationObjective, OptimizedPortfolio,
    PortfolioOptimizer,
};
pub use diversification::{
    DiversificationReport, calmar_ratio, concentration_ratio, diversification_ratio,
    effective_n, gini_coefficient, herfindahl_hirschman, information_ratio,
    max_drawdown_portfolio, sortino_ratio, treynor_ratio,
};
