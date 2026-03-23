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
