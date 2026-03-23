//! # Module: microstructure
//!
//! ## Responsibility
//! Tick-level market microstructure metrics: bid-ask spread, Amihud illiquidity,
//! Kyle's lambda (market impact coefficient), and Roll's implied spread.
//!
//! ## Guarantees
//! - Zero panics; all fallible operations return `Result<_, FinError>`
//! - All price/quantity inputs use `rust_decimal::Decimal` for precision
//! - Rolling windows use `VecDeque`; no unbounded allocation
//! - Returns `None` from `get()` methods until the window is full
//!
//! ## NOT Responsible For
//! - Order routing, execution, or risk checks
//! - Persistence

use crate::error::FinError;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

// ─────────────────────────────────────────
//  BidAskSpread
// ─────────────────────────────────────────

/// Rolling average bid-ask spread tracker, expressed in basis points.
///
/// Feed bid/ask prices via [`update`](BidAskSpread::update). Once `window` samples
/// have been seen, [`average_spread_bps`](BidAskSpread::average_spread_bps) returns
/// the rolling average.
///
/// Basis points = `(ask - bid) / mid * 10_000`.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::BidAskSpread;
/// use rust_decimal_macros::dec;
///
/// let mut tracker = BidAskSpread::new(5).unwrap();
/// for _ in 0..5 {
///     tracker.update(dec!(99.90), dec!(100.10)).unwrap();
/// }
/// let spread_bps = tracker.average_spread_bps().unwrap();
/// // spread = 0.20, mid = 100.0 → 20 bps
/// assert!((spread_bps - 20.0).abs() < 0.01);
/// ```
#[derive(Debug)]
pub struct BidAskSpread {
    window: usize,
    /// Rolling buffer of (spread_bps) values.
    buf: VecDeque<f64>,
}

impl BidAskSpread {
    /// Constructs a `BidAskSpread` tracker.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `window == 0`.
    pub fn new(window: usize) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, buf: VecDeque::with_capacity(window) })
    }

    /// Records a bid/ask quote.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `bid >= ask` or `bid <= 0`.
    pub fn update(&mut self, bid: Decimal, ask: Decimal) -> Result<(), FinError> {
        if bid <= Decimal::ZERO {
            return Err(FinError::InvalidInput(format!("bid must be positive, got {bid}")));
        }
        if ask <= bid {
            return Err(FinError::InvalidInput(format!(
                "ask ({ask}) must be greater than bid ({bid})"
            )));
        }
        let mid = (bid + ask) / Decimal::from(2u32);
        let spread = ask - bid;
        let mid_f = mid.to_f64().unwrap_or(0.0);
        let spread_f = spread.to_f64().unwrap_or(0.0);
        if mid_f <= 0.0 {
            return Err(FinError::InvalidInput("mid price must be positive".to_owned()));
        }
        let bps = spread_f / mid_f * 10_000.0;
        self.buf.push_back(bps);
        if self.buf.len() > self.window {
            self.buf.pop_front();
        }
        Ok(())
    }

    /// Returns the rolling average spread in basis points, or `None` if not yet ready.
    pub fn average_spread_bps(&self) -> Option<f64> {
        if self.buf.len() < self.window {
            return None;
        }
        let sum: f64 = self.buf.iter().sum();
        Some(sum / self.buf.len() as f64)
    }

    /// Returns `true` when the window is full.
    pub fn is_ready(&self) -> bool {
        self.buf.len() >= self.window
    }

    /// Returns the configured window size.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of samples buffered.
    pub fn sample_count(&self) -> usize {
        self.buf.len()
    }

    /// Resets the tracker.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

// ─────────────────────────────────────────
//  AmihudIlliquidity
// ─────────────────────────────────────────

/// Rolling Amihud Illiquidity ratio: `|return| / volume`.
///
/// A higher value indicates that prices move more per unit of volume (illiquid market).
///
/// `Illiquidity = mean(|r_t| / V_t)` over the rolling window.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::AmihudIlliquidity;
/// use rust_decimal_macros::dec;
///
/// let mut ai = AmihudIlliquidity::new(3).unwrap();
/// ai.update(dec!(100), dec!(102), dec!(1000)).unwrap();
/// ai.update(dec!(102), dec!(101), dec!(500)).unwrap();
/// ai.update(dec!(101), dec!(103), dec!(800)).unwrap();
/// let illiq = ai.get().unwrap();
/// assert!(illiq > 0.0);
/// ```
#[derive(Debug)]
pub struct AmihudIlliquidity {
    window: usize,
    buf: VecDeque<f64>,
}

impl AmihudIlliquidity {
    /// Constructs an `AmihudIlliquidity` tracker.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `window == 0`.
    pub fn new(window: usize) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, buf: VecDeque::with_capacity(window) })
    }

    /// Records a price observation.
    ///
    /// - `prev_close`: previous period closing price.
    /// - `close`: current period closing price.
    /// - `volume`: trading volume during the period (must be > 0).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `prev_close <= 0`, `close <= 0`, or `volume <= 0`.
    pub fn update(
        &mut self,
        prev_close: Decimal,
        close: Decimal,
        volume: Decimal,
    ) -> Result<(), FinError> {
        if prev_close <= Decimal::ZERO {
            return Err(FinError::InvalidInput("prev_close must be positive".to_owned()));
        }
        if close <= Decimal::ZERO {
            return Err(FinError::InvalidInput("close must be positive".to_owned()));
        }
        if volume <= Decimal::ZERO {
            return Err(FinError::InvalidInput("volume must be positive".to_owned()));
        }
        let pc = prev_close.to_f64().unwrap_or(1.0);
        let c = close.to_f64().unwrap_or(pc);
        let v = volume.to_f64().unwrap_or(1.0);
        let ret = ((c / pc).ln()).abs();
        let ratio = ret / v;
        self.buf.push_back(ratio);
        if self.buf.len() > self.window {
            self.buf.pop_front();
        }
        Ok(())
    }

    /// Returns the rolling Amihud illiquidity ratio, or `None` until ready.
    pub fn get(&self) -> Option<f64> {
        if self.buf.len() < self.window {
            return None;
        }
        let sum: f64 = self.buf.iter().sum();
        Some(sum / self.buf.len() as f64)
    }

    /// Returns `true` when the window is full.
    pub fn is_ready(&self) -> bool {
        self.buf.len() >= self.window
    }

    /// Returns the configured window size.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of samples buffered.
    pub fn sample_count(&self) -> usize {
        self.buf.len()
    }

    /// Resets the tracker.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

// ─────────────────────────────────────────
//  KyleLambda
// ─────────────────────────────────────────

/// Kyle's Lambda — estimated market impact coefficient.
///
/// Estimates how much the price moves per unit of signed order flow (volume imbalance).
/// Computed as OLS slope of price change on signed volume:
///
/// `λ = Cov(ΔP, ΔQ) / Var(ΔQ)`
///
/// where `ΔQ` is signed volume (positive = buy-initiated, negative = sell-initiated).
///
/// Returns `None` until the window is full or if signed volume has zero variance.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::KyleLambda;
/// use rust_decimal_macros::dec;
///
/// let mut kl = KyleLambda::new(4).unwrap();
/// kl.update(dec!(0.10), dec!(200)).unwrap();
/// kl.update(dec!(0.05), dec!(100)).unwrap();
/// kl.update(dec!(-0.08), dec!(-150)).unwrap();
/// kl.update(dec!(0.12), dec!(250)).unwrap();
/// let lambda = kl.get(); // Some(estimated lambda)
/// ```
#[derive(Debug)]
pub struct KyleLambda {
    window: usize,
    /// Buffer of (price_change, signed_volume) pairs.
    buf: VecDeque<(f64, f64)>,
}

impl KyleLambda {
    /// Constructs a `KyleLambda` estimator.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `window < 2`.
    pub fn new(window: usize) -> Result<Self, FinError> {
        if window < 2 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, buf: VecDeque::with_capacity(window) })
    }

    /// Records a price change and signed volume observation.
    ///
    /// - `price_change`: `close_t - close_{t-1}` (can be negative).
    /// - `signed_volume`: net order flow (positive = buy pressure, negative = sell pressure).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if either value is non-finite.
    pub fn update(&mut self, price_change: Decimal, signed_volume: Decimal) -> Result<(), FinError> {
        let dp = price_change.to_f64().ok_or_else(|| {
            FinError::InvalidInput("price_change is not representable as f64".to_owned())
        })?;
        let dq = signed_volume.to_f64().ok_or_else(|| {
            FinError::InvalidInput("signed_volume is not representable as f64".to_owned())
        })?;
        if !dp.is_finite() || !dq.is_finite() {
            return Err(FinError::InvalidInput(
                "price_change and signed_volume must be finite".to_owned(),
            ));
        }
        self.buf.push_back((dp, dq));
        if self.buf.len() > self.window {
            self.buf.pop_front();
        }
        Ok(())
    }

    /// Returns the estimated Kyle's lambda, or `None` until ready.
    pub fn get(&self) -> Option<f64> {
        if self.buf.len() < self.window {
            return None;
        }
        let n = self.buf.len() as f64;
        let mean_dp = self.buf.iter().map(|(dp, _)| dp).sum::<f64>() / n;
        let mean_dq = self.buf.iter().map(|(_, dq)| dq).sum::<f64>() / n;
        let cov: f64 = self.buf.iter().map(|(dp, dq)| (dp - mean_dp) * (dq - mean_dq)).sum::<f64>();
        let var_dq: f64 = self.buf.iter().map(|(_, dq)| (dq - mean_dq).powi(2)).sum::<f64>();
        if var_dq == 0.0 {
            return None;
        }
        Some(cov / var_dq)
    }

    /// Returns `true` when the window is full.
    pub fn is_ready(&self) -> bool {
        self.buf.len() >= self.window
    }

    /// Returns the configured window size.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of samples buffered.
    pub fn sample_count(&self) -> usize {
        self.buf.len()
    }

    /// Resets the estimator.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

// ─────────────────────────────────────────
//  RollImpliedSpread
// ─────────────────────────────────────────

/// Roll's Implied Spread estimator.
///
/// Estimates the effective bid-ask spread from serial autocorrelation of price changes:
///
/// `S = 2 * sqrt(-Cov(ΔP_t, ΔP_{t-1}))` when `Cov < 0`.
///
/// When `Cov >= 0` (no autocorrelation signal), returns `0.0` (no spread implied).
///
/// Returns `None` until `window + 1` price changes have been observed.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::RollImpliedSpread;
/// use rust_decimal_macros::dec;
///
/// let mut roll = RollImpliedSpread::new(10).unwrap();
/// // Alternating returns simulate bid-ask bounce
/// for i in 0..11 {
///     let ret = if i % 2 == 0 { dec!(0.05) } else { dec!(-0.05) };
///     roll.update(ret).unwrap();
/// }
/// let spread = roll.get();
/// assert!(spread.is_some());
/// ```
#[derive(Debug)]
pub struct RollImpliedSpread {
    window: usize,
    /// Rolling buffer of price changes.
    changes: VecDeque<f64>,
}

impl RollImpliedSpread {
    /// Constructs a `RollImpliedSpread` estimator.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `window < 2`.
    pub fn new(window: usize) -> Result<Self, FinError> {
        if window < 2 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self {
            window,
            changes: VecDeque::with_capacity(window + 1),
        })
    }

    /// Records a price change observation.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if the value is non-finite.
    pub fn update(&mut self, price_change: Decimal) -> Result<(), FinError> {
        let dp = price_change.to_f64().ok_or_else(|| {
            FinError::InvalidInput("price_change is not representable as f64".to_owned())
        })?;
        if !dp.is_finite() {
            return Err(FinError::InvalidInput("price_change must be finite".to_owned()));
        }
        self.changes.push_back(dp);
        if self.changes.len() > self.window + 1 {
            self.changes.pop_front();
        }
        Ok(())
    }

    /// Returns the Roll implied spread estimate, or `None` until ready.
    ///
    /// Returns `0.0` when the first-order autocovariance is non-negative (no bounce signal).
    pub fn get(&self) -> Option<f64> {
        if self.changes.len() < self.window + 1 {
            return None;
        }
        let n = self.changes.len();
        // Compute first-order autocovariance: Cov(dp_t, dp_{t-1})
        let mean = self.changes.iter().sum::<f64>() / n as f64;
        let cov: f64 = self
            .changes
            .iter()
            .zip(self.changes.iter().skip(1))
            .map(|(a, b)| (a - mean) * (b - mean))
            .sum::<f64>()
            / (n - 1) as f64;

        if cov >= 0.0 {
            Some(0.0)
        } else {
            let spread = 2.0 * (-cov).sqrt();
            Some(spread)
        }
    }

    /// Returns `true` when the window is full.
    pub fn is_ready(&self) -> bool {
        self.changes.len() >= self.window + 1
    }

    /// Returns the configured window size.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of price changes buffered.
    pub fn sample_count(&self) -> usize {
        self.changes.len()
    }

    /// Resets the estimator.
    pub fn reset(&mut self) {
        self.changes.clear();
    }
}

// ─────────────────────────────────────────
//  OrderImbalance
// ─────────────────────────────────────────

/// Rolling buy/sell volume order imbalance measure.
///
/// `OIR = (V_buy - V_sell) / (V_buy + V_sell)` for each bar.
/// Rolling mean over the configured window.
///
/// Range: `[-1.0, 1.0]`. Positive values indicate buy-side pressure.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::OrderImbalance;
/// use rust_decimal_macros::dec;
///
/// let mut oi = OrderImbalance::new(3).unwrap();
/// oi.update(dec!(600), dec!(400)).unwrap(); // OIR = 0.2
/// oi.update(dec!(700), dec!(300)).unwrap(); // OIR = 0.4
/// oi.update(dec!(800), dec!(200)).unwrap(); // OIR = 0.6
/// let imbalance = oi.get().unwrap();
/// assert!(imbalance > 0.0, "positive buy pressure: {imbalance}");
/// ```
#[derive(Debug)]
pub struct OrderImbalance {
    window: usize,
    /// Rolling buffer of per-bar order imbalance ratios.
    buf: VecDeque<f64>,
}

impl OrderImbalance {
    /// Constructs an `OrderImbalance` tracker.
    ///
    /// # Errors
    /// Returns [`FinError`] if `window == 0`.
    pub fn new(window: usize) -> Result<Self, FinError> {
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self { window, buf: VecDeque::with_capacity(window) })
    }

    /// Records a volume observation.
    ///
    /// - `buy_volume`: aggressive buy volume for the bar (must be >= 0).
    /// - `sell_volume`: aggressive sell volume for the bar (must be >= 0).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if both volumes are zero or either is negative.
    pub fn update(&mut self, buy_volume: Decimal, sell_volume: Decimal) -> Result<(), FinError> {
        if buy_volume < Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "buy_volume must be non-negative".to_owned(),
            ));
        }
        if sell_volume < Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "sell_volume must be non-negative".to_owned(),
            ));
        }
        let total = buy_volume + sell_volume;
        if total == Decimal::ZERO {
            return Err(FinError::InvalidInput(
                "buy_volume + sell_volume must be positive".to_owned(),
            ));
        }
        let bv = buy_volume.to_f64().unwrap_or(0.0);
        let sv = sell_volume.to_f64().unwrap_or(0.0);
        let tot = bv + sv;
        let oir = (bv - sv) / tot;
        self.buf.push_back(oir);
        if self.buf.len() > self.window {
            self.buf.pop_front();
        }
        Ok(())
    }

    /// Returns the rolling mean order imbalance ratio, or `None` until ready.
    pub fn get(&self) -> Option<f64> {
        if self.buf.len() < self.window {
            return None;
        }
        let sum: f64 = self.buf.iter().sum();
        Some(sum / self.buf.len() as f64)
    }

    /// Returns `true` when the window is full.
    pub fn is_ready(&self) -> bool {
        self.buf.len() >= self.window
    }

    /// Returns the configured window size.
    pub fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of samples buffered.
    pub fn sample_count(&self) -> usize {
        self.buf.len()
    }

    /// Resets the tracker.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

// ─────────────────────────────────────────
//  MicrostructureMetrics
// ─────────────────────────────────────────

/// Aggregated snapshot of all available microstructure metrics for a symbol.
///
/// Each field is `Some` if the underlying tracker has enough data (window full),
/// or `None` if still warming up.
#[derive(Debug, Clone, Default)]
pub struct MicrostructureSnapshot {
    /// Rolling average bid-ask spread in basis points.
    pub avg_spread_bps: Option<f64>,
    /// Rolling mean order imbalance ratio `[-1, 1]`.
    pub order_imbalance: Option<f64>,
    /// Kyle's lambda (price impact per unit of signed order flow).
    pub kyle_lambda: Option<f64>,
    /// Amihud illiquidity ratio (`|return| / volume`).
    pub amihud_illiquidity: Option<f64>,
    /// Roll implied spread (autocovariance-based).
    pub roll_spread: Option<f64>,
}

/// Feeds real-time market data into all microstructure estimators and produces
/// aggregate [`MicrostructureSnapshot`]s on demand.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::MicrostructureMetrics;
/// use rust_decimal_macros::dec;
///
/// let mut m = MicrostructureMetrics::new(5).unwrap();
/// for _ in 0..5 {
///     m.update_spread(dec!(99.90), dec!(100.10)).unwrap();
///     m.update_volume_imbalance(dec!(600), dec!(400)).unwrap();
///     m.update_price_impact(dec!(0.05), dec!(100)).unwrap();
///     m.update_amihud(dec!(100), dec!(102), dec!(1000)).unwrap();
///     m.update_roll(dec!(0.05)).unwrap();
/// }
/// let snap = m.snapshot();
/// assert!(snap.avg_spread_bps.is_some());
/// ```
pub struct MicrostructureMetrics {
    spread: BidAskSpread,
    imbalance: OrderImbalance,
    kyle: KyleLambda,
    amihud: AmihudIlliquidity,
    roll: RollImpliedSpread,
}

impl MicrostructureMetrics {
    /// Create a new aggregator with the given rolling window for all sub-trackers.
    ///
    /// `KyleLambda` and `RollImpliedSpread` require `window >= 2`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `window < 2`.
    pub fn new(window: usize) -> Result<Self, FinError> {
        if window < 2 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self {
            spread: BidAskSpread::new(window)?,
            imbalance: OrderImbalance::new(window)?,
            kyle: KyleLambda::new(window)?,
            amihud: AmihudIlliquidity::new(window)?,
            roll: RollImpliedSpread::new(window)?,
        })
    }

    /// Feed a bid/ask quote into the spread tracker.
    ///
    /// # Errors
    /// Propagates errors from [`BidAskSpread::update`].
    pub fn update_spread(&mut self, bid: Decimal, ask: Decimal) -> Result<(), FinError> {
        self.spread.update(bid, ask)
    }

    /// Feed a buy/sell volume observation into the order imbalance tracker.
    ///
    /// # Errors
    /// Propagates errors from [`OrderImbalance::update`].
    pub fn update_volume_imbalance(
        &mut self,
        buy_volume: Decimal,
        sell_volume: Decimal,
    ) -> Result<(), FinError> {
        self.imbalance.update(buy_volume, sell_volume)
    }

    /// Feed a price-change / signed-volume pair into the Kyle's lambda estimator.
    ///
    /// # Errors
    /// Propagates errors from [`KyleLambda::update`].
    pub fn update_price_impact(
        &mut self,
        price_change: Decimal,
        signed_volume: Decimal,
    ) -> Result<(), FinError> {
        self.kyle.update(price_change, signed_volume)
    }

    /// Feed a prev/current close and volume into the Amihud illiquidity estimator.
    ///
    /// # Errors
    /// Propagates errors from [`AmihudIlliquidity::update`].
    pub fn update_amihud(
        &mut self,
        prev_close: Decimal,
        close: Decimal,
        volume: Decimal,
    ) -> Result<(), FinError> {
        self.amihud.update(prev_close, close, volume)
    }

    /// Feed a price change into the Roll spread estimator.
    ///
    /// # Errors
    /// Propagates errors from [`RollImpliedSpread::update`].
    pub fn update_roll(&mut self, price_change: Decimal) -> Result<(), FinError> {
        self.roll.update(price_change)
    }

    /// Produce a snapshot of all currently available metrics.
    ///
    /// Fields are `None` until the underlying rolling window is full.
    pub fn snapshot(&self) -> MicrostructureSnapshot {
        MicrostructureSnapshot {
            avg_spread_bps: self.spread.average_spread_bps(),
            order_imbalance: self.imbalance.get(),
            kyle_lambda: self.kyle.get(),
            amihud_illiquidity: self.amihud.get(),
            roll_spread: self.roll.get(),
        }
    }

    /// Reset all sub-trackers.
    pub fn reset(&mut self) {
        self.spread.reset();
        self.imbalance.reset();
        self.kyle.reset();
        self.amihud.reset();
        self.roll.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // ── BidAskSpread ──────────────────────────────────────────────────────

    #[test]
    fn test_bid_ask_spread_zero_window_fails() {
        assert!(BidAskSpread::new(0).is_err());
    }

    #[test]
    fn test_bid_ask_spread_not_ready_before_window() {
        let mut t = BidAskSpread::new(3).unwrap();
        t.update(dec!(99.9), dec!(100.1)).unwrap();
        t.update(dec!(99.9), dec!(100.1)).unwrap();
        assert!(!t.is_ready());
        assert!(t.average_spread_bps().is_none());
    }

    #[test]
    fn test_bid_ask_spread_correct_bps() {
        let mut t = BidAskSpread::new(3).unwrap();
        // spread=0.20, mid=100.0 → 20 bps
        for _ in 0..3 {
            t.update(dec!(99.90), dec!(100.10)).unwrap();
        }
        let bps = t.average_spread_bps().unwrap();
        assert!((bps - 20.0).abs() < 0.01, "bps={bps}");
    }

    #[test]
    fn test_bid_ask_spread_inverted_fails() {
        let mut t = BidAskSpread::new(3).unwrap();
        assert!(t.update(dec!(101), dec!(100)).is_err());
    }

    #[test]
    fn test_bid_ask_spread_negative_bid_fails() {
        let mut t = BidAskSpread::new(3).unwrap();
        assert!(t.update(dec!(-1), dec!(100)).is_err());
    }

    #[test]
    fn test_bid_ask_spread_reset() {
        let mut t = BidAskSpread::new(2).unwrap();
        t.update(dec!(99), dec!(101)).unwrap();
        t.update(dec!(99), dec!(101)).unwrap();
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
    }

    // ── AmihudIlliquidity ─────────────────────────────────────────────────

    #[test]
    fn test_amihud_zero_window_fails() {
        assert!(AmihudIlliquidity::new(0).is_err());
    }

    #[test]
    fn test_amihud_not_ready_before_window() {
        let mut ai = AmihudIlliquidity::new(3).unwrap();
        ai.update(dec!(100), dec!(102), dec!(1000)).unwrap();
        assert!(!ai.is_ready());
        assert!(ai.get().is_none());
    }

    #[test]
    fn test_amihud_positive_for_price_moves() {
        let mut ai = AmihudIlliquidity::new(3).unwrap();
        ai.update(dec!(100), dec!(105), dec!(1000)).unwrap();
        ai.update(dec!(105), dec!(103), dec!(800)).unwrap();
        ai.update(dec!(103), dec!(107), dec!(1200)).unwrap();
        let illiq = ai.get().unwrap();
        assert!(illiq > 0.0, "illiquidity should be positive: {illiq}");
    }

    #[test]
    fn test_amihud_zero_volume_fails() {
        let mut ai = AmihudIlliquidity::new(3).unwrap();
        assert!(ai.update(dec!(100), dec!(105), dec!(0)).is_err());
    }

    #[test]
    fn test_amihud_reset() {
        let mut ai = AmihudIlliquidity::new(2).unwrap();
        ai.update(dec!(100), dec!(102), dec!(500)).unwrap();
        ai.update(dec!(102), dec!(101), dec!(600)).unwrap();
        assert!(ai.is_ready());
        ai.reset();
        assert!(!ai.is_ready());
    }

    // ── KyleLambda ────────────────────────────────────────────────────────

    #[test]
    fn test_kyle_period_1_fails() {
        assert!(KyleLambda::new(1).is_err());
    }

    #[test]
    fn test_kyle_not_ready_before_window() {
        let mut kl = KyleLambda::new(4).unwrap();
        kl.update(dec!(0.1), dec!(200)).unwrap();
        assert!(!kl.is_ready());
        assert!(kl.get().is_none());
    }

    #[test]
    fn test_kyle_positive_lambda_for_aligned_signals() {
        let mut kl = KyleLambda::new(4).unwrap();
        // Positive price changes with positive volume → positive lambda
        kl.update(dec!(0.10), dec!(100)).unwrap();
        kl.update(dec!(0.20), dec!(200)).unwrap();
        kl.update(dec!(0.15), dec!(150)).unwrap();
        kl.update(dec!(0.25), dec!(250)).unwrap();
        let lambda = kl.get().unwrap();
        assert!(lambda > 0.0, "lambda should be positive: {lambda}");
    }

    #[test]
    fn test_kyle_zero_volume_variance_returns_none() {
        let mut kl = KyleLambda::new(3).unwrap();
        // Constant signed volume → zero variance → None
        kl.update(dec!(0.1), dec!(100)).unwrap();
        kl.update(dec!(0.2), dec!(100)).unwrap();
        kl.update(dec!(0.3), dec!(100)).unwrap();
        assert!(kl.get().is_none());
    }

    #[test]
    fn test_kyle_reset() {
        let mut kl = KyleLambda::new(2).unwrap();
        kl.update(dec!(0.1), dec!(100)).unwrap();
        kl.update(dec!(0.2), dec!(200)).unwrap();
        assert!(kl.is_ready());
        kl.reset();
        assert!(!kl.is_ready());
    }

    // ── RollImpliedSpread ─────────────────────────────────────────────────

    #[test]
    fn test_roll_period_1_fails() {
        assert!(RollImpliedSpread::new(1).is_err());
    }

    #[test]
    fn test_roll_not_ready_before_window() {
        let mut r = RollImpliedSpread::new(5).unwrap();
        r.update(dec!(0.05)).unwrap();
        assert!(!r.is_ready());
        assert!(r.get().is_none());
    }

    #[test]
    fn test_roll_positive_spread_for_alternating_returns() {
        let mut r = RollImpliedSpread::new(10).unwrap();
        for i in 0..11 {
            let ret = if i % 2 == 0 { dec!(0.05) } else { dec!(-0.05) };
            r.update(ret).unwrap();
        }
        let spread = r.get().unwrap();
        assert!(spread > 0.0, "alternating returns should give positive Roll spread: {spread}");
    }

    #[test]
    fn test_roll_zero_spread_for_trending_returns() {
        // All positive returns → no bid-ask bounce → cov >= 0 → spread = 0
        let mut r = RollImpliedSpread::new(5).unwrap();
        for _ in 0..6 {
            r.update(dec!(0.10)).unwrap();
        }
        let spread = r.get().unwrap();
        // Constant returns → zero variance → autocovariance = 0 → spread = 0
        assert_eq!(spread, 0.0);
    }

    #[test]
    fn test_roll_reset() {
        let mut r = RollImpliedSpread::new(3).unwrap();
        for _ in 0..4 {
            r.update(dec!(0.01)).unwrap();
        }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }

    // ── OrderImbalance ────────────────────────────────────────────────────

    #[test]
    fn test_order_imbalance_zero_window_fails() {
        assert!(OrderImbalance::new(0).is_err());
    }

    #[test]
    fn test_order_imbalance_not_ready_before_window() {
        let mut oi = OrderImbalance::new(3).unwrap();
        oi.update(dec!(600), dec!(400)).unwrap();
        assert!(!oi.is_ready());
        assert!(oi.get().is_none());
    }

    #[test]
    fn test_order_imbalance_positive_for_buy_heavy() {
        let mut oi = OrderImbalance::new(3).unwrap();
        oi.update(dec!(800), dec!(200)).unwrap();
        oi.update(dec!(700), dec!(300)).unwrap();
        oi.update(dec!(900), dec!(100)).unwrap();
        let imbalance = oi.get().unwrap();
        assert!(imbalance > 0.0, "expected positive imbalance: {imbalance}");
    }

    #[test]
    fn test_order_imbalance_negative_for_sell_heavy() {
        let mut oi = OrderImbalance::new(3).unwrap();
        oi.update(dec!(200), dec!(800)).unwrap();
        oi.update(dec!(300), dec!(700)).unwrap();
        oi.update(dec!(100), dec!(900)).unwrap();
        let imbalance = oi.get().unwrap();
        assert!(imbalance < 0.0, "expected negative imbalance: {imbalance}");
    }

    #[test]
    fn test_order_imbalance_zero_total_fails() {
        let mut oi = OrderImbalance::new(3).unwrap();
        assert!(oi.update(dec!(0), dec!(0)).is_err());
    }

    #[test]
    fn test_order_imbalance_negative_volume_fails() {
        let mut oi = OrderImbalance::new(3).unwrap();
        assert!(oi.update(dec!(-100), dec!(100)).is_err());
    }

    #[test]
    fn test_order_imbalance_reset() {
        let mut oi = OrderImbalance::new(2).unwrap();
        oi.update(dec!(500), dec!(500)).unwrap();
        oi.update(dec!(500), dec!(500)).unwrap();
        assert!(oi.is_ready());
        oi.reset();
        assert!(!oi.is_ready());
    }

    // ── MicrostructureMetrics ──────────────────────────────────────────────

    #[test]
    fn test_microstructure_metrics_window_too_small_fails() {
        assert!(MicrostructureMetrics::new(1).is_err());
        assert!(MicrostructureMetrics::new(0).is_err());
    }

    #[test]
    fn test_microstructure_metrics_snapshot_none_before_warm() {
        let mut m = MicrostructureMetrics::new(5).unwrap();
        m.update_spread(dec!(99.9), dec!(100.1)).unwrap();
        let snap = m.snapshot();
        assert!(snap.avg_spread_bps.is_none());
        assert!(snap.order_imbalance.is_none());
        assert!(snap.kyle_lambda.is_none());
        assert!(snap.amihud_illiquidity.is_none());
        assert!(snap.roll_spread.is_none());
    }

    #[test]
    fn test_microstructure_metrics_snapshot_some_after_warm() {
        let mut m = MicrostructureMetrics::new(3).unwrap();
        for i in 0..3 {
            m.update_spread(dec!(99.90), dec!(100.10)).unwrap();
            m.update_volume_imbalance(dec!(600), dec!(400)).unwrap();
            m.update_price_impact(
                rust_decimal::prelude::FromPrimitive::from_f64(0.05 + i as f64 * 0.01).unwrap_or(dec!(0.05)),
                rust_decimal::prelude::FromPrimitive::from_f64(100.0 + i as f64 * 50.0).unwrap_or(dec!(100)),
            ).unwrap();
            m.update_amihud(dec!(100), dec!(102), dec!(1000)).unwrap();
            m.update_roll(if i % 2 == 0 { dec!(0.05) } else { dec!(-0.05) }).unwrap();
        }
        let snap = m.snapshot();
        assert!(snap.avg_spread_bps.is_some());
        assert!(snap.order_imbalance.is_some());
        assert!(snap.amihud_illiquidity.is_some());
        // Roll needs window+1 samples; may still be None with 3 samples and window=3
        let _ = snap.roll_spread; // just verify no panic
    }

    #[test]
    fn test_microstructure_metrics_reset() {
        let mut m = MicrostructureMetrics::new(2).unwrap();
        for _ in 0..2 {
            m.update_spread(dec!(99.9), dec!(100.1)).unwrap();
            m.update_volume_imbalance(dec!(500), dec!(500)).unwrap();
            m.update_price_impact(dec!(0.05), dec!(100)).unwrap();
            m.update_amihud(dec!(100), dec!(102), dec!(1000)).unwrap();
            m.update_roll(dec!(0.05)).unwrap();
        }
        m.reset();
        let snap = m.snapshot();
        assert!(snap.avg_spread_bps.is_none());
        assert!(snap.order_imbalance.is_none());
    }

    // ── TradeSign / Lee-Ready ─────────────────────────────────────────────

    #[test]
    fn test_trade_sign_classify_buy() {
        assert_eq!(TradeSign::classify(100.5, 100.0), TradeSign::Buy);
    }

    #[test]
    fn test_trade_sign_classify_sell() {
        assert_eq!(TradeSign::classify(99.5, 100.0), TradeSign::Sell);
    }

    #[test]
    fn test_trade_sign_classify_unknown_equal() {
        assert_eq!(TradeSign::classify(100.0, 100.0), TradeSign::Unknown);
    }

    // ── KyleLambdaEstimate ────────────────────────────────────────────────

    #[test]
    fn test_kyle_lambda_estimate_positive() {
        let changes = vec![0.1, 0.2, 0.15, 0.25, -0.05];
        let volumes = vec![100.0, 200.0, 150.0, 250.0, -50.0];
        let est = KyleLambdaEstimate::estimate(&changes, &volumes);
        assert!(est.lambda > 0.0, "lambda={}", est.lambda);
        assert!((0.0..=1.0).contains(&est.r_squared), "r2={}", est.r_squared);
    }

    #[test]
    fn test_kyle_lambda_estimate_empty() {
        let est = KyleLambdaEstimate::estimate(&[], &[]);
        assert_eq!(est.lambda, 0.0);
        assert_eq!(est.r_squared, 0.0);
    }

    #[test]
    fn test_kyle_lambda_estimate_zero_variance() {
        let changes = vec![0.1, 0.1, 0.1];
        let volumes = vec![100.0, 100.0, 100.0];
        let est = KyleLambdaEstimate::estimate(&changes, &volumes);
        assert_eq!(est.lambda, 0.0);
        assert_eq!(est.r_squared, 0.0);
    }

    // ── HasbrouckShare ────────────────────────────────────────────────────

    #[test]
    fn test_hasbrouck_share_equal_variance() {
        let a = vec![0.1, -0.1, 0.2, -0.2, 0.1];
        let b = vec![0.1, -0.1, 0.2, -0.2, 0.1];
        let hs = HasbrouckShare::estimate(&a, &b);
        // Equal variance → shares close to 0.5 each
        assert!((hs.security_a_share - 0.5).abs() < 0.01, "a={}", hs.security_a_share);
        assert!((hs.security_b_share - 0.5).abs() < 0.01, "b={}", hs.security_b_share);
    }

    #[test]
    fn test_hasbrouck_share_sums_to_one() {
        let a = vec![0.1, 0.2, -0.1, 0.3];
        let b = vec![0.05, 0.1, -0.2, 0.15];
        let hs = HasbrouckShare::estimate(&a, &b);
        let total = hs.security_a_share + hs.security_b_share;
        assert!((total - 1.0).abs() < 1e-10, "total={total}");
    }

    #[test]
    fn test_hasbrouck_share_empty() {
        let hs = HasbrouckShare::estimate(&[], &[]);
        assert_eq!(hs.security_a_share, 0.5);
        assert_eq!(hs.security_b_share, 0.5);
    }

    // ── PinEstimate ───────────────────────────────────────────────────────

    #[test]
    fn test_pin_probability_basic() {
        let buys = vec![80u64, 90, 85, 70, 95];
        let sells = vec![20u64, 10, 15, 30, 5];
        let pin = PinEstimate::from_trade_counts(&buys, &sells);
        let p = pin.pin_probability();
        assert!((0.0..=1.0).contains(&p), "pin={p}");
    }

    #[test]
    fn test_pin_probability_zero_denominator() {
        let pin = PinEstimate { alpha: 0.0, mu: 0.0, epsilon_b: 0.0, epsilon_s: 0.0 };
        assert_eq!(pin.pin_probability(), 0.0);
    }

    #[test]
    fn test_pin_from_empty_counts() {
        let pin = PinEstimate::from_trade_counts(&[], &[]);
        assert_eq!(pin.pin_probability(), 0.0);
    }
}

// ─────────────────────────────────────────
//  TradeSign — Lee-Ready tick classification
// ─────────────────────────────────────────

/// Trade direction classification using the Lee-Ready tick test.
///
/// The tick test compares the current trade price to the previous trade price:
/// - Price increased → the trade is a `Buy` (up-tick).
/// - Price decreased → the trade is a `Sell` (down-tick).
/// - Price unchanged → `Unknown` (zero-tick; caller may apply the previous sign).
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::TradeSign;
///
/// assert_eq!(TradeSign::classify(100.5, 100.0), TradeSign::Buy);
/// assert_eq!(TradeSign::classify(99.5, 100.0), TradeSign::Sell);
/// assert_eq!(TradeSign::classify(100.0, 100.0), TradeSign::Unknown);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSign {
    /// Trade occurred on an up-tick (buy-initiated).
    Buy,
    /// Trade occurred on a down-tick (sell-initiated).
    Sell,
    /// Trade occurred on a zero-tick (direction indeterminate).
    Unknown,
}

impl TradeSign {
    /// Classify a trade using the Lee-Ready tick test.
    ///
    /// - `price`: current trade price.
    /// - `prev_price`: immediately preceding trade price.
    pub fn classify(price: f64, prev_price: f64) -> Self {
        if price > prev_price {
            Self::Buy
        } else if price < prev_price {
            Self::Sell
        } else {
            Self::Unknown
        }
    }
}

// ─────────────────────────────────────────
//  KyleLambdaEstimate — OLS price-impact coefficient
// ─────────────────────────────────────────

/// Kyle's Lambda estimated via OLS regression of price changes on signed volume.
///
/// `lambda` = OLS slope of `price_changes ~ signed_volumes`.
/// `r_squared` = coefficient of determination of that regression (0 to 1).
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::KyleLambdaEstimate;
///
/// let changes = vec![0.1, 0.2, -0.05, 0.15];
/// let volumes = vec![100.0, 200.0, -50.0, 150.0];
/// let est = KyleLambdaEstimate::estimate(&changes, &volumes);
/// assert!(est.lambda >= 0.0 || est.lambda < 0.0); // finite value
/// assert!((0.0..=1.0).contains(&est.r_squared));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct KyleLambdaEstimate {
    /// OLS price-impact coefficient (price change per unit of signed order flow).
    pub lambda: f64,
    /// R-squared of the OLS regression.
    pub r_squared: f64,
}

impl KyleLambdaEstimate {
    /// Estimate Kyle's Lambda from slices of price changes and signed volumes.
    ///
    /// Uses OLS: `lambda = Cov(ΔP, Q) / Var(Q)`.
    /// R-squared is computed as `(Cor(ΔP, Q))^2`.
    ///
    /// Returns `lambda = 0.0` and `r_squared = 0.0` when there are fewer than 2
    /// observations or when signed volume has zero variance.
    pub fn estimate(price_changes: &[f64], signed_volumes: &[f64]) -> Self {
        let n = price_changes.len().min(signed_volumes.len());
        if n < 2 {
            return Self { lambda: 0.0, r_squared: 0.0 };
        }
        let nf = n as f64;
        let mean_dp = price_changes[..n].iter().sum::<f64>() / nf;
        let mean_dq = signed_volumes[..n].iter().sum::<f64>() / nf;

        let mut cov_pq = 0.0_f64;
        let mut var_q = 0.0_f64;
        let mut var_p = 0.0_f64;
        for i in 0..n {
            let dp = price_changes[i] - mean_dp;
            let dq = signed_volumes[i] - mean_dq;
            cov_pq += dp * dq;
            var_q += dq * dq;
            var_p += dp * dp;
        }

        if var_q == 0.0 {
            return Self { lambda: 0.0, r_squared: 0.0 };
        }

        let lambda = cov_pq / var_q;
        let r_squared = if var_p == 0.0 {
            0.0
        } else {
            let cor = cov_pq / (var_q.sqrt() * var_p.sqrt());
            (cor * cor).min(1.0).max(0.0)
        };

        Self { lambda, r_squared }
    }
}

// ─────────────────────────────────────────
//  HasbrouckShare — variance decomposition information share
// ─────────────────────────────────────────

/// Simplified Hasbrouck (1995) information share via variance decomposition.
///
/// Decomposes the contribution of each security to the common efficient price
/// using the fraction of total return variance attributable to each series.
///
/// `security_a_share = Var(a) / (Var(a) + Var(b))`, and similarly for B.
/// Both shares sum to 1.0. When total variance is zero, each share is 0.5.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::HasbrouckShare;
///
/// let a = vec![0.1, -0.1, 0.2];
/// let b = vec![0.05, -0.05, 0.1];
/// let hs = HasbrouckShare::estimate(&a, &b);
/// assert!((hs.security_a_share + hs.security_b_share - 1.0).abs() < 1e-10);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct HasbrouckShare {
    /// Information share attributed to security A (0 to 1).
    pub security_a_share: f64,
    /// Information share attributed to security B (0 to 1).
    pub security_b_share: f64,
}

impl HasbrouckShare {
    /// Estimate Hasbrouck information shares from return series.
    ///
    /// `returns_a` and `returns_b` are period returns for the two securities.
    /// Uses the length of the shorter slice.
    pub fn estimate(returns_a: &[f64], returns_b: &[f64]) -> Self {
        let n = returns_a.len().min(returns_b.len());
        if n == 0 {
            return Self { security_a_share: 0.5, security_b_share: 0.5 };
        }
        let nf = n as f64;
        let mean_a = returns_a[..n].iter().sum::<f64>() / nf;
        let mean_b = returns_b[..n].iter().sum::<f64>() / nf;
        let var_a: f64 = returns_a[..n].iter().map(|x| (x - mean_a).powi(2)).sum::<f64>() / nf;
        let var_b: f64 = returns_b[..n].iter().map(|x| (x - mean_b).powi(2)).sum::<f64>() / nf;
        let total = var_a + var_b;
        if total == 0.0 {
            return Self { security_a_share: 0.5, security_b_share: 0.5 };
        }
        Self {
            security_a_share: var_a / total,
            security_b_share: var_b / total,
        }
    }
}

// ─────────────────────────────────────────
//  PinEstimate — Probability of Informed Trading
// ─────────────────────────────────────────

/// Easley et al. (1996) PIN model parameters estimated via method-of-moments.
///
/// Parameters:
/// - `alpha`: probability of an information event (0 to 1).
/// - `mu`: arrival rate of informed traders given an event.
/// - `epsilon_b`: uninformed buy-order arrival rate.
/// - `epsilon_s`: uninformed sell-order arrival rate.
///
/// # Example
/// ```rust
/// use fin_primitives::microstructure::PinEstimate;
///
/// let buys = vec![80u64, 90, 85];
/// let sells = vec![20u64, 10, 15];
/// let pin = PinEstimate::from_trade_counts(&buys, &sells);
/// let p = pin.pin_probability();
/// assert!((0.0..=1.0).contains(&p));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PinEstimate {
    /// Probability of an information event occurring.
    pub alpha: f64,
    /// Informed trader arrival rate conditional on an event.
    pub mu: f64,
    /// Uninformed buy-order arrival rate.
    pub epsilon_b: f64,
    /// Uninformed sell-order arrival rate.
    pub epsilon_s: f64,
}

impl PinEstimate {
    /// Estimate PIN parameters from daily buy/sell trade counts via method-of-moments.
    ///
    /// Method-of-moments approximation:
    /// - `epsilon_b ≈ mean(min(buys, sells))` (uninformed baseline)
    /// - `epsilon_s ≈ mean(min(buys, sells))`
    /// - `mu ≈ mean(|buys - sells|)` (excess flow attributable to informed)
    /// - `alpha ≈ fraction of days with buys > sells` (event probability)
    ///
    /// Returns all-zero parameters when both slices are empty.
    pub fn from_trade_counts(buy_days: &[u64], sell_days: &[u64]) -> Self {
        let n = buy_days.len().min(sell_days.len());
        if n == 0 {
            return Self { alpha: 0.0, mu: 0.0, epsilon_b: 0.0, epsilon_s: 0.0 };
        }
        let nf = n as f64;
        let mut sum_min = 0.0_f64;
        let mut sum_excess = 0.0_f64;
        let mut event_days = 0u64;

        for i in 0..n {
            let b = buy_days[i] as f64;
            let s = sell_days[i] as f64;
            let mn = b.min(s);
            let excess = (b - s).abs();
            sum_min += mn;
            sum_excess += excess;
            if (b - s).abs() > 1e-9 {
                event_days += 1;
            }
        }

        let epsilon_b = sum_min / nf;
        let epsilon_s = epsilon_b;
        let mu = sum_excess / nf;
        let alpha = event_days as f64 / nf;

        Self { alpha, mu, epsilon_b, epsilon_s }
    }

    /// Compute the PIN probability.
    ///
    /// `PIN = alpha * mu / (alpha * mu + epsilon_b + epsilon_s)`
    ///
    /// Returns `0.0` when the denominator is zero.
    pub fn pin_probability(&self) -> f64 {
        let numerator = self.alpha * self.mu;
        let denominator = numerator + self.epsilon_b + self.epsilon_s;
        if denominator == 0.0 {
            0.0
        } else {
            numerator / denominator
        }
    }
}
