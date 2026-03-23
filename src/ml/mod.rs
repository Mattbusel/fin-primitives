//! # Module: ml
//!
//! ## Responsibility
//! ML feature vector construction from indicator snapshots.
//! Provides a [`FeatureVector`] builder that captures N indicator outputs at a point in time,
//! normalizes them (z-score or min-max), and serializes to `Vec<f64>` for ML pipelines.
//!
//! ## Guarantees
//! - Zero panics: all fallible operations return `Result<_, FinError>`
//! - Feature names are validated at construction; duplicates are rejected
//! - Normalization is stable in the presence of zero-variance features (returns 0.0)
//! - `Into<Vec<f64>>` produces values in the same order as the feature names
//!
//! ## NOT Responsible For
//! - Model training or inference
//! - Persistence beyond `Vec<f64>` serialization

use crate::error::FinError;

/// Normalization strategy applied to raw feature values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Normalization {
    /// No normalization; raw values are used as-is.
    None,
    /// Z-score: `(x - mean) / std_dev`. Zero-variance features become `0.0`.
    ZScore,
    /// Min-max: `(x - min) / (max - min)`. Constant features become `0.0`.
    MinMax,
}

/// A named feature snapshot at a single point in time.
///
/// Constructed via [`FeatureVectorBuilder`] then finalized into [`FeatureVector`].
#[derive(Debug, Clone)]
pub struct FeatureVector {
    /// Ordered feature names.
    names: Vec<String>,
    /// Raw (un-normalized) feature values, parallel to `names`.
    raw: Vec<f64>,
    /// Normalization mode applied when converting to `Vec<f64>`.
    normalization: Normalization,
}

impl FeatureVector {
    /// Returns the number of features.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns `true` if no features have been added.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Returns a slice of feature names in insertion order.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Returns raw (un-normalized) values in insertion order.
    pub fn raw_values(&self) -> &[f64] {
        &self.raw
    }

    /// Returns the normalization mode.
    pub fn normalization(&self) -> Normalization {
        self.normalization
    }

    /// Returns the raw value for a feature by name, or `None` if not found.
    pub fn get_by_name(&self, name: &str) -> Option<f64> {
        self.names.iter().position(|n| n == name).map(|i| self.raw[i])
    }

    /// Normalizes raw values according to the configured [`Normalization`] mode
    /// and returns them as a `Vec<f64>`.
    ///
    /// - [`Normalization::None`]: returns raw values unchanged.
    /// - [`Normalization::ZScore`]: subtracts mean and divides by std dev.
    ///   Zero-variance features → `0.0`.
    /// - [`Normalization::MinMax`]: scales to `[0, 1]`.
    ///   Constant features → `0.0`.
    pub fn to_normalized_vec(&self) -> Vec<f64> {
        match self.normalization {
            Normalization::None => self.raw.clone(),
            Normalization::ZScore => zscore_normalize(&self.raw),
            Normalization::MinMax => minmax_normalize(&self.raw),
        }
    }
}

/// Implements `Into<Vec<f64>>` by applying normalization.
impl From<FeatureVector> for Vec<f64> {
    fn from(fv: FeatureVector) -> Vec<f64> {
        fv.to_normalized_vec()
    }
}

/// Implements construction from a fixed-size array reference.
///
/// Creates a `FeatureVector` with auto-generated names `"f0"`, `"f1"`, … and
/// [`Normalization::None`].
impl<const N: usize> From<&[f64; N]> for FeatureVector {
    fn from(arr: &[f64; N]) -> Self {
        let names: Vec<String> = (0..N).map(|i| format!("f{i}")).collect();
        let raw: Vec<f64> = arr.iter().copied().collect();
        Self { names, raw, normalization: Normalization::None }
    }
}

/// Builder for [`FeatureVector`].
///
/// # Example
/// ```rust
/// use fin_primitives::ml::{FeatureVectorBuilder, Normalization};
///
/// let fv = FeatureVectorBuilder::new(Normalization::ZScore)
///     .add("rsi", 55.3).unwrap()
///     .add("adx", 28.1).unwrap()
///     .add("hurst", 0.6).unwrap()
///     .build();
///
/// assert_eq!(fv.len(), 3);
/// let normalized: Vec<f64> = fv.into();
/// assert_eq!(normalized.len(), 3);
/// ```
#[derive(Debug, Default)]
pub struct FeatureVectorBuilder {
    names: Vec<String>,
    raw: Vec<f64>,
    normalization: Normalization,
}

impl Default for Normalization {
    fn default() -> Self {
        Normalization::None
    }
}

impl FeatureVectorBuilder {
    /// Creates a new builder with the specified normalization mode.
    pub fn new(normalization: Normalization) -> Self {
        Self { names: Vec::new(), raw: Vec::new(), normalization }
    }

    /// Adds a named feature with a raw `f64` value.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if:
    /// - `name` is empty or contains only whitespace.
    /// - A feature with the same name already exists.
    pub fn add(mut self, name: impl Into<String>, value: f64) -> Result<Self, FinError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(FinError::InvalidInput(
                "feature name must not be empty or whitespace".to_owned(),
            ));
        }
        if self.names.iter().any(|n| n == &name) {
            return Err(FinError::InvalidInput(format!(
                "duplicate feature name: '{name}'"
            )));
        }
        self.names.push(name);
        self.raw.push(value);
        Ok(self)
    }

    /// Adds multiple features from a slice of `(name, value)` pairs.
    ///
    /// # Errors
    /// Propagates errors from [`Self::add`].
    pub fn add_all(
        mut self,
        features: &[(&str, f64)],
    ) -> Result<Self, FinError> {
        for (name, value) in features {
            self = self.add(*name, *value)?;
        }
        Ok(self)
    }

    /// Finalizes the builder and returns a [`FeatureVector`].
    pub fn build(self) -> FeatureVector {
        FeatureVector {
            names: self.names,
            raw: self.raw,
            normalization: self.normalization,
        }
    }
}

// ---------- normalization helpers ----------

fn zscore_normalize(values: &[f64]) -> Vec<f64> {
    let n = values.len() as f64;
    if n == 0.0 {
        return Vec::new();
    }
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 {
        return vec![0.0; values.len()];
    }
    values.iter().map(|v| (v - mean) / std_dev).collect()
}

fn minmax_normalize(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    if range == 0.0 {
        return vec![0.0; values.len()];
    }
    values.iter().map(|v| (v - min) / range).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let fv = FeatureVectorBuilder::new(Normalization::None)
            .add("rsi", 60.0)
            .unwrap()
            .add("adx", 30.0)
            .unwrap()
            .build();
        assert_eq!(fv.len(), 2);
        assert_eq!(fv.names(), &["rsi", "adx"]);
        assert_eq!(fv.raw_values(), &[60.0, 30.0]);
    }

    #[test]
    fn test_duplicate_name_rejected() {
        let result = FeatureVectorBuilder::new(Normalization::None)
            .add("rsi", 60.0)
            .unwrap()
            .add("rsi", 70.0);
        assert!(matches!(result, Err(FinError::InvalidInput(_))));
    }

    #[test]
    fn test_empty_name_rejected() {
        let result = FeatureVectorBuilder::new(Normalization::None).add("  ", 1.0);
        assert!(matches!(result, Err(FinError::InvalidInput(_))));
    }

    #[test]
    fn test_zscore_normalization() {
        let fv = FeatureVectorBuilder::new(Normalization::ZScore)
            .add("a", 10.0)
            .unwrap()
            .add("b", 20.0)
            .unwrap()
            .add("c", 30.0)
            .unwrap()
            .build();
        let norm: Vec<f64> = fv.into();
        // mean=20, std=8.165, so z(-10/8.165, 0, +10/8.165)
        assert!((norm[1]).abs() < 1e-10, "middle value should be 0 after z-score");
        assert!(norm[0] < 0.0);
        assert!(norm[2] > 0.0);
    }

    #[test]
    fn test_minmax_normalization() {
        let fv = FeatureVectorBuilder::new(Normalization::MinMax)
            .add("a", 0.0)
            .unwrap()
            .add("b", 50.0)
            .unwrap()
            .add("c", 100.0)
            .unwrap()
            .build();
        let norm: Vec<f64> = fv.into();
        assert!((norm[0] - 0.0).abs() < 1e-10);
        assert!((norm[1] - 0.5).abs() < 1e-10);
        assert!((norm[2] - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_zero_variance_zscore() {
        let fv = FeatureVectorBuilder::new(Normalization::ZScore)
            .add("a", 5.0)
            .unwrap()
            .add("b", 5.0)
            .unwrap()
            .build();
        let norm: Vec<f64> = fv.into();
        assert_eq!(norm, vec![0.0, 0.0]);
    }

    #[test]
    fn test_zero_variance_minmax() {
        let fv = FeatureVectorBuilder::new(Normalization::MinMax)
            .add("a", 7.0)
            .unwrap()
            .add("b", 7.0)
            .unwrap()
            .build();
        let norm: Vec<f64> = fv.into();
        assert_eq!(norm, vec![0.0, 0.0]);
    }

    #[test]
    fn test_from_fixed_array() {
        let arr = [1.0_f64, 2.0, 3.0];
        let fv = FeatureVector::from(&arr);
        assert_eq!(fv.len(), 3);
        assert_eq!(fv.names(), &["f0", "f1", "f2"]);
        assert_eq!(fv.raw_values(), &[1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_get_by_name() {
        let fv = FeatureVectorBuilder::new(Normalization::None)
            .add("vol", 0.25)
            .unwrap()
            .build();
        assert_eq!(fv.get_by_name("vol"), Some(0.25));
        assert_eq!(fv.get_by_name("missing"), None);
    }

    #[test]
    fn test_add_all() {
        let features = [("x", 1.0), ("y", 2.0), ("z", 3.0)];
        let fv = FeatureVectorBuilder::new(Normalization::None)
            .add_all(&features)
            .unwrap()
            .build();
        assert_eq!(fv.len(), 3);
    }

    #[test]
    fn test_into_vec_none_normalization() {
        let fv = FeatureVectorBuilder::new(Normalization::None)
            .add("a", 42.0)
            .unwrap()
            .build();
        let v: Vec<f64> = fv.into();
        assert_eq!(v, vec![42.0]);
    }
}
