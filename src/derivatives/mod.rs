//! Derivative pricing modules.
//!
//! | Sub-module | Contents |
//! |------------|----------|
//! | [`swaps`] | Interest rate swap pricing: discount curve, par rate, DV01, NPV |

pub mod swaps;

/// Futures pricing, basis analytics, calendar spreads, and roll yield.
pub mod futures;
