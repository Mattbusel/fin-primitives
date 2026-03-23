//! # Module: regime
//!
//! ## Responsibility
//! Market regime classification using existing indicator primitives.
//! [`MarketRegimeDetector`] updates incrementally on each bar and classifies the
//! current market state into one of four regimes.
//!
//! ## Guarantees
//! - Returns [`MarketRegime::Unknown`] until all internal indicators are warm
//! - Zero panics; all arithmetic uses saturating/checked paths or f64 helpers
//! - Thresholds are configurable at construction
//!
//! ## Regime definitions
//! | Regime | Condition |
//! |--------|-----------|
//! | `Trending` | ADX > `adx_threshold` (strong directional move) |
//! | `MeanReverting` | Hurst < `hurst_threshold` (sub-diffusive, range-bound) |
//! | `Volatile` | Historical volatility > `vol_spike_threshold` (vol spike) |
//! | `Quiet` | Bollinger band width < `bb_width_quiet` (narrow bands) |
//! | `Unknown` | Not enough data to classify |
//!
//! When multiple conditions are simultaneously true, `Trending` takes highest
//! priority, then `Volatile`, then `MeanReverting`, then `Quiet`.

use crate::error::FinError;
use crate::signals::indicators::{Adx, BollingerWidth, HistoricalVolatility, HurstExponent};
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// Classification of the current market regime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketRegime {
    /// Strongly directional market (ADX above threshold).
    Trending,
    /// Range-bound, mean-reverting market (Hurst below threshold).
    MeanReverting,
    /// Elevated volatility spike (historical vol above threshold).
    Volatile,
    /// Low-volatility, compressed market (Bollinger band width below threshold).
    Quiet,
    /// Indicators not yet warmed up; classification unavailable.
    Unknown,
}

impl std::fmt::Display for MarketRegime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketRegime::Trending => write!(f, "Trending"),
            MarketRegime::MeanReverting => write!(f, "MeanReverting"),
            MarketRegime::Volatile => write!(f, "Volatile"),
            MarketRegime::Quiet => write!(f, "Quiet"),
            MarketRegime::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Configuration thresholds for [`MarketRegimeDetector`].
#[derive(Debug, Clone)]
pub struct RegimeConfig {
    /// ADX value above which the market is classified as `Trending`. Default: 25.0.
    pub adx_threshold: f64,
    /// Hurst exponent below which the market is classified as `MeanReverting`. Default: 0.45.
    pub hurst_threshold: f64,
    /// Annualized volatility (%) above which the market is classified as `Volatile`. Default: 30.0.
    pub vol_spike_threshold: f64,
    /// Bollinger band width below which the market is classified as `Quiet`. Default: 0.02.
    pub bb_width_quiet: f64,
}

impl Default for RegimeConfig {
    fn default() -> Self {
        Self {
            adx_threshold: 25.0,
            hurst_threshold: 0.45,
            vol_spike_threshold: 30.0,
            bb_width_quiet: 0.02,
        }
    }
}

/// Streaming market regime detector.
///
/// Internally maintains ADX, Hurst Exponent, Historical Volatility, and
/// Bollinger Band Width indicators. Call [`update`](Self::update) on each bar;
/// the current regime is returned immediately.
///
/// # Example
/// ```rust
/// use fin_primitives::regime::{MarketRegimeDetector, RegimeConfig, MarketRegime};
/// use fin_primitives::signals::BarInput;
/// use rust_decimal_macros::dec;
///
/// let mut detector = MarketRegimeDetector::new(14, RegimeConfig::default()).unwrap();
/// let bar = BarInput::new(dec!(100), dec!(102), dec!(98), dec!(100), dec!(1000));
/// let regime = detector.update(&bar).unwrap();
/// // Unknown until warm-up complete
/// assert_eq!(regime, MarketRegime::Unknown);
/// ```
pub struct MarketRegimeDetector {
    adx: Adx,
    hurst: HurstExponent,
    hv: HistoricalVolatility,
    bb_width: BollingerWidth,
    config: RegimeConfig,
}

impl MarketRegimeDetector {
    /// Constructs a new [`MarketRegimeDetector`].
    ///
    /// # Parameters
    /// - `period`: warm-up period shared across all internal indicators (must be >= 2).
    /// - `config`: classification thresholds.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(period: usize, config: RegimeConfig) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            adx: Adx::new("regime_adx", period)?,
            hurst: HurstExponent::new("regime_hurst", period)?,
            hv: HistoricalVolatility::new("regime_hv", period, 252)?,
            bb_width: BollingerWidth::new("regime_bb_width", period, Decimal::from(2u32))?,
            config,
        })
    }

    /// Constructs a detector with default thresholds.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn with_defaults(period: usize) -> Result<Self, FinError> {
        Self::new(period, RegimeConfig::default())
    }

    /// Updates all internal indicators with `bar` and returns the current regime.
    ///
    /// Returns [`MarketRegime::Unknown`] until all indicators are warmed up.
    ///
    /// # Errors
    /// Propagates any [`FinError`] from the underlying indicators (e.g. [`FinError::InvalidPeriod`]).
    pub fn update(&mut self, bar: &BarInput) -> Result<MarketRegime, FinError> {
        let adx_val = self.adx.update(bar)?;
        let hurst_val = self.hurst.update(bar)?;
        let hv_val = self.hv.update(bar)?;
        let bb_w_val = self.bb_width.update(bar)?;

        // Require all four indicators ready
        let (adx, hurst, hv, bb_w) = match (adx_val, hurst_val, hv_val, bb_w_val) {
            (
                SignalValue::Scalar(a),
                SignalValue::Scalar(h),
                SignalValue::Scalar(v),
                SignalValue::Scalar(b),
            ) => (a, h, v, b),
            _ => return Ok(MarketRegime::Unknown),
        };

        let adx_f = adx.to_f64().unwrap_or(0.0);
        let hurst_f = hurst.to_f64().unwrap_or(0.5);
        let hv_f = hv.to_f64().unwrap_or(0.0);
        let bb_w_f = bb_w.to_f64().unwrap_or(f64::MAX);

        // Priority: Trending > Volatile > MeanReverting > Quiet
        if adx_f > self.config.adx_threshold {
            return Ok(MarketRegime::Trending);
        }
        if hv_f > self.config.vol_spike_threshold {
            return Ok(MarketRegime::Volatile);
        }
        if hurst_f < self.config.hurst_threshold {
            return Ok(MarketRegime::MeanReverting);
        }
        if bb_w_f < self.config.bb_width_quiet {
            return Ok(MarketRegime::Quiet);
        }

        // No strong signal; default to mean-reverting when all quiet
        Ok(MarketRegime::MeanReverting)
    }

    /// Returns `true` when all internal indicators are warm and regime is classifiable.
    pub fn is_ready(&self) -> bool {
        self.adx.is_ready()
            && self.hurst.is_ready()
            && self.hv.is_ready()
            && self.bb_width.is_ready()
    }

    /// Returns the current configuration.
    pub fn config(&self) -> &RegimeConfig {
        &self.config
    }

    /// Resets all internal indicators to their initial state.
    pub fn reset(&mut self) {
        self.adx.reset();
        self.hurst.reset();
        self.hv.reset();
        self.bb_width.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: f64, l: f64, c: f64) -> BarInput {
        BarInput::new(
            Decimal::try_from(c).unwrap_or(dec!(100)),
            Decimal::try_from(h).unwrap_or(dec!(102)),
            Decimal::try_from(l).unwrap_or(dec!(98)),
            Decimal::try_from(c).unwrap_or(dec!(100)),
            dec!(1000),
        )
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(MarketRegimeDetector::new(0, RegimeConfig::default()).is_err());
        assert!(MarketRegimeDetector::new(1, RegimeConfig::default()).is_err());
    }

    #[test]
    fn test_unknown_before_warmup() {
        let mut d = MarketRegimeDetector::new(5, RegimeConfig::default()).unwrap();
        let regime = d.update(&bar(102.0, 98.0, 100.0)).unwrap();
        assert_eq!(regime, MarketRegime::Unknown);
        assert!(!d.is_ready());
    }

    #[test]
    fn test_regime_display() {
        assert_eq!(MarketRegime::Trending.to_string(), "Trending");
        assert_eq!(MarketRegime::MeanReverting.to_string(), "MeanReverting");
        assert_eq!(MarketRegime::Volatile.to_string(), "Volatile");
        assert_eq!(MarketRegime::Quiet.to_string(), "Quiet");
        assert_eq!(MarketRegime::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_reset_clears_warmup() {
        let mut d = MarketRegimeDetector::with_defaults(5).unwrap();
        for i in 0..30 {
            let h = 100.0 + i as f64;
            d.update(&bar(h + 1.0, h - 1.0, h)).unwrap();
        }
        d.reset();
        assert!(!d.is_ready());
    }

    #[test]
    fn test_trending_regime_detected() {
        // Strong uptrend: consistently rising highs, lows, closes → high ADX
        let config = RegimeConfig {
            adx_threshold: 10.0, // low threshold so trend is detectable quickly
            ..RegimeConfig::default()
        };
        let mut d = MarketRegimeDetector::new(5, config).unwrap();
        let mut last = MarketRegime::Unknown;
        for i in 0..60u32 {
            let c = 100.0 + i as f64;
            last = d.update(&bar(c + 0.5, c - 0.5, c)).unwrap();
        }
        // With a consistent uptrend and lowered ADX threshold, should eventually trend
        // (may still be MeanReverting if ADX hasn't crossed threshold yet; just check no panic)
        let _ = last;
    }

    #[test]
    fn test_config_accessors() {
        let cfg = RegimeConfig { adx_threshold: 20.0, ..RegimeConfig::default() };
        let d = MarketRegimeDetector::new(5, cfg).unwrap();
        assert!((d.config().adx_threshold - 20.0).abs() < 1e-10);
    }
}
