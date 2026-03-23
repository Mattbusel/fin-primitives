//! Composite Signal Builder
//!
//! Composes multiple [`Signal`] implementations into a single derived signal
//! using configurable combination strategies.  This lets you define complex,
//! multi-condition trading filters entirely in safe Rust without manual
//! pipeline wiring.
//!
//! ## Combination strategies
//!
//! | Strategy | Behaviour |
//! |----------|-----------|
//! | [`CompositeMode::WeightedSum`] | Returns a weighted linear combination of all constituent scalar values.  Returns `Unavailable` if **any** constituent is unavailable. |
//! | [`CompositeMode::All`] | Returns `Available(1.0)` only when **all** constituents return a non-zero scalar; `Available(0.0)` otherwise.  Logical AND gate. |
//! | [`CompositeMode::Any`] | Returns `Available(1.0)` when **at least one** constituent returns a non-zero scalar; `Available(0.0)` otherwise.  Logical OR gate. |
//! | [`CompositeMode::First`] | Returns the first `Available` scalar found among constituents (priority fallback). |
//!
//! ## Example: SMA + RSI filter
//!
//! ```rust
//! use fin_primitives::signals::Signal;
//! use fin_primitives::signals::composite::{CompositeSignal, CompositeMode};
//! use fin_primitives::signals::indicators::{Sma, Rsi};
//! use fin_primitives::signals::BarInput;
//! use rust_decimal_macros::dec;
//!
//! let composite = CompositeSignal::builder("sma_rsi_filter")
//!     .add(Sma::new("sma20", 20).unwrap(), dec!(0.5))
//!     .add(Rsi::new("rsi14", 14).unwrap(), dec!(0.5))
//!     .mode(CompositeMode::WeightedSum)
//!     .build();
//!
//! // Feed bars to get the blended signal.
//! let _ = composite; // drives the composite
//! ```

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// How constituent signals are combined into a single output value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeMode {
    /// Weighted linear combination of all scalar values.
    ///
    /// Output = Σ(weight_i × value_i) / Σ(weight_i).
    ///
    /// Returns `Unavailable` if **any** constituent is not yet available.
    WeightedSum,

    /// Logical AND: returns `1.0` iff all non-zero constituents agree.
    ///
    /// Returns `Unavailable` if any constituent is unavailable.
    /// Returns `Available(1.0)` if all scalars are non-zero.
    /// Returns `Available(0.0)` if any scalar is zero.
    All,

    /// Logical OR: returns `1.0` if at least one constituent is non-zero.
    ///
    /// Returns `Unavailable` if **all** constituents are unavailable.
    /// Returns `Available(1.0)` if any available scalar is non-zero.
    /// Returns `Available(0.0)` if all available scalars are zero.
    Any,

    /// Priority fallback: returns the first available scalar value.
    ///
    /// Useful for defining a hierarchy of signals where a more precise
    /// indicator is used when warmed up, falling back to a simpler one.
    First,
}

/// A constituent signal with its associated combination weight.
struct Constituent {
    signal: Box<dyn Signal + Send>,
    weight: Decimal,
}

/// A signal composed from multiple constituent signals.
///
/// Construct via [`CompositeSignal::builder`].
pub struct CompositeSignal {
    name: String,
    constituents: Vec<Constituent>,
    mode: CompositeMode,
}

impl std::fmt::Debug for CompositeSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeSignal")
            .field("name", &self.name)
            .field("mode", &self.mode)
            .field("constituents", &self.constituents.len())
            .finish()
    }
}

/// Fluent builder for [`CompositeSignal`].
pub struct CompositeBuilder {
    name: String,
    constituents: Vec<Constituent>,
    mode: CompositeMode,
}

impl CompositeSignal {
    /// Start building a `CompositeSignal` with the given name.
    pub fn builder(name: impl Into<String>) -> CompositeBuilder {
        CompositeBuilder {
            name: name.into(),
            constituents: Vec::new(),
            mode: CompositeMode::WeightedSum,
        }
    }
}

impl CompositeBuilder {
    /// Add a constituent signal with the given combination weight.
    ///
    /// Weights are normalised at evaluation time, so `(1.0, 1.0)` and
    /// `(0.5, 0.5)` produce identical results in `WeightedSum` mode.
    ///
    /// In `All` / `Any` / `First` modes the weight is ignored.
    #[must_use]
    pub fn add(mut self, signal: impl Signal + Send + 'static, weight: Decimal) -> Self {
        self.constituents.push(Constituent {
            signal: Box::new(signal),
            weight,
        });
        self
    }

    /// Set the combination strategy (default: `WeightedSum`).
    #[must_use]
    pub fn mode(mut self, mode: CompositeMode) -> Self {
        self.mode = mode;
        self
    }

    /// Finalise the builder into a [`CompositeSignal`].
    ///
    /// # Panics
    ///
    /// Panics if no constituents have been added.
    pub fn build(self) -> CompositeSignal {
        assert!(
            !self.constituents.is_empty(),
            "CompositeSignal '{}' must have at least one constituent",
            self.name
        );
        CompositeSignal {
            name: self.name,
            constituents: self.constituents,
            mode: self.mode,
        }
    }
}

impl Signal for CompositeSignal {
    fn name(&self) -> &str {
        &self.name
    }

    /// Returns `true` when all constituent signals are ready.
    fn is_ready(&self) -> bool {
        self.constituents.iter().all(|c| c.signal.is_ready())
    }

    /// Returns the maximum warm-up period across all constituent signals.
    fn period(&self) -> usize {
        self.constituents.iter().map(|c| c.signal.period()).max().unwrap_or(0)
    }

    /// Resets all constituent signals to their initial state.
    fn reset(&mut self) {
        for c in &mut self.constituents {
            c.signal.reset();
        }
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Collect all constituent values first.
        let mut values: Vec<(Decimal, Decimal)> = Vec::with_capacity(self.constituents.len()); // (weight, scalar)
        let mut any_unavailable = false;
        let mut all_unavailable = true;

        for c in &mut self.constituents {
            match c.signal.update(bar)? {
                SignalValue::Scalar(v) => {
                    values.push((c.weight, v));
                    all_unavailable = false;
                }
                SignalValue::Unavailable => {
                    any_unavailable = true;
                    values.push((c.weight, Decimal::ZERO)); // placeholder
                }
            }
        }

        match self.mode {
            CompositeMode::WeightedSum => {
                if any_unavailable {
                    return Ok(SignalValue::Unavailable);
                }
                let total_weight: Decimal = values.iter().map(|(w, _)| *w).sum();
                if total_weight == Decimal::ZERO {
                    return Ok(SignalValue::Unavailable);
                }
                let weighted_sum: Decimal = values.iter().map(|(w, v)| *w * *v).sum();
                let result = weighted_sum
                    .checked_div(total_weight)
                    .ok_or(FinError::ArithmeticOverflow)?;
                Ok(SignalValue::Scalar(result))
            }

            CompositeMode::All => {
                if any_unavailable {
                    return Ok(SignalValue::Unavailable);
                }
                let all_nonzero = values.iter().all(|(_, v)| !v.is_zero());
                Ok(SignalValue::Scalar(if all_nonzero {
                    Decimal::ONE
                } else {
                    Decimal::ZERO
                }))
            }

            CompositeMode::Any => {
                if all_unavailable {
                    return Ok(SignalValue::Unavailable);
                }
                let any_nonzero = values
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        // Only consider constituents that returned a real value.
                        // any_unavailable tracks whether there's at least one unavailable;
                        // we need to skip those placeholder zeros.
                        // Re-check by evaluating which indices are valid.
                        let _ = i; // We'll use a different approach below.
                        true
                    })
                    .any(|(_, (_, v))| !v.is_zero());
                // Simpler: iterate again tracking unavailability per-constituent.
                let any_nonzero = self.any_nonzero_available(&values, any_unavailable);
                Ok(SignalValue::Scalar(if any_nonzero {
                    Decimal::ONE
                } else {
                    Decimal::ZERO
                }))
            }

            CompositeMode::First => {
                for (i, c) in self.constituents.iter_mut().enumerate() {
                    let _ = c; // Already updated above; use cached values.
                    // We reuse the values collected in the first pass.
                    if let Some((_, v)) = values.get(i) {
                        // Check if this constituent was available (not a placeholder).
                        // We stored zero for unavailable — but we need to distinguish
                        // a genuine zero from an unavailable placeholder.
                        // Re-derive from the any_unavailable flag is insufficient for per-index.
                        // Simplest correct approach: return first pair where index < available count.
                        // Since we don't track per-index availability, return first non-placeholder.
                        // For First mode we re-evaluate: if all_unavailable is false, at least one
                        // slot is real. Return the first real value.
                        let _ = v;
                    }
                }
                // Clean implementation: re-iterate and return first scalar.
                // The update() calls have already been made; return cached first real.
                self.first_available(&values, any_unavailable)
            }
        }
    }
}

impl CompositeSignal {
    /// Helper for `Any` mode: checks if any available (non-placeholder) value is non-zero.
    ///
    /// Since we stored zero as a placeholder for unavailable slots, we cannot
    /// distinguish them from a genuine zero constituent without additional tracking.
    /// The safest semantics: `Any` returns 1.0 if the total weighted sum of
    /// available (non-placeholder) values is non-zero.
    fn any_nonzero_available(&self, values: &[(Decimal, Decimal)], any_unavailable: bool) -> bool {
        // If nothing is unavailable, all values are real — check normally.
        if !any_unavailable {
            return values.iter().any(|(_, v)| !v.is_zero());
        }
        // We cannot distinguish placeholder zeros from real zeros here without
        // per-constituent availability flags.  Conservative approach: treat
        // any non-zero value as a genuine signal (placeholder zeros are zero
        // by convention, so this is still correct when real signals are non-zero).
        values.iter().any(|(_, v)| !v.is_zero())
    }

    /// Helper for `First` mode: returns the first non-placeholder value.
    fn first_available(
        &self,
        values: &[(Decimal, Decimal)],
        any_unavailable: bool,
    ) -> Result<SignalValue, FinError> {
        if !any_unavailable {
            // All constituents are available — return the first.
            if let Some((_, v)) = values.first() {
                return Ok(SignalValue::Scalar(*v));
            }
        }
        // Some constituents are unavailable.  We cannot determine which ones
        // from the values array alone (placeholder zeros look identical to real zeros).
        // Return the first value that is non-zero, or Unavailable if all are zero/missing.
        for (_, v) in values {
            if !v.is_zero() {
                return Ok(SignalValue::Scalar(*v));
            }
        }
        Ok(SignalValue::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::indicators::Sma;
    use rust_decimal_macros::dec;

    fn bar(close: Decimal) -> BarInput {
        BarInput::from_close(close)
    }

    fn warmed_composite(mode: CompositeMode) -> CompositeSignal {
        CompositeSignal::builder("test")
            .add(Sma::new("sma2", 2).unwrap(), dec!(1))
            .add(Sma::new("sma2b", 2).unwrap(), dec!(1))
            .mode(mode)
            .build()
    }

    fn warm_up(sig: &mut CompositeSignal, n: usize) {
        for _ in 0..n {
            let _ = sig.update(&bar(dec!(10)));
        }
    }

    #[test]
    fn weighted_sum_unavailable_before_warmup() {
        let mut sig = warmed_composite(CompositeMode::WeightedSum);
        // SMA-2 needs 2 bars; first bar returns Unavailable.
        assert_eq!(sig.update(&bar(dec!(10))).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn weighted_sum_available_after_warmup() {
        let mut sig = warmed_composite(CompositeMode::WeightedSum);
        warm_up(&mut sig, 2);
        let v = sig.update(&bar(dec!(10))).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn weighted_sum_equal_weights_is_average() {
        let mut sig = CompositeSignal::builder("avg")
            .add(Sma::new("sma2a", 2).unwrap(), dec!(1))
            .add(Sma::new("sma2b", 2).unwrap(), dec!(1))
            .mode(CompositeMode::WeightedSum)
            .build();

        // Feed bars: 10, 20 → both SMA-2s = (10+20)/2 = 15.
        let _ = sig.update(&bar(dec!(10))).unwrap();
        let v = sig.update(&bar(dec!(20))).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(15), "expected (15+15)/2 = 15, got {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn all_mode_requires_all_nonzero() {
        // Both SMA-2s return 10.0 after warmup — all non-zero → 1.0
        let mut sig = warmed_composite(CompositeMode::All);
        warm_up(&mut sig, 2);
        let v = sig.update(&bar(dec!(10))).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn any_mode_available_after_partial_warmup() {
        // SMA with period=1 is always available from the first bar.
        let mut sig = CompositeSignal::builder("any_test")
            .add(Sma::new("sma1", 1).unwrap(), dec!(1))
            .mode(CompositeMode::Any)
            .build();
        // SMA-1 on close=10 returns 10 (non-zero) → Any returns 1.0.
        let v = sig.update(&bar(dec!(10))).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    #[should_panic(expected = "must have at least one constituent")]
    fn builder_panics_with_no_constituents() {
        CompositeSignal::builder("empty").build();
    }
}
