//! Fixed income analytics: bond pricing, duration, convexity, and yield calculations.
//!
//! ## Modules
//!
//! - [`bond`]: Full bond pricing engine — price, YTM (Brent's method), Macaulay/modified
//!   duration, convexity, DV01, and price-change approximation.

pub mod bond;

pub use bond::{
    Bond, CouponFrequency,
    convexity, current_yield, dv01, macaulay_duration, modified_duration,
    price, price_change_approximation, yield_to_maturity, zero_coupon_bond_price,
};
