//! Fixed income analytics: bond pricing, duration, convexity, and yield calculations.
//!
//! ## Modules
//!
//! - [`bond`]: Full bond pricing engine — price, YTM (Brent's method), Macaulay/modified
//!   duration, convexity, DV01, and price-change approximation.

pub mod bond;

/// Bond spread analytics: Z-spread, OAS, I-spread, and asset-swap spread.
pub mod spread_analytics;

/// Interest rate swap pricing: fixed/floating legs, discount curve bootstrapping,
/// par swap rate, DV01, and modified duration.
pub mod swaps;

/// Mortgage calculations: amortization schedules, prepayment savings, refinance breakeven, and LTV.
pub mod mortgage;

pub use mortgage::{AmortizationRow, MortgageCalculator, MortgageTerms};

pub use bond::{
    Bond, CouponFrequency,
    convexity, current_yield, dv01, macaulay_duration, modified_duration,
    price, price_change_approximation, yield_to_maturity, zero_coupon_bond_price,
};
