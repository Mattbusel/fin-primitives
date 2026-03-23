//! # Module: funding
//!
//! Funding rate calculations for perpetual futures contracts.
//!
//! ## Key Types
//!
//! - [`FundingRate`] — a single funding rate observation (rate, timestamp, interval)
//! - [`FundingPayment`] — computed payment for a position at a given rate
//! - [`FundingRateCalculator`] — stateless calculation helpers
//! - [`FundingHistory`] — rolling history with aggregation and statistics

use std::collections::VecDeque;

// ─────────────────────────────────────────
//  FundingRate
// ─────────────────────────────────────────

/// A single perpetual-futures funding rate observation.
///
/// Funding intervals are typically 8 hours (3× per day).
#[derive(Debug, Clone, PartialEq)]
pub struct FundingRate {
    /// Raw funding rate for the period (e.g. `0.0001` = 0.01%).
    pub rate: f64,
    /// Unix timestamp (seconds) of this observation.
    pub timestamp: u64,
    /// Interval length in hours (usually 8).
    pub interval_hours: u8,
}

// ─────────────────────────────────────────
//  FundingPayment
// ─────────────────────────────────────────

/// The funding payment due for a single settlement.
///
/// When `funding_rate > 0`, longs pay shorts; when negative, shorts pay longs.
#[derive(Debug, Clone, PartialEq)]
pub struct FundingPayment {
    /// Notional position size (in base currency units).
    pub position_size: f64,
    /// Funding rate that was applied.
    pub funding_rate: f64,
    /// Computed payment amount (positive = outflow for long, inflow for short).
    pub payment: f64,
    /// Whether the position is long (`true`) or short (`false`).
    pub is_long: bool,
}

// ─────────────────────────────────────────
//  FundingRateCalculator
// ─────────────────────────────────────────

/// Stateless funding rate calculation utilities.
pub struct FundingRateCalculator;

impl FundingRateCalculator {
    /// Compute the premium index: `(mark_price - spot_price) / spot_price`.
    ///
    /// Returns `0.0` if `spot_price` is zero to avoid division by zero.
    pub fn premium_index(mark_price: f64, spot_price: f64) -> f64 {
        if spot_price == 0.0 {
            return 0.0;
        }
        (mark_price - spot_price) / spot_price
    }

    /// Compute the funding rate clamped to `[-clamp, clamp]`.
    ///
    /// `funding = clamp(premium + interest_rate, -clamp, clamp)`
    ///
    /// The default clamp value used by most exchanges is `0.0005` (0.05%).
    pub fn compute_funding_rate(premium: f64, interest_rate: f64, clamp: f64) -> f64 {
        let raw = premium + interest_rate;
        raw.clamp(-clamp, clamp)
    }

    /// Compute the funding payment for a position.
    ///
    /// - Long positions: payment is positive when rate > 0 (they pay shorts).
    /// - Short positions: payment is positive when rate < 0 (they pay longs).
    pub fn compute_payment(
        position_size: f64,
        funding_rate: f64,
        is_long: bool,
    ) -> FundingPayment {
        // Payment = position_size * funding_rate; direction flips for shorts.
        let raw_payment = position_size * funding_rate;
        let payment = if is_long { raw_payment } else { -raw_payment };
        FundingPayment {
            position_size,
            funding_rate,
            payment,
            is_long,
        }
    }

    /// Annualize a per-interval funding rate.
    ///
    /// `annualized = funding_rate * intervals_per_day * 365`
    pub fn annualize_rate(funding_rate: f64, intervals_per_day: f64) -> f64 {
        funding_rate * intervals_per_day * 365.0
    }

    /// Predict the next funding rate using an exponentially weighted average
    /// of the last three rates (most-recent weight 0.5, next 0.3, oldest 0.2).
    ///
    /// Returns `0.0` if the slice is empty.
    pub fn predicted_next_rate(rates: &[FundingRate]) -> f64 {
        if rates.is_empty() {
            return 0.0;
        }
        // Take up to the last 3 entries (most-recent last).
        let len = rates.len();
        let window: Vec<f64> = rates[len.saturating_sub(3)..]
            .iter()
            .map(|r| r.rate)
            .collect();

        match window.len() {
            1 => window[0],
            2 => 0.6 * window[1] + 0.4 * window[0],
            _ => 0.5 * window[2] + 0.3 * window[1] + 0.2 * window[0],
        }
    }
}

// ─────────────────────────────────────────
//  FundingHistory
// ─────────────────────────────────────────

/// Rolling history of funding rate observations with aggregation helpers.
pub struct FundingHistory {
    records: VecDeque<FundingRate>,
    /// Maximum records retained (oldest are evicted once capacity is exceeded).
    capacity: usize,
}

impl FundingHistory {
    /// Create a new history with the given rolling `capacity`.
    pub fn new(capacity: usize) -> Self {
        Self {
            records: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// Append a new funding rate observation, evicting the oldest if at capacity.
    pub fn add(&mut self, rate: FundingRate) {
        if self.records.len() >= self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(rate);
    }

    /// Average funding rate over the most recent `window` observations.
    ///
    /// Returns `None` if there are no records.
    pub fn avg_rate(&self, window: usize) -> Option<f64> {
        let len = self.records.len();
        if len == 0 {
            return None;
        }
        let take = window.min(len);
        let sum: f64 = self.records.iter().rev().take(take).map(|r| r.rate).sum();
        Some(sum / take as f64)
    }

    /// Cumulative funding payment for a position across the most recent `window` settlements.
    pub fn cumulative_payment(
        &self,
        position_size: f64,
        is_long: bool,
        window: usize,
    ) -> f64 {
        let len = self.records.len();
        let take = window.min(len);
        self.records
            .iter()
            .rev()
            .take(take)
            .map(|r| FundingRateCalculator::compute_payment(position_size, r.rate, is_long).payment)
            .sum()
    }

    /// Standard deviation of funding rates over the most recent `window` observations.
    ///
    /// Returns `None` if there are fewer than 2 records in the window.
    pub fn rate_volatility(&self, window: usize) -> Option<f64> {
        let len = self.records.len();
        if len < 2 {
            return None;
        }
        let take = window.min(len);
        if take < 2 {
            return None;
        }
        let rates: Vec<f64> = self
            .records
            .iter()
            .rev()
            .take(take)
            .map(|r| r.rate)
            .collect();
        let mean = rates.iter().sum::<f64>() / rates.len() as f64;
        let variance =
            rates.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rates.len() - 1) as f64;
        Some(variance.sqrt())
    }
}

// ─────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(r: f64) -> FundingRate {
        FundingRate { rate: r, timestamp: 0, interval_hours: 8 }
    }

    // ── FundingRateCalculator ──────────────────────────────────────────────

    #[test]
    fn premium_index_normal() {
        let pi = FundingRateCalculator::premium_index(101.0, 100.0);
        assert!((pi - 0.01).abs() < 1e-12);
    }

    #[test]
    fn premium_index_zero_spot() {
        assert_eq!(FundingRateCalculator::premium_index(100.0, 0.0), 0.0);
    }

    #[test]
    fn compute_funding_rate_clamped_high() {
        let clamp = 0.0005;
        let result = FundingRateCalculator::compute_funding_rate(0.01, 0.0001, clamp);
        assert!((result - clamp).abs() < 1e-12);
    }

    #[test]
    fn compute_funding_rate_clamped_low() {
        let clamp = 0.0005;
        let result = FundingRateCalculator::compute_funding_rate(-0.01, 0.0001, clamp);
        assert!((result + clamp).abs() < 1e-12);
    }

    #[test]
    fn compute_funding_rate_within_clamp() {
        let result =
            FundingRateCalculator::compute_funding_rate(0.0002, 0.0001, 0.0005);
        assert!((result - 0.0003).abs() < 1e-12);
    }

    #[test]
    fn compute_payment_long_positive_rate() {
        let p = FundingRateCalculator::compute_payment(10_000.0, 0.001, true);
        assert!((p.payment - 10.0).abs() < 1e-9);
        assert!(p.is_long);
    }

    #[test]
    fn compute_payment_short_positive_rate() {
        let p = FundingRateCalculator::compute_payment(10_000.0, 0.001, false);
        assert!((p.payment + 10.0).abs() < 1e-9);
        assert!(!p.is_long);
    }

    #[test]
    fn annualize_rate_8h_intervals() {
        // 3 intervals per day for 8h
        let ann = FundingRateCalculator::annualize_rate(0.0001, 3.0);
        assert!((ann - 0.1095).abs() < 1e-9);
    }

    #[test]
    fn predicted_next_rate_empty() {
        assert_eq!(FundingRateCalculator::predicted_next_rate(&[]), 0.0);
    }

    #[test]
    fn predicted_next_rate_one() {
        let rates = vec![rate(0.001)];
        assert!((FundingRateCalculator::predicted_next_rate(&rates) - 0.001).abs() < 1e-12);
    }

    #[test]
    fn predicted_next_rate_three() {
        let rates = vec![rate(0.002), rate(0.001), rate(0.003)];
        // 0.5*0.003 + 0.3*0.001 + 0.2*0.002 = 0.0015+0.0003+0.0004 = 0.0022
        let pred = FundingRateCalculator::predicted_next_rate(&rates);
        assert!((pred - 0.0022).abs() < 1e-12);
    }

    // ── FundingHistory ─────────────────────────────────────────────────────

    #[test]
    fn history_avg_rate_empty() {
        let h = FundingHistory::new(10);
        assert!(h.avg_rate(5).is_none());
    }

    #[test]
    fn history_avg_rate() {
        let mut h = FundingHistory::new(10);
        h.add(rate(0.001));
        h.add(rate(0.003));
        h.add(rate(0.002));
        let avg = h.avg_rate(3).unwrap();
        assert!((avg - 0.002).abs() < 1e-12);
    }

    #[test]
    fn history_capacity_eviction() {
        let mut h = FundingHistory::new(3);
        for i in 0..5u64 {
            h.add(FundingRate { rate: i as f64, timestamp: i, interval_hours: 8 });
        }
        // Should only retain last 3: 2.0, 3.0, 4.0
        assert_eq!(h.records.len(), 3);
        let avg = h.avg_rate(3).unwrap();
        assert!((avg - 3.0).abs() < 1e-12);
    }

    #[test]
    fn history_cumulative_payment() {
        let mut h = FundingHistory::new(10);
        h.add(rate(0.001));
        h.add(rate(0.002));
        // Long, 10_000 position: 10 + 20 = 30
        let total = h.cumulative_payment(10_000.0, true, 10);
        assert!((total - 30.0).abs() < 1e-9);
    }

    #[test]
    fn history_rate_volatility_insufficient() {
        let mut h = FundingHistory::new(10);
        h.add(rate(0.001));
        assert!(h.rate_volatility(5).is_none());
    }

    #[test]
    fn history_rate_volatility_computed() {
        let mut h = FundingHistory::new(10);
        h.add(rate(0.001));
        h.add(rate(0.003));
        let vol = h.rate_volatility(5).unwrap();
        // sample std dev of [0.001, 0.003]: mean=0.002, variance=2e-6, std=sqrt(2)*1e-3
        let expected = (2.0_f64 * 1e-6_f64).sqrt();
        assert!((vol - expected).abs() < 1e-12);
    }
}
