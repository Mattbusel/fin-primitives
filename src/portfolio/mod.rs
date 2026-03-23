//! Portfolio construction and optimization.
//!
//! ## Modules
//!
//! - [`optimizer`]: Markowitz mean-variance optimization (MinVariance, MaxSharpe,
//!   RiskParity, EqualWeight) via projected gradient descent.

pub mod optimizer;

/// Mean-variance portfolio optimization: gradient-based Sharpe maximization,
/// variance minimization, efficient frontier generation, and constraint projection.
pub mod optimization;

/// Black-Litterman portfolio optimization model: blends equilibrium returns with investor views.
pub mod black_litterman;

/// Portfolio diversification metrics: HHI, effective-N, Gini, concentration ratio,
/// diversification ratio, Sortino, Calmar, Treynor, information ratio.
pub mod diversification;

/// Brinson-Hood-Beebower performance attribution: allocation, selection, interaction,
/// Brinson-Fachler variant, and sector-level summaries.
pub mod attribution;

/// Multi-factor risk model: OLS regression, Fama-French 3-factor, APT,
/// information ratio, factor contributions, and systematic/idiosyncratic decomposition.
pub mod factor_model;

/// Portfolio rebalancing strategies: trigger logic, drift analytics, plan generation,
/// cost estimation, tax-lot optimisation, and minimum-variance rebalancing.
pub mod rebalancing;

/// CDO tranching, waterfall cashflow distribution, and Monte Carlo CDO pricing
/// via Gaussian one-factor copula.
pub mod structured_products;

pub use optimizer::{
    Asset, Constraint, CovarianceMatrix, OptimizationObjective, OptimizedPortfolio,
    PortfolioOptimizer,
};
pub use diversification::{
    DiversificationReport, calmar_ratio, concentration_ratio, diversification_ratio,
    effective_n, gini_coefficient, herfindahl_hirschman, information_ratio,
    max_drawdown_portfolio, sortino_ratio, treynor_ratio,
};
pub use attribution::{
    BHBAttribution, BHBAttributor, Segment, bf_allocation_effect,
};
