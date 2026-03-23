//! Derivative pricing modules.
//!
//! | Sub-module | Contents |
//! |------------|----------|
//! | [`swaps`] | Interest rate swap pricing: discount curve, par rate, DV01, NPV |

pub mod swaps;

/// Futures pricing, basis analytics, calendar spreads, and roll yield.
pub mod futures;

/// Multi-leg option strategy analytics: straddle, strangle, collar, butterfly, P&L profiles.
pub mod option_strategies;

/// Exotic option pricing: barrier, Asian, lookback, and digital options.
pub mod exotic_options;

/// Variance swap pricing, realised variance tracking, and VIX replication.
pub mod variance_swap;

/// Forward contract pricing: equity, FX, commodity forwards, and forward curves.
pub mod forwards;

/// Short-rate models: Vasicek, CIR, and Hull-White with bond pricing and simulation.
pub mod interest_rate_models;

/// Credit Default Swap pricing: protection/premium legs, par spread, CS01, and implied hazard rates.
pub mod credit_default_swap;
