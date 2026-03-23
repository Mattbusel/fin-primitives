//! Portfolio construction and optimization.
//!
//! ## Modules
//!
//! - [`optimizer`]: Markowitz mean-variance optimization (MinVariance, MaxSharpe,
//!   RiskParity, EqualWeight) via projected gradient descent.

pub mod optimizer;

pub use optimizer::{
    Asset, Constraint, CovarianceMatrix, OptimizationObjective, OptimizedPortfolio,
    PortfolioOptimizer,
};
