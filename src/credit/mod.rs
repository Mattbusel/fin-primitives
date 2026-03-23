//! Credit analytics module.
//!
//! Provides credit default swap pricing with hazard rate models.

pub mod cds;

pub use cds::{
    CdsSpec, CdsValuation, HazardCurve,
    cds_cs01, cds_npv, par_spread, premium_leg_pv, protection_leg_pv,
};
