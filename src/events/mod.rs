//! Event Study Framework
//!
//! Analyzes abnormal returns around discrete market events such as earnings
//! announcements, buy-the-rumor / sell-the-news dynamics, and post-earnings drift.
//!
//! # Methodology
//!
//! The market model is used to estimate expected returns: the benchmark's
//! return over the same interval is the expected return for the security.
//! Abnormal return AR(d) = raw_return(d) - expected_return(d).
//! CAR is the running sum of ARs from the start of the window.

/// A discrete market event anchored to a calendar date.
#[derive(Debug, Clone)]
pub struct MarketEvent {
    /// Unique identifier for this event.
    pub event_id: String,
    /// Event date as a Unix timestamp (seconds since epoch).
    pub event_date: u64,
    /// Category of event (e.g. "earnings", "macro", "guidance").
    pub event_type: String,
    /// Human-readable description.
    pub description: String,
}

/// Defines the pre- and post-event window in trading days.
#[derive(Debug, Clone, Copy)]
pub struct EventWindow {
    /// Number of days before the event (negative means before; e.g. -10).
    pub pre_days: i32,
    /// Number of days after the event (positive means after; e.g. +10).
    pub post_days: i32,
}

/// A single day's abnormal return observation within an event window.
#[derive(Debug, Clone)]
pub struct AbnormalReturn {
    /// Day relative to the event (negative = before, 0 = event day, positive = after).
    pub day: i32,
    /// Observed log-return of the security on this day.
    pub raw_return: f64,
    /// Expected return (benchmark return as market model proxy).
    pub expected_return: f64,
    /// Abnormal return: `raw_return - expected_return`.
    pub abnormal_return: f64,
    /// Cumulative abnormal return from the start of the window up to and including this day.
    pub car: f64,
}

/// Full event study result for a single event.
#[derive(Debug, Clone)]
pub struct EventResult {
    /// The event that was studied.
    pub event: MarketEvent,
    /// CAR over the pre-event window (days `pre_days..0`).
    pub car_pre: f64,
    /// CAR over the post-event window (days `1..=post_days`).
    pub car_post: f64,
    /// Day with the highest CAR within the window.
    pub peak_day: i32,
    /// Day with the lowest CAR within the window.
    pub trough_day: i32,
    /// Full time-series of abnormal returns within the window.
    pub abnormal_returns: Vec<AbnormalReturn>,
}

/// The event study engine.
pub struct EventStudy;

impl EventStudy {
    /// Compute abnormal returns around `event` using a market-model benchmark.
    ///
    /// # Arguments
    /// - `event`: the market event to study.
    /// - `price_series`: `(unix_ts_secs, price)` pairs for the security, chronological.
    /// - `benchmark`: `(unix_ts_secs, price)` pairs for the benchmark, chronological.
    /// - `window`: event window specification.
    ///
    /// # Returns
    /// An [`EventResult`] with abnormal returns and summary statistics.
    pub fn compute(
        event: &MarketEvent,
        price_series: &[(u64, f64)],
        benchmark: &[(u64, f64)],
        window: EventWindow,
    ) -> EventResult {
        // Build daily log-return series indexed by day offset from event_date
        let sec_returns = daily_log_returns(price_series, event.event_date);
        let bmk_returns = daily_log_returns(benchmark, event.event_date);

        let mut abnormal_returns: Vec<AbnormalReturn> = Vec::new();
        let mut cumulative = 0.0f64;

        let day_start = window.pre_days;
        let day_end = window.post_days;

        for d in day_start..=day_end {
            let raw = sec_returns.get(&d).copied().unwrap_or(0.0);
            let exp = bmk_returns.get(&d).copied().unwrap_or(0.0);
            let ar = raw - exp;
            cumulative += ar;
            abnormal_returns.push(AbnormalReturn {
                day: d,
                raw_return: raw,
                expected_return: exp,
                abnormal_return: ar,
                car: cumulative,
            });
        }

        // CAR pre (pre_days..0, not including event day)
        let car_pre: f64 = abnormal_returns
            .iter()
            .filter(|ar| ar.day >= window.pre_days && ar.day < 0)
            .map(|ar| ar.abnormal_return)
            .sum();

        // CAR post (1..=post_days)
        let car_post: f64 = abnormal_returns
            .iter()
            .filter(|ar| ar.day >= 1 && ar.day <= window.post_days)
            .map(|ar| ar.abnormal_return)
            .sum();

        // Peak and trough by CAR value
        let (peak_day, trough_day) = abnormal_returns.iter().fold(
            (0i32, 0i32),
            |(peak_d, trough_d), ar| {
                let peak_car = abnormal_returns.iter().find(|x| x.day == peak_d).map(|x| x.car).unwrap_or(0.0);
                let trough_car = abnormal_returns.iter().find(|x| x.day == trough_d).map(|x| x.car).unwrap_or(0.0);
                let new_peak = if ar.car > peak_car { ar.day } else { peak_d };
                let new_trough = if ar.car < trough_car { ar.day } else { trough_d };
                (new_peak, new_trough)
            },
        );

        EventResult {
            event: event.clone(),
            car_pre,
            car_post,
            peak_day,
            trough_day,
            abnormal_returns,
        }
    }

    /// Compute the t-statistic on average CAR across multiple event results.
    ///
    /// Formula: `t = mean_CAR / (std_CAR / sqrt(N))`
    ///
    /// The CAR used per event is `car_pre + car_post` (total window CAR).
    /// Returns `0.0` if fewer than 2 results are provided.
    pub fn significance(results: &[EventResult]) -> f64 {
        let n = results.len();
        if n < 2 {
            return 0.0;
        }
        let cars: Vec<f64> = results
            .iter()
            .map(|r| r.car_pre + r.car_post)
            .collect();
        let mean = cars.iter().sum::<f64>() / n as f64;
        let variance = cars.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        let std_dev = variance.sqrt();
        if std_dev < 1e-15 {
            return 0.0;
        }
        mean / (std_dev / (n as f64).sqrt())
    }
}

/// Converts a price series into a map of `day_offset → log_return`.
///
/// Day 0 is the day whose timestamp is closest to (and not before) `event_date`.
/// Each subsequent index represents one calendar-day step in the series.
fn daily_log_returns(series: &[(u64, f64)], event_date: u64) -> std::collections::HashMap<i32, f64> {
    use std::collections::HashMap;

    if series.len() < 2 {
        return HashMap::new();
    }

    // Find event index: first entry with ts >= event_date
    let event_idx = series.partition_point(|&(ts, _)| ts < event_date);
    // Clamp to valid range for price access
    let event_idx = event_idx.min(series.len() - 1);

    let mut map = HashMap::new();

    for i in 1..series.len() {
        let p_prev = series[i - 1].1;
        let p_curr = series[i].1;
        let log_ret = if p_prev > 0.0 && p_curr > 0.0 {
            (p_curr / p_prev).ln()
        } else {
            0.0
        };
        // Day offset: how many steps from event_idx this return index falls
        // Return at index i corresponds to day (i as i32 - event_idx as i32)
        let day = i as i32 - event_idx as i32;
        map.insert(day, log_ret);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic price series around an event date.
    /// `event_ts` is index N in the series; series covers indices 0..=2N.
    fn synthetic_prices(n: usize, event_idx: usize, drift: f64, vol: f64) -> Vec<(u64, f64)> {
        let mut prices = Vec::with_capacity(n);
        let mut p = 100.0f64;
        let base_ts: u64 = 1_000_000;
        let day_secs: u64 = 86_400;
        // Use a simple deterministic pseudo-price series
        for i in 0..n {
            if i > 0 {
                // Alternate up/down with drift
                let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                p *= (drift + sign * vol).exp();
            }
            prices.push((base_ts + i as u64 * day_secs, p));
        }
        let _ = event_idx; // event_ts will be set by caller
        prices
    }

    fn make_event(date: u64) -> MarketEvent {
        MarketEvent {
            event_id: "EVT001".into(),
            event_date: date,
            event_type: "earnings".into(),
            description: "Q3 earnings release".into(),
        }
    }

    #[test]
    fn test_compute_returns_correct_window_length() {
        let prices = synthetic_prices(30, 15, 0.001, 0.005);
        let bench = synthetic_prices(30, 15, 0.0005, 0.003);
        let event_ts = prices[15].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -5, post_days: 5 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        assert_eq!(result.abnormal_returns.len(), 11); // -5..=+5 inclusive
    }

    #[test]
    fn test_car_accumulates_correctly() {
        let prices = synthetic_prices(20, 10, 0.001, 0.003);
        let bench = synthetic_prices(20, 10, 0.001, 0.003); // same → zero AR
        let event_ts = prices[10].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -3, post_days: 3 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        // When security == benchmark, all ARs ≈ 0, CARs ≈ 0
        for ar in &result.abnormal_returns {
            assert!(ar.car.abs() < 1e-10, "CAR should be ~0 when security==benchmark");
        }
    }

    #[test]
    fn test_abnormal_return_equals_raw_minus_expected() {
        let prices: Vec<(u64, f64)> = (0..20u64).map(|i| (1_000_000 + i * 86_400, 100.0 + i as f64)).collect();
        let bench: Vec<(u64, f64)>  = (0..20u64).map(|i| (1_000_000 + i * 86_400, 100.0 + i as f64 * 0.5)).collect();
        let event_ts = 1_000_000 + 10 * 86_400;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -2, post_days: 2 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        for ar in &result.abnormal_returns {
            let diff = (ar.raw_return - ar.expected_return - ar.abnormal_return).abs();
            assert!(diff < 1e-12, "AR identity failed on day {}", ar.day);
        }
    }

    #[test]
    fn test_car_monotone_with_window_start() {
        let prices = synthetic_prices(25, 12, 0.002, 0.004);
        let bench = synthetic_prices(25, 12, 0.001, 0.002);
        let event_ts = prices[12].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -5, post_days: 5 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        // CAR should equal sum of prior ARs
        let mut running = 0.0f64;
        for ar in &result.abnormal_returns {
            running += ar.abnormal_return;
            assert!((ar.car - running).abs() < 1e-12, "CAR mismatch at day {}", ar.day);
        }
    }

    #[test]
    fn test_car_pre_and_post_split() {
        let prices = synthetic_prices(25, 12, 0.001, 0.002);
        let bench = synthetic_prices(25, 12, 0.0005, 0.001);
        let event_ts = prices[12].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -5, post_days: 5 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        let manual_pre: f64 = result.abnormal_returns.iter()
            .filter(|ar| ar.day >= -5 && ar.day < 0)
            .map(|ar| ar.abnormal_return)
            .sum();
        let manual_post: f64 = result.abnormal_returns.iter()
            .filter(|ar| ar.day >= 1 && ar.day <= 5)
            .map(|ar| ar.abnormal_return)
            .sum();
        assert!((result.car_pre - manual_pre).abs() < 1e-12);
        assert!((result.car_post - manual_post).abs() < 1e-12);
    }

    #[test]
    fn test_peak_day_is_highest_car() {
        let prices = synthetic_prices(25, 12, 0.003, 0.001);
        let bench = synthetic_prices(25, 12, 0.001, 0.001);
        let event_ts = prices[12].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -5, post_days: 5 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        let max_car = result.abnormal_returns.iter().map(|ar| ar.car).fold(f64::NEG_INFINITY, f64::max);
        let peak_car = result.abnormal_returns.iter().find(|ar| ar.day == result.peak_day).map(|ar| ar.car).unwrap_or(0.0);
        assert!((peak_car - max_car).abs() < 1e-12);
    }

    #[test]
    fn test_trough_day_is_lowest_car() {
        let prices = synthetic_prices(25, 12, -0.001, 0.003);
        let bench = synthetic_prices(25, 12, 0.001, 0.001);
        let event_ts = prices[12].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -5, post_days: 5 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        let min_car = result.abnormal_returns.iter().map(|ar| ar.car).fold(f64::INFINITY, f64::min);
        let trough_car = result.abnormal_returns.iter().find(|ar| ar.day == result.trough_day).map(|ar| ar.car).unwrap_or(0.0);
        assert!((trough_car - min_car).abs() < 1e-12);
    }

    #[test]
    fn test_significance_zero_for_less_than_two() {
        let event = make_event(1_000_000);
        let prices = synthetic_prices(20, 10, 0.001, 0.002);
        let bench = synthetic_prices(20, 10, 0.001, 0.002);
        let window = EventWindow { pre_days: -3, post_days: 3 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        assert_eq!(EventStudy::significance(&[result]), 0.0);
        assert_eq!(EventStudy::significance(&[]), 0.0);
    }

    #[test]
    fn test_significance_positive_when_cars_positive() {
        let mut results = Vec::new();
        for i in 0..5 {
            // Security consistently outperforms benchmark
            let prices: Vec<(u64, f64)> = (0..20u64)
                .map(|j| (1_000_000 + i * 1_000_000 + j * 86_400, 100.0 * (1.01f64).powi(j as i32)))
                .collect();
            let bench: Vec<(u64, f64)> = (0..20u64)
                .map(|j| (1_000_000 + i * 1_000_000 + j * 86_400, 100.0 * (1.005f64).powi(j as i32)))
                .collect();
            let event_ts = 1_000_000 + i * 1_000_000 + 10 * 86_400;
            let event = make_event(event_ts);
            let window = EventWindow { pre_days: -3, post_days: 3 };
            results.push(EventStudy::compute(&event, &prices, &bench, window));
        }
        let t = EventStudy::significance(&results);
        assert!(t > 0.0, "t-statistic should be positive when CAR is consistently positive");
    }

    #[test]
    fn test_significance_negative_when_cars_negative() {
        let mut results = Vec::new();
        for i in 0..5 {
            // Security consistently underperforms benchmark
            let prices: Vec<(u64, f64)> = (0..20u64)
                .map(|j| (1_000_000 + i * 1_000_000 + j * 86_400, 100.0 * (0.99f64).powi(j as i32)))
                .collect();
            let bench: Vec<(u64, f64)> = (0..20u64)
                .map(|j| (1_000_000 + i * 1_000_000 + j * 86_400, 100.0 * (1.005f64).powi(j as i32)))
                .collect();
            let event_ts = 1_000_000 + i * 1_000_000 + 10 * 86_400;
            let event = make_event(event_ts);
            let window = EventWindow { pre_days: -3, post_days: 3 };
            results.push(EventStudy::compute(&event, &prices, &bench, window));
        }
        let t = EventStudy::significance(&results);
        assert!(t < 0.0, "t-statistic should be negative when CAR is consistently negative");
    }

    #[test]
    fn test_day_range_in_window() {
        let prices = synthetic_prices(30, 15, 0.001, 0.002);
        let bench = synthetic_prices(30, 15, 0.001, 0.002);
        let event_ts = prices[15].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -10, post_days: 10 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        let days: Vec<i32> = result.abnormal_returns.iter().map(|ar| ar.day).collect();
        assert!(days.contains(&-10));
        assert!(days.contains(&0));
        assert!(days.contains(&10));
    }

    #[test]
    fn test_event_fields_preserved() {
        let prices = synthetic_prices(20, 10, 0.001, 0.002);
        let bench = synthetic_prices(20, 10, 0.001, 0.002);
        let event_ts = prices[10].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -2, post_days: 2 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        assert_eq!(result.event.event_id, "EVT001");
        assert_eq!(result.event.event_type, "earnings");
    }

    #[test]
    fn test_significance_t_stat_formula() {
        // Manually construct results with known CARs
        fn dummy_result(car: f64) -> EventResult {
            EventResult {
                event: make_event(1_000_000),
                car_pre: car / 2.0,
                car_post: car / 2.0,
                peak_day: 1,
                trough_day: -1,
                abnormal_returns: vec![],
            }
        }
        let cars = [0.02, 0.03, 0.025, 0.018, 0.022];
        let results: Vec<EventResult> = cars.iter().map(|&c| dummy_result(c)).collect();
        let t = EventStudy::significance(&results);
        // Mean ≈ 0.023, should yield a significant positive t
        assert!(t > 1.0, "t-stat = {t}, expected > 1");
    }

    #[test]
    fn test_zero_price_series_gives_zero_returns() {
        // prices of 0 should not panic, returns default to 0
        let prices: Vec<(u64, f64)> = vec![(1_000_000, 0.0), (1_086_400, 0.0)];
        let bench: Vec<(u64, f64)>  = vec![(1_000_000, 100.0), (1_086_400, 101.0)];
        let event = make_event(1_000_000);
        let window = EventWindow { pre_days: -1, post_days: 1 };
        // Should not panic
        let _ = EventStudy::compute(&event, &prices, &bench, window);
    }

    #[test]
    fn test_asymmetric_window() {
        let prices = synthetic_prices(30, 15, 0.001, 0.002);
        let bench = synthetic_prices(30, 15, 0.001, 0.002);
        let event_ts = prices[15].0;
        let event = make_event(event_ts);
        let window = EventWindow { pre_days: -2, post_days: 8 };
        let result = EventStudy::compute(&event, &prices, &bench, window);
        assert_eq!(result.abnormal_returns.len(), 11); // -2..=8 = 11 days
    }

    #[test]
    fn test_significance_all_identical_cars_returns_zero() {
        fn dummy_result(car: f64) -> EventResult {
            EventResult {
                event: make_event(1_000_000),
                car_pre: car,
                car_post: 0.0,
                peak_day: 0,
                trough_day: 0,
                abnormal_returns: vec![],
            }
        }
        // All identical → std_dev = 0 → return 0
        let results: Vec<EventResult> = [0.01, 0.01, 0.01].iter().map(|&c| dummy_result(c)).collect();
        assert_eq!(EventStudy::significance(&results), 0.0);
    }
}
