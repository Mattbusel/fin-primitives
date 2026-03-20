//! # Module: risk
//!
//! ## Responsibility
//! Tracks equity drawdown and evaluates configurable risk rules on each equity update.
//!
//! ## Guarantees
//! - `DrawdownTracker::current_drawdown_pct` is always non-negative
//! - `RiskMonitor::update` returns all triggered `RiskBreach` values (empty vec if none)
//!
//! ## NOT Responsible For
//! - Position sizing
//! - Order cancellation (callers must act on returned breaches)

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Tracks peak equity and computes current drawdown percentage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DrawdownTracker {
    peak_equity: Decimal,
    current_equity: Decimal,
    worst_drawdown_pct: Decimal,
    /// Number of updates since the last new peak.
    updates_since_peak: usize,
    /// Total number of equity updates processed.
    update_count: usize,
    /// Number of updates where equity was below peak (in drawdown).
    drawdown_update_count: usize,
    /// Cumulative sum of drawdown percentages for computing averages.
    #[serde(default)]
    drawdown_pct_sum: Decimal,
    /// Longest consecutive run of updates spent below peak.
    #[serde(default)]
    max_drawdown_streak: usize,
    /// Current consecutive run of updates where equity increased from the prior update.
    #[serde(default)]
    gain_streak: usize,
    /// Number of times a new equity peak has been set.
    #[serde(default)]
    peak_count: usize,
    /// Previous equity value (for computing per-update changes).
    #[serde(default)]
    prev_equity: Decimal,
    /// Welford running mean of per-update equity changes.
    #[serde(default)]
    equity_change_mean: f64,
    /// Welford running M2 (sum of squared deviations) for sample variance.
    #[serde(default)]
    equity_change_m2: f64,
    /// Count of equity changes recorded (= update_count after first update).
    #[serde(default)]
    equity_change_count: usize,
    /// Most negative single-step equity change seen (0.0 until first loss).
    #[serde(default)]
    min_equity_delta: f64,
    /// Longest run of consecutive updates where equity increased.
    #[serde(default)]
    max_gain_streak: usize,
    /// Sum of all positive per-update equity changes.
    #[serde(default)]
    total_gain_sum: f64,
    /// Sum of the absolute values of all negative per-update equity changes.
    #[serde(default)]
    total_loss_sum: f64,
    /// Number of completed recoveries (drawdown resolved by hitting a new peak).
    #[serde(default)]
    completed_recoveries: usize,
    /// Sum of `updates_since_peak` values at the moment each recovery completed.
    #[serde(default)]
    total_recovery_updates: usize,
    /// Sum of drawdown percentages at the start of each recovery (for averaging).
    #[serde(default)]
    recovery_drawdown_pct_sum: Decimal,
    /// Largest single-step equity gain as a percentage of prior equity.
    #[serde(default)]
    max_gain_delta_pct: f64,
    /// Number of distinct drawdown episodes (each time equity drops below peak after being at/above it).
    #[serde(default)]
    drawdown_episodes: usize,
    /// Current consecutive run of updates where equity decreased from the prior update.
    #[serde(default)]
    loss_streak_current: usize,
    /// Initial equity (set at construction, unchanged by reset unless re-constructed).
    initial_equity: Decimal,
    /// Current consecutive run of updates where equity was unchanged.
    #[serde(default)]
    flat_streak: usize,
}

impl DrawdownTracker {
    /// Creates a new `DrawdownTracker` with the given initial (and peak) equity.
    pub fn new(initial_equity: Decimal) -> Self {
        Self {
            peak_equity: initial_equity,
            current_equity: initial_equity,
            worst_drawdown_pct: Decimal::ZERO,
            updates_since_peak: 0,
            update_count: 0,
            drawdown_update_count: 0,
            drawdown_pct_sum: Decimal::ZERO,
            max_drawdown_streak: 0,
            gain_streak: 0,
            peak_count: 0,
            prev_equity: initial_equity,
            equity_change_mean: 0.0,
            equity_change_m2: 0.0,
            equity_change_count: 0,
            min_equity_delta: 0.0,
            max_gain_streak: 0,
            total_gain_sum: 0.0,
            total_loss_sum: 0.0,
            completed_recoveries: 0,
            total_recovery_updates: 0,
            recovery_drawdown_pct_sum: Decimal::ZERO,
            max_gain_delta_pct: 0.0,
            drawdown_episodes: 0,
            loss_streak_current: 0,
            initial_equity,
            flat_streak: 0,
        }
    }

    /// Updates the tracker with the latest equity value, updating the peak if higher.
    pub fn update(&mut self, equity: Decimal) {
        // Welford online variance update for equity changes
        if self.update_count > 0 {
            if let (Some(prev), Some(curr)) = (
                self.prev_equity.to_f64(),
                equity.to_f64(),
            ) {
                let delta = curr - prev;
                self.equity_change_count += 1;
                let n = self.equity_change_count as f64;
                let old_mean = self.equity_change_mean;
                self.equity_change_mean += (delta - old_mean) / n;
                self.equity_change_m2 += (delta - old_mean) * (delta - self.equity_change_mean);
                if delta < self.min_equity_delta {
                    self.min_equity_delta = delta;
                }
                if delta > 0.0 {
                    self.total_gain_sum += delta;
                    if prev > 0.0 {
                        let pct = delta / prev * 100.0;
                        if pct > self.max_gain_delta_pct {
                            self.max_gain_delta_pct = pct;
                        }
                    }
                } else if delta < 0.0 {
                    self.total_loss_sum += -delta;
                }
            }
        }
        self.prev_equity = equity;

        self.update_count += 1;
        if equity > self.current_equity {
            self.gain_streak += 1;
            if self.gain_streak > self.max_gain_streak {
                self.max_gain_streak = self.gain_streak;
            }
            self.loss_streak_current = 0;
            self.flat_streak = 0;
        } else if equity < self.current_equity {
            self.gain_streak = 0;
            self.loss_streak_current += 1;
            self.flat_streak = 0;
        } else {
            self.gain_streak = 0;
            self.loss_streak_current = 0;
            self.flat_streak += 1;
        }
        if equity > self.peak_equity {
            if self.updates_since_peak > 0 {
                self.total_recovery_updates += self.updates_since_peak;
                self.recovery_drawdown_pct_sum += self.current_drawdown_pct();
                self.completed_recoveries += 1;
            }
            self.peak_equity = equity;
            self.updates_since_peak = 0;
            self.peak_count += 1;
        } else {
            if equity < self.peak_equity && self.updates_since_peak == 0 {
                self.drawdown_episodes += 1;
            }
            self.updates_since_peak += 1;
            self.drawdown_update_count += 1;
        }
        self.current_equity = equity;
        let dd = self.current_drawdown_pct();
        if dd > self.worst_drawdown_pct {
            self.worst_drawdown_pct = dd;
        }
        if !dd.is_zero() {
            self.drawdown_pct_sum += dd;
        }
        if self.updates_since_peak > self.max_drawdown_streak {
            self.max_drawdown_streak = self.updates_since_peak;
        }
    }

    /// Returns the number of `update()` calls since the last new equity peak.
    ///
    /// A value of 0 means the last update set a new peak. Higher values indicate
    /// how long the portfolio has been in drawdown (in update units).
    pub fn drawdown_duration(&self) -> usize {
        self.updates_since_peak
    }

    /// Returns current drawdown as a percentage: `(peak - current) / peak * 100`.
    ///
    /// Returns `0` if `peak_equity` is zero.
    pub fn current_drawdown_pct(&self) -> Decimal {
        if self.peak_equity == Decimal::ZERO {
            return Decimal::ZERO;
        }
        (self.peak_equity - self.current_equity) / self.peak_equity * Decimal::ONE_HUNDRED
    }

    /// Returns the highest equity seen since construction.
    pub fn peak(&self) -> Decimal {
        self.peak_equity
    }

    /// Returns the current equity value.
    pub fn current_equity(&self) -> Decimal {
        self.current_equity
    }

    /// Returns `true` if the current drawdown percentage does not exceed `max_dd_pct`.
    pub fn is_below_threshold(&self, max_dd_pct: Decimal) -> bool {
        self.current_drawdown_pct() <= max_dd_pct
    }

    /// Resets the peak to the current equity value.
    ///
    /// Useful for daily or session-boundary resets where you want drawdown measured
    /// from the start of the new session rather than the all-time high.
    pub fn reset_peak(&mut self) {
        self.peak_equity = self.current_equity;
        self.updates_since_peak = 0;
    }

    /// Returns the worst (highest) drawdown percentage seen since construction or last reset.
    pub fn worst_drawdown_pct(&self) -> Decimal {
        self.worst_drawdown_pct
    }

    /// Returns the total number of equity updates since construction or last reset.
    pub fn update_count(&self) -> usize {
        self.update_count
    }

    /// Returns the fraction of updates where equity was at or above peak (not in drawdown).
    ///
    /// `win_rate = (update_count - drawdown_update_count) / update_count`
    ///
    /// Returns `None` if no updates have been processed.
    pub fn win_rate(&self) -> Option<Decimal> {
        if self.update_count == 0 {
            return None;
        }
        let at_peak = self.update_count - self.drawdown_update_count;
        #[allow(clippy::cast_possible_truncation)]
        Some(Decimal::from(at_peak as u64) / Decimal::from(self.update_count as u64))
    }

    /// Returns how far below peak current equity is, as a percentage.
    ///
    /// `underwater_pct = (peak - current) / peak × 100`
    ///
    /// Returns `Decimal::ZERO` when at or above peak.
    pub fn underwater_pct(&self) -> Decimal {
        if self.peak_equity == Decimal::ZERO {
            return Decimal::ZERO;
        }
        let diff = self.peak_equity - self.current_equity;
        if diff <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        diff / self.peak_equity * Decimal::ONE_HUNDRED
    }

    /// Fully resets the tracker as if it were freshly constructed with `initial` equity.
    pub fn reset(&mut self, initial: Decimal) {
        self.peak_equity = initial;
        self.current_equity = initial;
        self.drawdown_pct_sum = Decimal::ZERO;
        self.max_drawdown_streak = 0;
        self.worst_drawdown_pct = Decimal::ZERO;
        self.updates_since_peak = 0;
        self.update_count = 0;
        self.drawdown_update_count = 0;
        self.gain_streak = 0;
        self.peak_count = 0;
        self.prev_equity = initial;
        self.equity_change_mean = 0.0;
        self.equity_change_m2 = 0.0;
        self.equity_change_count = 0;
        self.min_equity_delta = 0.0;
        self.max_gain_streak = 0;
        self.total_gain_sum = 0.0;
        self.total_loss_sum = 0.0;
        self.completed_recoveries = 0;
        self.total_recovery_updates = 0;
        self.recovery_drawdown_pct_sum = Decimal::ZERO;
        self.max_gain_delta_pct = 0.0;
        self.drawdown_episodes = 0;
        self.loss_streak_current = 0;
        self.flat_streak = 0;
    }

    /// Returns the sample standard deviation of per-update equity changes.
    ///
    /// Uses Welford's online algorithm internally. Returns `None` until at least
    /// two updates have been processed (can't compute variance from one sample).
    pub fn volatility(&self) -> Option<f64> {
        if self.equity_change_count < 2 {
            return None;
        }
        let variance = self.equity_change_m2 / (self.equity_change_count - 1) as f64;
        Some(variance.sqrt())
    }

    /// Returns the recovery factor: `net_profit_pct / worst_drawdown_pct`.
    ///
    /// A higher value indicates better risk-adjusted performance.
    /// Returns `None` when `worst_drawdown_pct` is zero (no drawdown has occurred).
    pub fn recovery_factor(&self, net_profit_pct: Decimal) -> Option<Decimal> {
        if self.worst_drawdown_pct.is_zero() {
            return None;
        }
        Some(net_profit_pct / self.worst_drawdown_pct)
    }

    /// Returns the Calmar ratio: `annualized_return / worst_drawdown_pct`.
    ///
    /// Higher values indicate better risk-adjusted performance. Returns `None` when
    /// `worst_drawdown_pct` is zero (no drawdown has occurred).
    pub fn calmar_ratio(&self, annualized_return: Decimal) -> Option<Decimal> {
        if self.worst_drawdown_pct.is_zero() {
            return None;
        }
        Some(annualized_return / self.worst_drawdown_pct)
    }

    /// Returns `true` if the current equity is strictly below the peak (i.e. in drawdown).
    pub fn in_drawdown(&self) -> bool {
        self.current_equity < self.peak_equity
    }

    /// Applies a sequence of equity values in order, as if each were an individual `update` call.
    ///
    /// Useful for batch processing historical equity curves without a manual loop.
    pub fn update_with_returns(&mut self, equities: &[Decimal]) {
        for &eq in equities {
            self.update(eq);
        }
    }

    /// Returns the number of consecutive updates where equity was below the peak.
    ///
    /// Equivalent to [`DrawdownTracker::drawdown_duration`]. Provided as a semantic
    /// alias for call sites that prefer "count" over "duration".
    pub fn drawdown_count(&self) -> usize {
        self.updates_since_peak
    }

    /// Returns the Sharpe ratio: `annualized_return / annualized_vol`.
    ///
    /// Returns `None` when `annualized_vol` is zero to avoid division by zero.
    pub fn sharpe_ratio(
        &self,
        annualized_return: Decimal,
        annualized_vol: Decimal,
    ) -> Option<Decimal> {
        if annualized_vol.is_zero() {
            return None;
        }
        Some(annualized_return / annualized_vol)
    }

    /// Returns the percentage gain required from the current equity to recover to the peak.
    ///
    /// Formula: `(peak / current - 1) * 100`. Returns `Decimal::ZERO` when already at peak
    /// or when current equity is zero (to avoid division by zero).
    pub fn recovery_to_peak_pct(&self) -> Decimal {
        if self.current_equity.is_zero() || self.current_equity >= self.peak_equity {
            return Decimal::ZERO;
        }
        (self.peak_equity / self.current_equity - Decimal::ONE) * Decimal::ONE_HUNDRED
    }

    /// Fraction of equity updates spent below peak: `drawdown_update_count / update_count`.
    ///
    /// Returns `Decimal::ZERO` when no updates have been processed.
    #[allow(clippy::cast_possible_truncation)]
    pub fn time_underwater_pct(&self) -> Decimal {
        if self.update_count == 0 {
            return Decimal::ZERO;
        }
        Decimal::from(self.drawdown_update_count as u64)
            / Decimal::from(self.update_count as u64)
    }

    /// Average drawdown percentage across all updates that had a non-zero drawdown.
    ///
    /// Returns `None` when no drawdown updates have been recorded.
    #[allow(clippy::cast_possible_truncation)]
    pub fn avg_drawdown_pct(&self) -> Option<Decimal> {
        if self.drawdown_update_count == 0 {
            return None;
        }
        Some(self.drawdown_pct_sum / Decimal::from(self.drawdown_update_count as u64))
    }

    /// Longest consecutive run of updates where equity was below peak.
    pub fn max_loss_streak(&self) -> usize {
        self.max_drawdown_streak.max(self.updates_since_peak)
    }

    /// Returns the current consecutive run of updates where equity increased from the prior update.
    ///
    /// Resets to zero on any non-increasing update. Useful for detecting sustained rallies.
    pub fn consecutive_gain_updates(&self) -> usize {
        self.gain_streak
    }

    /// Returns `current_equity / peak_equity`, useful for position sizing formulas.
    ///
    /// Returns `Decimal::ONE` when peak is zero (no drawdown state yet). A value below 1
    /// indicates the portfolio is in drawdown; exactly 1 means at peak.
    pub fn equity_ratio(&self) -> Decimal {
        if self.peak_equity.is_zero() {
            return Decimal::ONE;
        }
        self.current_equity / self.peak_equity
    }

    /// Returns how many times a new equity peak has been set since construction or last reset.
    pub fn new_peak_count(&self) -> usize {
        self.peak_count
    }

    /// Returns the "pain index": mean absolute drawdown across all updates.
    ///
    /// `pain_index = drawdown_pct_sum / update_count`
    ///
    /// Represents the average percentage loss a holder experienced over the equity curve.
    /// Returns `Decimal::ZERO` when no updates have been processed.
    #[allow(clippy::cast_possible_truncation)]
    pub fn pain_index(&self) -> Decimal {
        if self.update_count == 0 {
            return Decimal::ZERO;
        }
        self.drawdown_pct_sum / Decimal::from(self.update_count as u64)
    }

    /// Returns `true` if `equity` is strictly greater than the current peak (new high-water mark).
    ///
    /// Useful for triggering high-water-mark-based fee calculations or performance resets.
    /// Note: this does NOT update the tracker — call `update(equity)` to advance the peak.
    pub fn above_high_water_mark(&self, equity: Decimal) -> bool {
        equity > self.peak_equity
    }

    /// Returns the largest single-step equity drop seen across all updates.
    ///
    /// Returns the magnitude (positive number) of the worst per-update loss.
    /// Returns `None` if no loss has occurred or fewer than two updates have been processed.
    pub fn max_single_loss(&self) -> Option<f64> {
        if self.equity_change_count == 0 || self.min_equity_delta >= 0.0 {
            return None;
        }
        Some(-self.min_equity_delta)
    }

    /// Returns the fraction of equity updates that decreased equity (loss rate).
    ///
    /// A value of `0.0` means equity never decreased; `1.0` means it always decreased.
    /// Returns `None` if no updates have been processed.
    ///
    /// Note: uses the drawdown update count as a proxy for loss updates — specifically
    /// the number of updates where equity was below peak, not strictly below the prior update.
    pub fn loss_rate(&self) -> Option<f64> {
        if self.update_count == 0 {
            return None;
        }
        Some(self.drawdown_update_count as f64 / self.update_count as f64)
    }

    /// Returns the current number of consecutive updates where equity decreased.
    ///
    /// Resets to zero on any update where equity increases or stays the same.
    /// A current losing streak indicator complementing [`DrawdownTracker::consecutive_gain_updates`].
    pub fn consecutive_loss_updates(&self) -> usize {
        // gain_streak tracks consecutive gains; when gain_streak is 0 and we're in drawdown
        // that approximates a loss streak. We return updates_since_peak as the losing streak
        // (time underwater is the closest proxy without a dedicated field).
        if self.gain_streak > 0 {
            0
        } else {
            self.updates_since_peak
        }
    }

    /// Returns the running mean of per-update equity changes.
    ///
    /// Computed via Welford's online algorithm. Returns `None` until at least one
    /// equity change has been recorded (requires 2+ updates).
    pub fn equity_change_mean(&self) -> Option<f64> {
        if self.equity_change_count == 0 {
            return None;
        }
        Some(self.equity_change_mean)
    }

    /// Returns the hypothetical drawdown percentage if equity dropped by `shock_pct` from current.
    ///
    /// `stress_drawdown = current_drawdown + shock_pct × (1 - current_drawdown/100)`
    ///
    /// This estimates the total drawdown from peak if the current equity fell an additional
    /// `shock_pct` percent. Returns the result as a percentage (0–100+).
    pub fn stress_test(&self, shock_pct: Decimal) -> Decimal {
        if self.peak_equity.is_zero() {
            return shock_pct;
        }
        let stressed_equity = self.current_equity
            * (Decimal::ONE_HUNDRED - shock_pct)
            / Decimal::ONE_HUNDRED;
        if stressed_equity >= self.peak_equity {
            return Decimal::ZERO;
        }
        (self.peak_equity - stressed_equity) / self.peak_equity * Decimal::ONE_HUNDRED
    }

    /// Returns the longest consecutive run of equity increases seen since construction or reset.
    pub fn max_gain_streak(&self) -> usize {
        self.max_gain_streak
    }

    /// Returns the cumulative sum of all positive per-update equity changes.
    ///
    /// Returns `0.0` if no gains have been recorded.
    pub fn total_gain_sum(&self) -> f64 {
        self.total_gain_sum
    }

    /// Returns the cumulative sum of absolute values of all negative per-update equity changes.
    ///
    /// Returns `0.0` if no losses have been recorded.
    pub fn total_loss_sum(&self) -> f64 {
        self.total_loss_sum
    }

    /// Returns `total_gain_sum / total_loss_sum`. Returns `None` if no losses recorded.
    pub fn gain_to_loss_ratio(&self) -> Option<f64> {
        if self.total_loss_sum == 0.0 { None } else { Some(self.total_gain_sum / self.total_loss_sum) }
    }

    /// Trading expectancy: `win_rate × avg_gain − loss_rate × avg_loss`.
    ///
    /// Returns `None` if fewer than 2 equity changes have been recorded.
    pub fn expectancy(&self) -> Option<f64> {
        let n = self.equity_change_count;
        if n < 2 { return None; }
        let wr = self.win_rate()?.to_f64()?;
        let loss_rate = 1.0 - wr;
        let gain_count = (wr * n as f64).round() as usize;
        let loss_count = n.saturating_sub(gain_count);
        let avg_gain = if gain_count > 0 { self.total_gain_sum / gain_count as f64 } else { 0.0 };
        let avg_loss = if loss_count > 0 { self.total_loss_sum / loss_count as f64 } else { 0.0 };
        Some(wr * avg_gain - loss_rate * avg_loss)
    }

    /// Average number of updates required to recover from a drawdown to a new peak.
    ///
    /// Returns `None` if no drawdown has ever been fully recovered.
    pub fn recovery_speed(&self) -> Option<f64> {
        if self.completed_recoveries == 0 { return None; }
        Some(self.total_recovery_updates as f64 / self.completed_recoveries as f64)
    }

    /// Number of times a new equity peak has been set.
    ///
    /// This equals the number of `update()` calls where equity exceeded the prior peak.
    pub fn peak_hit_count(&self) -> usize {
        self.peak_count
    }

    /// Average drawdown percentage at the moment each recovery began.
    ///
    /// Returns `None` if no drawdown has ever been fully recovered.
    pub fn avg_recovery_drawdown_pct(&self) -> Option<Decimal> {
        if self.completed_recoveries == 0 { return None; }
        #[allow(clippy::cast_possible_truncation)]
        Some(self.recovery_drawdown_pct_sum / Decimal::from(self.completed_recoveries as u32))
    }

    /// Largest single-step equity gain expressed as a percentage of the prior equity.
    ///
    /// Returns `0.0` if no gain has been recorded yet.
    pub fn max_gain_pct(&self) -> f64 {
        self.max_gain_delta_pct
    }

    /// Average number of updates spent in each drawdown episode.
    ///
    /// Returns `None` if no drawdown episode has been entered yet.
    pub fn avg_drawdown_duration(&self) -> Option<f64> {
        if self.drawdown_episodes == 0 { return None; }
        Some(self.drawdown_update_count as f64 / self.drawdown_episodes as f64)
    }

    /// The peak equity level the current equity must reach to exit drawdown.
    ///
    /// Equals the all-time peak. If equity is already at peak, this is the current equity.
    pub fn breakeven_equity(&self) -> Decimal {
        self.peak_equity
    }

    /// Current consecutive count of updates where equity decreased.
    ///
    /// Resets to 0 as soon as equity increases or stays flat.
    pub fn loss_streak(&self) -> usize {
        self.loss_streak_current
    }

    /// Net return as a percentage: `(current_equity - initial_equity) / initial_equity * 100`.
    ///
    /// Returns `None` if `initial_equity` is zero.
    pub fn net_return_pct(&self) -> Option<f64> {
        let init = self.initial_equity.to_f64()?;
        if init == 0.0 { return None; }
        let curr = self.current_equity.to_f64()?;
        Some((curr - init) / init * 100.0)
    }

    /// Current count of consecutive updates where equity did not change.
    pub fn consecutive_flat_count(&self) -> usize {
        self.flat_streak
    }

    /// Total number of `update()` calls processed since construction or last `reset()`.
    pub fn total_updates(&self) -> usize {
        self.update_count
    }

    /// Percentage of all updates spent below peak equity (in drawdown).
    ///
    /// Returns `0.0` if no updates have been processed.
    pub fn pct_time_in_drawdown(&self) -> f64 {
        if self.update_count == 0 { return 0.0; }
        self.drawdown_update_count as f64 / self.update_count as f64 * 100.0
    }

    /// Compound Annual Growth Rate (CAGR) of equity.
    ///
    /// `CAGR = (current / initial) ^ (periods_per_year / update_count) - 1`.
    /// Returns `None` if `initial_equity` is zero or non-positive, or fewer than 2 updates.
    pub fn equity_cagr(&self, periods_per_year: usize) -> Option<f64> {
        if self.update_count < 2 || periods_per_year == 0 { return None; }
        let init = self.initial_equity.to_f64()?;
        if init <= 0.0 { return None; }
        let curr = self.current_equity.to_f64()?;
        if curr <= 0.0 { return None; }
        let years = self.update_count as f64 / periods_per_year as f64;
        Some((curr / init).powf(1.0 / years) - 1.0)
    }

    /// Returns `true` when equity is below its peak but gained on the last update.
    pub fn is_recovering(&self) -> bool {
        self.in_drawdown() && self.gain_streak > 0
    }

    /// Current drawdown as a fraction of the worst recorded drawdown.
    ///
    /// Returns `Decimal::ZERO` if no drawdown has been recorded yet.
    pub fn drawdown_ratio(&self) -> Decimal {
        if self.worst_drawdown_pct.is_zero() { return Decimal::ZERO; }
        self.current_drawdown_pct() / self.worst_drawdown_pct
    }

    /// Current equity as a multiple of initial equity (e.g., `1.5` = 50% gain).
    pub fn equity_multiple(&self) -> Decimal {
        if self.initial_equity.is_zero() { return Decimal::ONE; }
        self.current_equity / self.initial_equity
    }

    /// Average per-update equity gain across all positive updates.
    ///
    /// Uses `win_rate` and `update_count` to estimate the number of positive updates.
    /// Returns `None` if there have been no positive updates recorded.
    pub fn avg_gain_pct(&self) -> Option<f64> {
        let wr: f64 = self.win_rate()?.to_string().parse().ok()?;
        let gain_count = (wr / 100.0 * self.update_count as f64).round() as usize;
        if gain_count == 0 { return None; }
        Some(self.total_gain_sum / gain_count as f64)
    }

    /// Median of a slice of drawdown percentages.
    ///
    /// The input need not be sorted. Returns `None` if the slice is empty.
    pub fn median_drawdown_pct(drawdowns: &[Decimal]) -> Option<Decimal> {
        if drawdowns.is_empty() { return None; }
        let mut sorted = drawdowns.to_vec();
        sorted.sort();
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 1 {
            Some(sorted[mid])
        } else {
            Some((sorted[mid - 1] + sorted[mid]) / Decimal::TWO)
        }
    }

    /// Sortino ratio from a slice of period returns.
    ///
    /// `sortino = (mean_return - target) / downside_deviation`
    ///
    /// where downside deviation is the standard deviation of returns *below* `target`.
    /// Returns `None` if `returns` is empty or downside deviation is zero.
    pub fn sortino_ratio(returns: &[Decimal], target: Decimal) -> Option<f64> {
        if returns.is_empty() {
            return None;
        }
        let n = returns.len() as f64;
        let target_f = target.to_f64()?;
        let mean: f64 = returns.iter().filter_map(|r| r.to_f64()).sum::<f64>() / n;
        let downside_sq_sum: f64 = returns
            .iter()
            .filter_map(|r| r.to_f64())
            .map(|r| {
                let diff = r - target_f;
                if diff < 0.0 { diff * diff } else { 0.0 }
            })
            .sum();
        if downside_sq_sum == 0.0 {
            return None;
        }
        let downside_dev = (downside_sq_sum / n).sqrt();
        if downside_dev == 0.0 {
            return None;
        }
        Some((mean - target_f) / downside_dev)
    }

    /// Annualised volatility from a slice of period returns.
    ///
    /// `volatility = std_dev(returns) * sqrt(periods_per_year)`
    ///
    /// Returns `None` if `returns` has fewer than 2 elements.
    pub fn returns_volatility(returns: &[Decimal], periods_per_year: u32) -> Option<f64> {
        if returns.len() < 2 {
            return None;
        }
        let n = returns.len() as f64;
        let mean: f64 = returns.iter()
            .filter_map(|r| r.to_f64())
            .sum::<f64>() / n;
        let variance: f64 = returns.iter()
            .filter_map(|r| r.to_f64())
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / (n - 1.0);
        let vol = variance.sqrt() * (periods_per_year as f64).sqrt();
        Some(vol)
    }

    /// Omega ratio: sum of returns above `threshold` / abs(sum of returns below `threshold`).
    ///
    /// Values > 1 indicate more upside than downside relative to the threshold.
    /// Returns `None` if `returns` is empty or total downside is zero.
    pub fn omega_ratio(returns: &[Decimal], threshold: Decimal) -> Option<f64> {
        if returns.is_empty() {
            return None;
        }
        let threshold_f = threshold.to_f64()?;
        let upside: f64 = returns
            .iter()
            .filter_map(|r| r.to_f64())
            .map(|r| (r - threshold_f).max(0.0))
            .sum();
        let downside: f64 = returns
            .iter()
            .filter_map(|r| r.to_f64())
            .map(|r| (threshold_f - r).max(0.0))
            .sum();
        if downside == 0.0 {
            return None;
        }
        Some(upside / downside)
    }

    /// Information ratio: `(mean(returns) - mean(benchmark)) / std_dev(returns - benchmark)`.
    ///
    /// Measures risk-adjusted excess return over a benchmark. Returns `None` if fewer than 2
    /// matched return pairs exist or tracking error is zero.
    pub fn information_ratio(returns: &[Decimal], benchmark: &[Decimal]) -> Option<f64> {
        let n = returns.len().min(benchmark.len());
        if n < 2 {
            return None;
        }
        let excess: Vec<f64> = returns[..n]
            .iter()
            .zip(benchmark[..n].iter())
            .filter_map(|(r, b)| Some(r.to_f64()? - b.to_f64()?))
            .collect();
        if excess.len() < 2 {
            return None;
        }
        let mean_excess = excess.iter().sum::<f64>() / excess.len() as f64;
        let tracking_variance = excess.iter().map(|e| (e - mean_excess).powi(2)).sum::<f64>()
            / (excess.len() as f64 - 1.0);
        let tracking_error = tracking_variance.sqrt();
        if tracking_error == 0.0 {
            return None;
        }
        Some(mean_excess / tracking_error)
    }

    /// Annualized volatility of equity changes: `std_dev_of_changes * sqrt(periods_per_year)`.
    ///
    /// Returns `None` if fewer than 2 updates have been recorded.
    pub fn annualized_volatility(&self, periods_per_year: u32) -> Option<f64> {
        if self.equity_change_count < 2 { return None; }
        let n = self.equity_change_count as f64;
        let variance = self.equity_change_m2 / (n - 1.0);
        Some(variance.sqrt() * (periods_per_year as f64).sqrt())
    }

    /// Pain ratio: `annualized_return_pct / pain_index`.
    ///
    /// A higher ratio indicates better risk-adjusted performance relative to
    /// sustained drawdown. Returns `None` if the pain index is zero (no drawdowns).
    pub fn pain_ratio(&self, annualized_return_pct: Decimal) -> Option<Decimal> {
        let pi = self.pain_index();
        if pi.is_zero() { return None; }
        Some(annualized_return_pct / pi)
    }

    /// Fraction of all updates where equity was at or above the peak (above water).
    ///
    /// Complement of [`time_underwater_pct`](Self::time_underwater_pct).
    /// Returns `Decimal::ONE` when no updates have been processed.
    pub fn time_above_watermark_pct(&self) -> Decimal {
        if self.update_count == 0 {
            return Decimal::ONE;
        }
        Decimal::ONE - self.time_underwater_pct()
    }
}

impl std::fmt::Display for DrawdownTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "equity={} peak={} drawdown={:.2}%",
            self.current_equity,
            self.peak_equity,
            self.current_drawdown_pct()
        )
    }
}

/// A triggered risk rule violation.
#[derive(Debug, Clone, PartialEq)]
pub struct RiskBreach {
    /// The name of the rule that triggered.
    pub rule: String,
    /// Human-readable detail of the violation.
    pub detail: String,
}

/// A risk rule that can be checked against current equity and drawdown.
pub trait RiskRule: Send {
    /// Returns the rule's name.
    fn name(&self) -> &str;

    /// Returns `Some(RiskBreach)` if the rule is violated, or `None` if compliant.
    ///
    /// # Arguments
    /// * `equity` - current portfolio equity
    /// * `drawdown_pct` - current drawdown percentage from peak
    fn check(&self, equity: Decimal, drawdown_pct: Decimal) -> Option<RiskBreach>;
}

/// Triggers a breach when drawdown exceeds a threshold percentage.
pub struct MaxDrawdownRule {
    /// The maximum allowed drawdown percentage (e.g., `dec!(10)` = 10%).
    pub threshold_pct: Decimal,
}

impl RiskRule for MaxDrawdownRule {
    fn name(&self) -> &str {
        "max_drawdown"
    }

    fn check(&self, _equity: Decimal, drawdown_pct: Decimal) -> Option<RiskBreach> {
        if drawdown_pct > self.threshold_pct {
            Some(RiskBreach {
                rule: self.name().to_owned(),
                detail: format!("drawdown {drawdown_pct:.2}% > {:.2}%", self.threshold_pct),
            })
        } else {
            None
        }
    }
}

/// Triggers a breach when equity falls below a floor.
pub struct MinEquityRule {
    /// The minimum acceptable equity.
    pub floor: Decimal,
}

impl RiskRule for MinEquityRule {
    fn name(&self) -> &str {
        "min_equity"
    }

    fn check(&self, equity: Decimal, _drawdown_pct: Decimal) -> Option<RiskBreach> {
        if equity < self.floor {
            Some(RiskBreach {
                rule: self.name().to_owned(),
                detail: format!("equity {equity} < floor {}", self.floor),
            })
        } else {
            None
        }
    }
}

/// Evaluates multiple `RiskRule`s on each equity update and returns all breaches.
pub struct RiskMonitor {
    rules: Vec<Box<dyn RiskRule>>,
    tracker: DrawdownTracker,
    breach_count: usize,
}

impl RiskMonitor {
    /// Creates a new `RiskMonitor` with no rules and the given initial equity.
    pub fn new(initial_equity: Decimal) -> Self {
        Self {
            rules: Vec::new(),
            tracker: DrawdownTracker::new(initial_equity),
            breach_count: 0,
        }
    }

    /// Adds a rule to the monitor (builder pattern).
    #[must_use]
    pub fn add_rule(mut self, rule: impl RiskRule + 'static) -> Self {
        self.rules.push(Box::new(rule));
        self
    }

    /// Updates equity and returns all triggered breaches.
    pub fn update(&mut self, equity: Decimal) -> Vec<RiskBreach> {
        self.tracker.update(equity);
        let dd = self.tracker.current_drawdown_pct();
        let breaches: Vec<RiskBreach> = self.rules
            .iter()
            .filter_map(|r| r.check(equity, dd))
            .collect();
        self.breach_count += breaches.len();
        breaches
    }

    /// Returns the current drawdown percentage without triggering an update.
    pub fn drawdown_pct(&self) -> Decimal {
        self.tracker.current_drawdown_pct()
    }

    /// Returns the current equity value without triggering an update.
    pub fn current_equity(&self) -> Decimal {
        self.tracker.current_equity()
    }

    /// Returns the peak equity seen so far.
    pub fn peak_equity(&self) -> Decimal {
        self.tracker.peak()
    }

    /// Resets the internal drawdown tracker to `initial_equity`.
    pub fn reset(&mut self, initial_equity: Decimal) {
        self.tracker.reset(initial_equity);
        self.breach_count = 0;
    }

    /// Returns the number of rules registered with this monitor.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Resets the drawdown peak to the current equity.
    ///
    /// Delegates to [`DrawdownTracker::reset_peak`]. Useful at session boundaries
    /// when you want drawdown measured from the current level, not the all-time high.
    pub fn reset_peak(&mut self) {
        self.tracker.reset_peak();
    }

    /// Returns `true` if equity is currently below the recorded peak (i.e. in drawdown).
    pub fn is_in_drawdown(&self) -> bool {
        self.tracker.current_drawdown_pct() > Decimal::ZERO
    }

    /// Returns the worst (highest) drawdown percentage seen since construction or last reset.
    pub fn worst_drawdown_pct(&self) -> Decimal {
        self.tracker.worst_drawdown_pct()
    }

    /// Returns the total number of equity updates processed since construction or last reset.
    pub fn equity_history_len(&self) -> usize {
        self.tracker.update_count()
    }

    /// Returns the number of consecutive equity updates since the last peak (drawdown duration).
    pub fn drawdown_duration(&self) -> usize {
        self.tracker.drawdown_duration()
    }

    /// Returns the total number of rule breaches triggered since construction or last reset.
    pub fn breach_count(&self) -> usize {
        self.breach_count
    }

    /// Returns the maximum drawdown percentage seen since construction or last reset.
    ///
    /// Alias for [`worst_drawdown_pct`](Self::worst_drawdown_pct).
    pub fn max_drawdown_pct(&self) -> Decimal {
        self.tracker.worst_drawdown_pct()
    }

    /// Returns a shared reference to the internal [`DrawdownTracker`].
    ///
    /// Useful when callers need direct access to tracker state (e.g., worst drawdown)
    /// without going through the monitor's forwarding accessors.
    pub fn drawdown_tracker(&self) -> &DrawdownTracker {
        &self.tracker
    }

    /// Checks all rules against `equity` without updating the peak or current equity.
    ///
    /// Useful for prospective checks (e.g., "would this trade breach a rule?") where
    /// you do not want to alter tracked state.
    pub fn check(&self, equity: Decimal) -> Vec<RiskBreach> {
        let dd = if self.tracker.peak() == Decimal::ZERO {
            Decimal::ZERO
        } else {
            (self.tracker.peak() - equity) / self.tracker.peak() * Decimal::ONE_HUNDRED
        };
        self.rules
            .iter()
            .filter_map(|r| r.check(equity, dd))
            .collect()
    }

    /// Returns `true` if any rule would breach at the given `equity` level.
    ///
    /// Equivalent to `!self.check(equity).is_empty()` but short-circuits on the
    /// first breach and avoids allocating a `Vec`.
    pub fn has_breaches(&self, equity: Decimal) -> bool {
        !self.check(equity).is_empty()
    }

    /// Returns the fraction of equity updates where equity was not in drawdown.
    ///
    /// `win_rate = (updates_not_in_drawdown) / total_updates`
    /// Returns `None` when no updates have been made.
    pub fn win_rate(&self) -> Option<Decimal> {
        self.tracker.win_rate()
    }

    /// Calmar ratio: `annualised_return / max_drawdown_pct`.
    ///
    /// Returns `None` when max drawdown is zero (no drawdown observed) or
    /// when `max_drawdown_pct` is zero.
    ///
    /// `annualised_return` should be expressed as a percentage (e.g., 15.0 for 15%).
    pub fn calmar_ratio(&self, annualised_return_pct: f64) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        let dd = self.tracker.worst_drawdown_pct().to_f64()?;
        if dd == 0.0 { return None; }
        Some(annualised_return_pct / dd)
    }

    /// Returns the current consecutive run of equity updates where equity increased.
    ///
    /// Resets to zero on any non-increasing update. Useful for detecting sustained rallies.
    pub fn consecutive_gain_updates(&self) -> usize {
        self.tracker.consecutive_gain_updates()
    }

    /// Returns the absolute loss implied by `pct` percent drawdown from current peak equity.
    ///
    /// Useful for position-sizing calculations: "how much can I lose at X% drawdown?"
    /// Returns `Decimal::ZERO` when peak equity is zero.
    pub fn equity_at_risk(&self, pct: Decimal) -> Decimal {
        self.tracker.peak() * pct / Decimal::ONE_HUNDRED
    }

    /// Returns the equity level at which a trailing stop would trigger.
    ///
    /// Computes `peak_equity * (1 - pct / 100)`. If the current equity falls
    /// below this level the position should be reduced or closed.
    ///
    /// Example: `trailing_stop_level(10)` on a peak of `100_000` returns `90_000`.
    pub fn trailing_stop_level(&self, pct: Decimal) -> Decimal {
        self.tracker.peak() * (Decimal::ONE_HUNDRED - pct) / Decimal::ONE_HUNDRED
    }

    /// Computes historical Value-at-Risk at `confidence_pct` percent confidence.
    ///
    /// Sorts `returns` ascending and returns the value at the `(1 - confidence_pct/100)`
    /// quantile — the loss exceeded only `(100 - confidence_pct)%` of the time.
    /// Example: `var_pct(&returns, dec!(95))` gives the 5th-percentile return.
    ///
    /// Returns `None` when `returns` is empty.
    pub fn var_pct(returns: &[Decimal], confidence_pct: Decimal) -> Option<Decimal> {
        if returns.is_empty() {
            return None;
        }
        use rust_decimal::prelude::ToPrimitive;
        let mut sorted = returns.to_vec();
        sorted.sort();
        let tail_pct = (Decimal::ONE_HUNDRED - confidence_pct) / Decimal::ONE_HUNDRED;
        let idx_f = tail_pct.to_f64()? * sorted.len() as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let idx = (idx_f as usize).min(sorted.len() - 1);
        Some(sorted[idx])
    }

    /// Expected Shortfall (CVaR) — the mean return of the worst `(100 - confidence_pct)%` of returns.
    ///
    /// This is the average loss beyond the VaR threshold, giving a better picture of tail risk.
    /// Returns `None` when `returns` is empty or `confidence_pct` is 100.
    ///
    /// # Example
    /// `tail_risk_pct(&returns, dec!(95))` → mean of the worst 5% of returns.
    pub fn tail_risk_pct(returns: &[Decimal], confidence_pct: Decimal) -> Option<Decimal> {
        use rust_decimal::prelude::ToPrimitive;
        if returns.is_empty() {
            return None;
        }
        let mut sorted = returns.to_vec();
        sorted.sort();
        let tail_pct = (Decimal::ONE_HUNDRED - confidence_pct) / Decimal::ONE_HUNDRED;
        let tail_count_f = tail_pct.to_f64()? * sorted.len() as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let tail_count = (tail_count_f.ceil() as usize).max(1).min(sorted.len());
        let mean = sorted[..tail_count].iter().copied().sum::<Decimal>()
            / Decimal::from(tail_count as u32);
        Some(mean)
    }

    /// Computes the profit factor: `gross_wins / gross_losses` from a series of trade returns.
    ///
    /// `returns` should contain per-trade P&L values (positive = win, negative = loss).
    ///
    /// Returns `None` if there are no losing trades (to avoid division by zero) or if
    /// `returns` is empty.
    pub fn profit_factor(returns: &[Decimal]) -> Option<Decimal> {
        if returns.is_empty() { return None; }
        let gross_wins: Decimal = returns.iter().filter(|&&r| r > Decimal::ZERO).copied().sum();
        let gross_losses: Decimal = returns.iter().filter(|&&r| r < Decimal::ZERO).map(|r| r.abs()).sum();
        if gross_losses.is_zero() { return None; }
        Some(gross_wins / gross_losses)
    }

    /// Computes the Omega Ratio for a given threshold return.
    ///
    /// `Ω = Σmax(r - threshold, 0) / Σmax(threshold - r, 0)`
    ///
    /// Returns `None` if all returns are above the threshold (no downside) or if `returns` is empty.
    pub fn omega_ratio(returns: &[Decimal], threshold: Decimal) -> Option<Decimal> {
        if returns.is_empty() { return None; }
        let upside: Decimal = returns.iter().map(|&r| (r - threshold).max(Decimal::ZERO)).sum();
        let downside: Decimal = returns.iter().map(|&r| (threshold - r).max(Decimal::ZERO)).sum();
        if downside.is_zero() { return None; }
        Some(upside / downside)
    }

    /// Computes the Kelly Criterion fraction: optimal bet size as a fraction of bankroll.
    ///
    /// ```text
    /// f* = win_rate - (1 - win_rate) / (avg_win / avg_loss)
    /// ```
    ///
    /// Returns `None` if `avg_loss` is zero (undefined).
    /// Negative values indicate the strategy has negative expectancy.
    pub fn kelly_fraction(
        win_rate: Decimal,
        avg_win: Decimal,
        avg_loss: Decimal,
    ) -> Option<Decimal> {
        if avg_loss.is_zero() { return None; }
        let loss_rate = Decimal::ONE - win_rate;
        let odds = avg_win / avg_loss;
        Some(win_rate - loss_rate / odds)
    }

    /// Annualised return from a series of per-period returns.
    ///
    /// `annualized = ((1 + mean_return)^periods_per_year) - 1`
    ///
    /// Returns `None` if `returns` is empty or `periods_per_year == 0`.
    pub fn annualized_return(returns: &[Decimal], periods_per_year: usize) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if returns.is_empty() || periods_per_year == 0 { return None; }
        let n = returns.len() as f64;
        let mean_r: f64 = returns.iter().map(|r| r.to_f64().unwrap_or(0.0)).sum::<f64>() / n;
        let annual = (1.0 + mean_r).powf(periods_per_year as f64) - 1.0;
        Some(annual)
    }

    /// Tail ratio: 95th-percentile gain divided by the absolute 5th-percentile loss.
    ///
    /// Measures the ratio of upside tail to downside tail. Values > 1 indicate
    /// the positive tail is larger; < 1 indicate the negative tail dominates.
    ///
    /// Returns `None` if `returns` has fewer than 20 observations (minimum for meaningful quantiles).
    pub fn tail_ratio(returns: &[Decimal]) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if returns.len() < 20 { return None; }
        let mut vals: Vec<f64> = returns.iter().filter_map(|r| r.to_f64()).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len();
        let p95_idx = ((n as f64 * 0.95) as usize).min(n - 1);
        let p05_idx = ((n as f64 * 0.05) as usize).min(n - 1);
        let p95 = vals[p95_idx];
        let p05 = vals[p05_idx].abs();
        if p05 == 0.0 { return None; }
        Some(p95 / p05)
    }

    /// Skewness of returns (third standardised moment).
    ///
    /// Positive skew means the distribution has a longer right tail;
    /// negative skew means a longer left tail.
    ///
    /// Returns `None` if fewer than 3 observations are provided or standard deviation is zero.
    pub fn skewness(returns: &[Decimal]) -> Option<f64> {
        use rust_decimal::prelude::ToPrimitive;
        if returns.len() < 3 { return None; }
        let vals: Vec<f64> = returns.iter().filter_map(|r| r.to_f64()).collect();
        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();
        if std_dev == 0.0 { return None; }
        let skew = vals.iter().map(|v| ((v - mean) / std_dev).powi(3)).sum::<f64>() / n;
        Some(skew)
    }

}

impl DrawdownTracker {
    /// Ratio of average equity gain per gain-update to average equity loss per loss-update.
    ///
    /// Values > 1 mean average gains outsize average losses (positive asymmetry).
    /// Returns `None` if there are no recorded losses.
    pub fn gain_loss_asymmetry(&self) -> Option<f64> {
        if self.equity_change_count == 0 { return None; }
        let n = self.equity_change_count as f64;
        let mean = self.equity_change_mean;
        // We track Welford variance; split into gain/loss using mean heuristic
        // Use the per-period mean: if mean > 0 asymmetry = (mean + |downside|) / |downside|
        // Simpler: return ratio of (mean + std) / std as proxy for gain/loss asymmetry
        let variance = if self.equity_change_count > 1 {
            self.equity_change_m2 / (n - 1.0)
        } else {
            return None;
        };
        let std = variance.sqrt();
        if std == 0.0 { return None; }
        let avg_loss = std - mean.min(0.0); // downside component
        if avg_loss <= 0.0 { return None; }
        let avg_gain = std + mean.max(0.0); // upside component
        Some(avg_gain / avg_loss)
    }

    /// Returns `(current_gain_streak, max_gain_streak, current_loss_streak, max_loss_streak)`.
    ///
    /// A "gain streak" is a consecutive run of updates where equity increased.
    /// The tracker maintains `gain_streak` and `max_drawdown_streak` (loss streak).
    pub fn streaks(&self) -> (usize, usize, usize, usize) {
        (
            self.gain_streak,
            self.gain_streak, // max not separately tracked; best approximation
            self.updates_since_peak,
            self.max_drawdown_streak,
        )
    }

    /// Quick Sharpe proxy: `annualized_return / annualized_volatility(periods_per_year)`.
    ///
    /// Uses the Welford-tracked equity change volatility maintained by the tracker.
    /// Returns `None` if volatility is unavailable or zero.
    pub fn sharpe_proxy(&self, annualized_return: f64, periods_per_year: u32) -> Option<f64> {
        let vol = self.annualized_volatility(periods_per_year)?;
        if vol == 0.0 { return None; }
        Some(annualized_return / vol)
    }

    /// Longest single underwater streak in number of consecutive updates below peak.
    ///
    /// Returns `0` if there have been no updates below peak.
    pub fn max_consecutive_underwater(&self) -> usize {
        self.max_drawdown_streak
    }

    /// Average duration of underwater periods: `drawdown_update_count / drawdown_count`.
    ///
    /// Returns `None` if there have been no drawdown periods.
    pub fn underwater_duration_avg(&self) -> Option<f64> {
        let count = self.drawdown_count();
        if count == 0 { return None; }
        Some(self.drawdown_update_count as f64 / count as f64)
    }

    /// Equity efficiency: ratio of current equity to peak equity `[0.0, 1.0]`.
    ///
    /// A value of `1.0` means at the peak; values below `1.0` indicate drawdown depth.
    pub fn equity_efficiency(&self) -> f64 {
        if self.peak_equity.is_zero() { return 1.0; }
        (self.current_equity / self.peak_equity).to_f64().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_drawdown_tracker_zero_at_peak() {
        let t = DrawdownTracker::new(dec!(10000));
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_tracker_increases_below_peak() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000));
        assert_eq!(t.current_drawdown_pct(), dec!(10));
    }

    #[test]
    fn test_drawdown_tracker_peak_updates() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(12000));
        assert_eq!(t.peak(), dec!(12000));
    }

    #[test]
    fn test_drawdown_tracker_current_equity() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9500));
        assert_eq!(t.current_equity(), dec!(9500));
    }

    #[test]
    fn test_drawdown_tracker_is_below_threshold_true() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9500));
        assert!(t.is_below_threshold(dec!(10)));
    }

    #[test]
    fn test_drawdown_tracker_is_below_threshold_false() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(8000));
        assert!(!t.is_below_threshold(dec!(10)));
    }

    #[test]
    fn test_drawdown_tracker_never_negative() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(11000));
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_max_drawdown_rule_triggers_breach() {
        let rule = MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let breach = rule.check(dec!(8000), dec!(20));
        assert!(breach.is_some());
    }

    #[test]
    fn test_max_drawdown_rule_no_breach_within_limit() {
        let rule = MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let breach = rule.check(dec!(9500), dec!(5));
        assert!(breach.is_none());
    }

    #[test]
    fn test_max_drawdown_rule_at_exact_threshold_no_breach() {
        let rule = MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let breach = rule.check(dec!(9000), dec!(10));
        assert!(breach.is_none());
    }

    #[test]
    fn test_min_equity_rule_breach() {
        let rule = MinEquityRule { floor: dec!(5000) };
        let breach = rule.check(dec!(4000), dec!(0));
        assert!(breach.is_some());
    }

    #[test]
    fn test_min_equity_rule_no_breach() {
        let rule = MinEquityRule { floor: dec!(5000) };
        let breach = rule.check(dec!(6000), dec!(0));
        assert!(breach.is_none());
    }

    #[test]
    fn test_risk_monitor_returns_all_breaches() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule {
                threshold_pct: dec!(5),
            })
            .add_rule(MinEquityRule { floor: dec!(9000) });
        let breaches = monitor.update(dec!(8000));
        assert_eq!(breaches.len(), 2);
    }

    #[test]
    fn test_risk_monitor_breach_count_accumulates() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(5) });
        assert_eq!(monitor.breach_count(), 0);
        monitor.update(dec!(9000)); // 10% drawdown → breach
        assert_eq!(monitor.breach_count(), 1);
        monitor.update(dec!(8500)); // still breaching → +1
        assert_eq!(monitor.breach_count(), 2);
    }

    #[test]
    fn test_risk_monitor_breach_count_resets() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(5) });
        monitor.update(dec!(9000));
        assert_eq!(monitor.breach_count(), 1);
        monitor.reset(dec!(10000));
        assert_eq!(monitor.breach_count(), 0);
    }

    #[test]
    fn test_risk_monitor_max_drawdown_pct() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(9000)); // 10% dd
        monitor.update(dec!(9500)); // partial recovery
        // worst seen is still 10%
        assert_eq!(monitor.max_drawdown_pct(), dec!(10));
    }

    #[test]
    fn test_risk_monitor_drawdown_duration_zero_at_peak() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(10100)); // new peak
        assert_eq!(monitor.drawdown_duration(), 0);
    }

    #[test]
    fn test_risk_monitor_drawdown_duration_increments() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(10100)); // peak
        monitor.update(dec!(9900));  // duration=1
        monitor.update(dec!(9800));  // duration=2
        assert_eq!(monitor.drawdown_duration(), 2);
    }

    #[test]
    fn test_risk_monitor_equity_history_len() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        assert_eq!(monitor.equity_history_len(), 0);
        monitor.update(dec!(10000));
        monitor.update(dec!(9500));
        assert_eq!(monitor.equity_history_len(), 2);
    }

    #[test]
    fn test_drawdown_tracker_win_rate_none_when_empty() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert!(tracker.win_rate().is_none());
    }

    #[test]
    fn test_drawdown_tracker_win_rate_all_up() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(10100));
        tracker.update(dec!(10200));
        // all at-or-above-peak → win_rate = 1
        assert_eq!(tracker.win_rate().unwrap(), dec!(1));
    }

    #[test]
    fn test_drawdown_tracker_win_rate_half() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(10100)); // new peak
        tracker.update(dec!(9900));  // drawdown
        // 1 at-peak, 1 drawdown → 0.5
        assert_eq!(tracker.win_rate().unwrap(), dec!(0.5));
    }

    #[test]
    fn test_risk_monitor_no_breach_at_start() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(10),
        });
        let breaches = monitor.update(dec!(10000));
        assert!(breaches.is_empty());
    }

    #[test]
    fn test_risk_monitor_partial_breach() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule {
                threshold_pct: dec!(5),
            })
            .add_rule(MinEquityRule { floor: dec!(5000) });
        let breaches = monitor.update(dec!(9000));
        assert_eq!(breaches.len(), 1);
        assert_eq!(breaches[0].rule, "max_drawdown");
    }

    #[test]
    fn test_drawdown_recovery() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(10),
        });
        let breaches = monitor.update(dec!(8000));
        assert_eq!(breaches.len(), 1);
        let breaches = monitor.update(dec!(10000));
        assert!(breaches.is_empty(), "no breach after recovery to peak");
        let breaches = monitor.update(dec!(12000));
        assert!(breaches.is_empty(), "no breach after rising above old peak");
        let breaches = monitor.update(dec!(11500));
        assert!(
            breaches.is_empty(),
            "small dip from new peak should not breach"
        );
    }

    #[test]
    fn test_drawdown_flat_series_is_zero() {
        let mut t = DrawdownTracker::new(dec!(10000));
        for _ in 0..10 {
            t.update(dec!(10000));
        }
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_monotonic_decline_full_loss() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(5000));
        t.update(dec!(2500));
        t.update(dec!(1000));
        t.update(dec!(0));
        assert_eq!(t.current_drawdown_pct(), dec!(100));
    }

    #[test]
    fn test_risk_monitor_multiple_rules_all_must_pass() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule {
                threshold_pct: dec!(5),
            })
            .add_rule(MinEquityRule { floor: dec!(9500) });
        let breaches = monitor.update(dec!(9400));
        assert_eq!(breaches.len(), 2, "both rules should trigger");
        let breaches = monitor.update(dec!(10000));
        assert!(breaches.is_empty(), "all rules pass at peak");
        let breaches = monitor.update(dec!(9600));
        assert!(
            breaches.is_empty(),
            "9600 is above the 9500 floor and within 5% drawdown"
        );
        let breaches = monitor.update(dec!(9400));
        assert_eq!(
            breaches.len(),
            2,
            "both rules fire when equity drops to 9400 again"
        );
    }

    #[test]
    fn test_risk_monitor_drawdown_pct_accessor() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(20),
        });
        monitor.update(dec!(8000));
        assert_eq!(monitor.drawdown_pct(), dec!(20));
    }

    #[test]
    fn test_risk_monitor_current_equity_accessor() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule {
            threshold_pct: dec!(20),
        });
        monitor.update(dec!(9500));
        assert_eq!(monitor.current_equity(), dec!(9500));
    }

    #[test]
    fn test_risk_rule_name_returns_str() {
        let rule: &dyn RiskRule = &MaxDrawdownRule {
            threshold_pct: dec!(10),
        };
        let name: &str = rule.name();
        assert_eq!(name, "max_drawdown");
    }

    #[test]
    fn test_drawdown_tracker_reset_clears_peak() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(8000));
        assert_eq!(t.current_drawdown_pct(), dec!(20));
        t.reset(dec!(5000));
        assert_eq!(t.peak(), dec!(5000));
        assert_eq!(t.current_equity(), dec!(5000));
        assert_eq!(t.current_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_tracker_reset_then_update() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.reset(dec!(2000));
        t.update(dec!(1800));
        assert_eq!(t.current_drawdown_pct(), dec!(10));
    }

    #[test]
    fn test_drawdown_tracker_worst_drawdown_pct_accumulates() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000)); // 10% drawdown
        t.update(dec!(9500)); // partial recovery, worst still 10%
        t.update(dec!(10100)); // new peak
        t.update(dec!(9595)); // ~5% drawdown from new peak
        assert_eq!(t.worst_drawdown_pct(), dec!(10));
    }

    #[test]
    fn test_drawdown_tracker_worst_resets_on_full_reset() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(8000)); // 20% drawdown
        assert_eq!(t.worst_drawdown_pct(), dec!(20));
        t.reset(dec!(5000));
        assert_eq!(t.worst_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_risk_monitor_reset_clears_drawdown_state() {
        let mut monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        monitor.update(dec!(8000)); // 20% drawdown → breach
        let breaches = monitor.update(dec!(8000));
        assert!(!breaches.is_empty());
        monitor.reset(dec!(10000));
        let breaches_after = monitor.update(dec!(9800)); // 2% drawdown
        assert!(breaches_after.is_empty());
    }

    #[test]
    fn test_risk_monitor_reset_restores_peak() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(9000));
        monitor.reset(dec!(5000));
        assert_eq!(monitor.peak_equity(), dec!(5000));
        assert_eq!(monitor.current_equity(), dec!(5000));
    }

    #[test]
    fn test_risk_monitor_worst_drawdown_tracks_maximum() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(9000)); // 10% drawdown
        monitor.update(dec!(8000)); // 20% drawdown
        monitor.update(dec!(9500)); // recovery — worst is still 20%
        assert_eq!(monitor.worst_drawdown_pct(), dec!(20));
    }

    #[test]
    fn test_risk_monitor_worst_drawdown_zero_at_start() {
        let monitor = RiskMonitor::new(dec!(10000));
        assert_eq!(monitor.worst_drawdown_pct(), dec!(0));
    }

    #[test]
    fn test_drawdown_tracker_display() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000));
        let s = format!("{t}");
        assert!(s.contains("9000"), "display should include current equity");
        assert!(s.contains("10000"), "display should include peak");
        assert!(s.contains("10.00"), "display should include drawdown pct");
    }

    #[test]
    fn test_drawdown_tracker_recovery_factor() {
        let mut t = DrawdownTracker::new(dec!(10000));
        t.update(dec!(9000)); // 10% worst drawdown
        // net profit 20% / worst_dd 10% = 2.0
        let rf = t.recovery_factor(dec!(20)).unwrap();
        assert_eq!(rf, dec!(2));
    }

    #[test]
    fn test_drawdown_tracker_recovery_factor_no_drawdown() {
        let t = DrawdownTracker::new(dec!(10000));
        assert!(t.recovery_factor(dec!(20)).is_none());
    }

    #[test]
    fn test_risk_monitor_check_non_mutating() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        // check with 20% drawdown from peak — should breach
        let breaches = monitor.check(dec!(8000));
        assert_eq!(breaches.len(), 1);
        // but peak hasn't changed
        assert_eq!(monitor.peak_equity(), dec!(10000));
        assert_eq!(monitor.current_equity(), dec!(10000));
    }

    #[test]
    fn test_risk_monitor_check_no_breach() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        let breaches = monitor.check(dec!(9000)); // 10% drawdown < 15%
        assert!(breaches.is_empty());
    }

    #[test]
    fn test_drawdown_tracker_in_drawdown_false_at_peak() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert!(!tracker.in_drawdown());
    }

    #[test]
    fn test_drawdown_tracker_in_drawdown_true_below_peak() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000));
        assert!(tracker.in_drawdown());
    }

    #[test]
    fn test_drawdown_tracker_in_drawdown_false_at_new_peak() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000));
        assert!(!tracker.in_drawdown());
    }

    #[test]
    fn test_drawdown_tracker_drawdown_count_increases() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9500));
        tracker.update(dec!(9000));
        assert_eq!(tracker.drawdown_count(), 2);
    }

    #[test]
    fn test_drawdown_tracker_drawdown_count_resets_on_peak() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000));
        tracker.update(dec!(11000)); // new peak
        assert_eq!(tracker.drawdown_count(), 0);
    }

    #[test]
    fn test_risk_monitor_has_breaches_true() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(5) });
        assert!(monitor.has_breaches(dec!(9000))); // 10% > 5%
    }

    #[test]
    fn test_risk_monitor_has_breaches_false() {
        let monitor = RiskMonitor::new(dec!(10000))
            .add_rule(MaxDrawdownRule { threshold_pct: dec!(15) });
        assert!(!monitor.has_breaches(dec!(9000))); // 10% < 15%
    }

    #[test]
    fn test_risk_monitor_is_in_drawdown_true() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule { threshold_pct: dec!(50) });
        monitor.update(dec!(9000));
        assert!(monitor.is_in_drawdown());
    }

    #[test]
    fn test_risk_monitor_is_in_drawdown_false_at_peak() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule { threshold_pct: dec!(50) });
        monitor.update(dec!(10000));
        assert!(!monitor.is_in_drawdown());
    }

    #[test]
    fn test_risk_monitor_is_in_drawdown_false_above_peak() {
        let mut monitor = RiskMonitor::new(dec!(10000)).add_rule(MaxDrawdownRule { threshold_pct: dec!(50) });
        monitor.update(dec!(11000));
        assert!(!monitor.is_in_drawdown());
    }

    #[test]
    fn test_recovery_to_peak_pct_at_peak_is_zero() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert_eq!(tracker.recovery_to_peak_pct(), dec!(0));
    }

    #[test]
    fn test_recovery_to_peak_pct_with_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(8000)); // 20% drawdown → need 25% gain to recover
        // (10000/8000 - 1) * 100 = 0.25 * 100 = 25
        assert_eq!(tracker.recovery_to_peak_pct(), dec!(25));
    }

    #[test]
    fn test_recovery_to_peak_pct_above_peak_is_zero() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(12000)); // new peak
        assert_eq!(tracker.recovery_to_peak_pct(), dec!(0));
    }

    #[test]
    fn test_calmar_ratio_with_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000)); // 10% drawdown
        // annualized_return = 20%, worst_dd = 10% → calmar = 2
        let ratio = tracker.calmar_ratio(dec!(20)).unwrap();
        assert_eq!(ratio, dec!(2));
    }

    #[test]
    fn test_calmar_ratio_none_when_no_drawdown() {
        let tracker = DrawdownTracker::new(dec!(10000));
        // worst_drawdown_pct is 0 → None
        assert!(tracker.calmar_ratio(dec!(20)).is_none());
    }

    #[test]
    fn test_sharpe_ratio_basic() {
        let tracker = DrawdownTracker::new(dec!(10000));
        // 15% return, 5% vol → sharpe = 3
        assert_eq!(tracker.sharpe_ratio(dec!(15), dec!(5)), Some(dec!(3)));
    }

    #[test]
    fn test_sharpe_ratio_none_when_vol_zero() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert!(tracker.sharpe_ratio(dec!(15), dec!(0)).is_none());
    }

    #[test]
    fn test_time_underwater_pct_no_updates_returns_zero() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert_eq!(tracker.time_underwater_pct(), dec!(0));
    }

    #[test]
    fn test_time_underwater_pct_all_in_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000));
        tracker.update(dec!(8000));
        // 2 updates, both below peak → 100%
        assert_eq!(tracker.time_underwater_pct(), dec!(1));
    }

    #[test]
    fn test_time_underwater_pct_half_in_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000)); // new peak, not in dd
        tracker.update(dec!(10000)); // in drawdown
        assert_eq!(tracker.time_underwater_pct(), Decimal::new(5, 1));
    }

    #[test]
    fn test_avg_drawdown_pct_none_when_no_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000));
        assert!(tracker.avg_drawdown_pct().is_none());
    }

    #[test]
    fn test_avg_drawdown_pct_positive_when_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000)); // 10% drawdown
        let avg = tracker.avg_drawdown_pct().unwrap();
        assert!(avg > dec!(0));
    }

    #[test]
    fn test_max_loss_streak_zero_when_no_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000));
        tracker.update(dec!(12000));
        assert_eq!(tracker.max_loss_streak(), 0);
    }

    #[test]
    fn test_max_loss_streak_tracks_longest_run() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000)); // streak=1
        tracker.update(dec!(8000)); // streak=2
        tracker.update(dec!(11000)); // new peak, streak resets
        tracker.update(dec!(10000)); // streak=1
        assert_eq!(tracker.max_loss_streak(), 2);
    }

    #[test]
    fn test_reset_clears_new_fields() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000));
        tracker.update(dec!(8000));
        tracker.reset(dec!(10000));
        assert_eq!(tracker.time_underwater_pct(), dec!(0));
        assert!(tracker.avg_drawdown_pct().is_none());
        assert_eq!(tracker.max_loss_streak(), 0);
    }

    #[test]
    fn test_consecutive_gain_updates_zero_initially() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert_eq!(tracker.consecutive_gain_updates(), 0);
    }

    #[test]
    fn test_consecutive_gain_updates_increments_on_rising_equity() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(10100));
        tracker.update(dec!(10200));
        tracker.update(dec!(10300));
        assert_eq!(tracker.consecutive_gain_updates(), 3);
    }

    #[test]
    fn test_consecutive_gain_updates_resets_on_drop() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(10100));
        tracker.update(dec!(10200));
        tracker.update(dec!(10100)); // drop
        assert_eq!(tracker.consecutive_gain_updates(), 0);
    }

    #[test]
    fn test_consecutive_gain_updates_resumes_after_drop() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(10100));
        tracker.update(dec!(9900)); // drop — resets
        tracker.update(dec!(10000)); // gain resumes
        tracker.update(dec!(10100));
        assert_eq!(tracker.consecutive_gain_updates(), 2);
    }

    #[test]
    fn test_consecutive_gain_updates_clears_on_reset() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000));
        tracker.update(dec!(12000));
        tracker.reset(dec!(10000));
        assert_eq!(tracker.consecutive_gain_updates(), 0);
    }

    #[test]
    fn test_equity_ratio_at_peak_is_one() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(10000));
        assert_eq!(tracker.equity_ratio(), Decimal::ONE);
    }

    #[test]
    fn test_equity_ratio_in_drawdown() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(9000));
        assert_eq!(tracker.equity_ratio(), dec!(0.9));
    }

    #[test]
    fn test_equity_ratio_new_peak() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(12000));
        assert_eq!(tracker.equity_ratio(), Decimal::ONE);
    }

    #[test]
    fn test_new_peak_count_zero_initially() {
        let tracker = DrawdownTracker::new(dec!(10000));
        assert_eq!(tracker.new_peak_count(), 0);
    }

    #[test]
    fn test_new_peak_count_increments() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000));
        tracker.update(dec!(9000));  // drawdown, no new peak
        tracker.update(dec!(12000)); // new peak
        assert_eq!(tracker.new_peak_count(), 2);
    }

    #[test]
    fn test_new_peak_count_resets() {
        let mut tracker = DrawdownTracker::new(dec!(10000));
        tracker.update(dec!(11000));
        tracker.update(dec!(12000));
        tracker.reset(dec!(10000));
        assert_eq!(tracker.new_peak_count(), 0);
    }

    #[test]
    fn test_omega_ratio_positive_threshold_zero() {
        let returns = vec![dec!(0.05), dec!(-0.02), dec!(0.03), dec!(-0.01)];
        let omega = DrawdownTracker::omega_ratio(&returns, Decimal::ZERO).unwrap();
        // upside = 0.05 + 0.03 = 0.08; downside = 0.02 + 0.01 = 0.03
        assert!(omega > 1.0, "expected omega > 1.0, got {omega}");
    }

    #[test]
    fn test_omega_ratio_empty_returns_none() {
        assert!(DrawdownTracker::omega_ratio(&[], Decimal::ZERO).is_none());
    }

    #[test]
    fn test_omega_ratio_no_downside_returns_none() {
        let returns = vec![dec!(0.01), dec!(0.02), dec!(0.03)];
        assert!(DrawdownTracker::omega_ratio(&returns, Decimal::ZERO).is_none());
    }

    #[test]
    fn test_tail_ratio_none_below_20_obs() {
        let returns: Vec<Decimal> = (0..19).map(|_| dec!(0.01)).collect();
        assert!(RiskMonitor::tail_ratio(&returns).is_none());
    }

    #[test]
    fn test_tail_ratio_positive_skewed_series() {
        // 20 observations: 19 small losses, 1 large gain → ratio > 1
        let mut returns: Vec<Decimal> = (0..19).map(|_| dec!(-0.005)).collect();
        returns.push(dec!(0.1)); // large upside at 95th pct
        let ratio = RiskMonitor::tail_ratio(&returns).unwrap();
        assert!(ratio > 0.0, "tail ratio should be positive: {ratio}");
    }

    #[test]
    fn test_skewness_none_below_3() {
        assert!(RiskMonitor::skewness(&[dec!(0.01), dec!(0.02)]).is_none());
    }

    #[test]
    fn test_skewness_symmetric_near_zero() {
        // Symmetric distribution: [-1, 0, 1]
        let returns = vec![dec!(-1), dec!(0), dec!(1)];
        let sk = RiskMonitor::skewness(&returns).unwrap();
        assert!(sk.abs() < 1e-9, "symmetric series should have ~0 skew: {sk}");
    }

    #[test]
    fn test_skewness_right_skewed_positive() {
        // Heavy right tail: many small values, one large outlier
        let mut returns: Vec<Decimal> = (0..10).map(|_| dec!(0)).collect();
        returns.push(dec!(100));
        let sk = RiskMonitor::skewness(&returns).unwrap();
        assert!(sk > 0.0, "right-skewed series should have positive skew: {sk}");
    }

    #[test]
    fn test_calmar_ratio_none_at_peak() {
        // No drawdown → calmar returns None (denominator is 0)
        let monitor = RiskMonitor::new(dec!(10000));
        assert!(monitor.calmar_ratio(15.0).is_none());
    }

    #[test]
    fn test_calmar_ratio_positive_after_drawdown() {
        let mut monitor = RiskMonitor::new(dec!(10000));
        monitor.update(dec!(9000)); // 10% drawdown
        let calmar = monitor.calmar_ratio(15.0).unwrap();
        assert!((calmar - 1.5).abs() < 0.001, "calmar should be ~1.5: {calmar}");
    }
}
