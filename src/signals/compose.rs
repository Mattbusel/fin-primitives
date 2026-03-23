//! # Signal Composition Engine
//!
//! A composable, expression-tree DSL for building derived signals from existing
//! indicators. Instead of writing bespoke structs for every derived computation,
//! callers use [`SignalExpr`] to describe *what* to compute and [`ComposedSignal`]
//! to evaluate that expression on each new bar.
//!
//! ## Architecture
//!
//! ```text
//! SignalExpr (description)
//!     └── ComposedSignal (stateful evaluator)
//!             └── inner Signal impls (Sma, Ema, Rsi, …)
//! ```
//!
//! [`SignalExpr`] is a pure data structure — it describes the computation graph
//! without owning any mutable indicator state. [`ComposedSignal`] is the stateful
//! counterpart that actually holds the inner indicators and evaluates the tree.
//!
//! ## Warmup Semantics
//!
//! Composed signals correctly propagate warmup: if any leaf signal in the expression
//! tree is not yet ready, the composed signal returns `SignalValue::Unavailable`.
//! The warmup period of a composition is the *maximum* warmup period of all leaves,
//! plus any additional lag introduced by [`SignalExpr::Lag`] nodes.
//!
//! ## Builder API
//!
//! Use [`SignalBuilder`] for a fluent, method-chaining API. The builder works on
//! any concrete `Signal` type and produces a [`ComposedSignal`] ready for use.
//!
//! ## Example
//!
//! ```rust
//! use fin_primitives::signals::indicators::Rsi;
//! use fin_primitives::signals::{BarInput, Signal};
//! use fin_primitives::signals::compose::{SignalBuilder, NormMethod, Direction};
//! use rust_decimal_macros::dec;
//!
//! let rsi = Rsi::new("rsi14", 14).unwrap();
//!
//! // Build: RSI(14) → lag(1) → normalize(ZScore) → threshold(2.0, Above)
//! let mut composed = SignalBuilder::new(rsi)
//!     .lag(1)
//!     .normalize(NormMethod::ZScore)
//!     .threshold(dec!(2), Direction::Above)
//!     .build();
//!
//! let bar = BarInput::from_close(dec!(100));
//! let _value = composed.update(&bar); // Ok(Unavailable) during warmup
//! ```

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

// ── NormMethod ────────────────────────────────────────────────────────────────

/// Method used to normalise a signal's output stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormMethod {
    /// Scales the value to `[0, 1]` using the rolling min/max over the last `window` bars.
    ///
    /// `output = (value - min) / (max - min)`
    MinMax,

    /// Standardises to zero mean and unit variance over the last `window` bars.
    ///
    /// `output = (value - mean) / std_dev`
    ZScore,

    /// Expresses the value as its percentile rank within the last `window` bars.
    ///
    /// `output ∈ [0, 1]` where `1.0` = largest value in the window.
    Percentile,
}

// ── Direction ─────────────────────────────────────────────────────────────────

/// Direction of a threshold test applied to a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Passes `+1` when the signal is strictly above the threshold, `0` otherwise.
    Above,
    /// Passes `-1` when the signal is strictly below the threshold, `0` otherwise.
    Below,
    /// Passes `+1` on an upward cross, `-1` on a downward cross, `0` otherwise.
    Cross,
}

// ── SignalKind ────────────────────────────────────────────────────────────────

/// Identifies a named leaf signal within a composed expression.
///
/// In a [`ComposedSignal`], every `Raw` leaf in the expression tree corresponds
/// to an entry in the signal registry keyed by `name`.
#[derive(Debug, Clone)]
pub struct SignalKind {
    /// The name of the signal as returned by [`Signal::name`].
    pub name: String,
}

impl SignalKind {
    /// Creates a new `SignalKind` referencing a signal by name.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

// ── SignalExpr ────────────────────────────────────────────────────────────────

/// A composable expression tree for building derived signals.
///
/// `SignalExpr` is a pure description of a computation — it does not hold any
/// mutable state. Feed it into [`ComposedSignal`] to evaluate it on bar data.
///
/// # Expression Nodes
///
/// | Variant | Description |
/// |---------|-------------|
/// | `Raw` | Leaf: the raw output of a named indicator |
/// | `Add` | Element-wise addition of two sub-expressions |
/// | `Sub` | Element-wise subtraction |
/// | `Mul` | Multiply a sub-expression by a scalar constant |
/// | `Lag` | Delay a sub-expression by `n` bars |
/// | `Normalize` | Normalise a sub-expression using a rolling window |
/// | `Threshold` | Convert a scalar sub-expression to `+1`, `0`, or `-1` |
#[derive(Debug, Clone)]
pub enum SignalExpr {
    /// A raw indicator output, identified by name.
    Raw(SignalKind),

    /// The sum of two sub-expressions. Returns `Unavailable` if either is.
    Add(Box<SignalExpr>, Box<SignalExpr>),

    /// The difference of two sub-expressions (`left - right`). Returns `Unavailable` if either is.
    Sub(Box<SignalExpr>, Box<SignalExpr>),

    /// A sub-expression scaled by a constant factor.
    Mul(Box<SignalExpr>, Decimal),

    /// A sub-expression delayed by `n` bars.
    ///
    /// Returns `Unavailable` until the buffer has accumulated `n` bars of ready values.
    Lag(Box<SignalExpr>, usize),

    /// A sub-expression normalised using a rolling window of `window` bars.
    ///
    /// Returns `Unavailable` until `window` ready values have been accumulated.
    Normalize(Box<SignalExpr>, NormMethod, usize),

    /// Converts a sub-expression to a directional signal relative to a threshold.
    ///
    /// Output is `Scalar(1)`, `Scalar(-1)`, or `Scalar(0)`.
    Threshold(Box<SignalExpr>, Decimal, Direction),
}

impl SignalExpr {
    /// Constructs a `Raw` leaf from a signal name.
    pub fn raw(name: impl Into<String>) -> Self {
        Self::Raw(SignalKind::new(name))
    }

    /// Wraps `self` in an `Add` with `rhs`.
    pub fn add(self, rhs: SignalExpr) -> Self {
        Self::Add(Box::new(self), Box::new(rhs))
    }

    /// Wraps `self` in a `Sub` with `rhs`.
    pub fn sub(self, rhs: SignalExpr) -> Self {
        Self::Sub(Box::new(self), Box::new(rhs))
    }

    /// Wraps `self` in a `Mul` with scalar `factor`.
    pub fn mul(self, factor: Decimal) -> Self {
        Self::Mul(Box::new(self), factor)
    }

    /// Wraps `self` in a `Lag` of `n` bars.
    pub fn lag(self, n: usize) -> Self {
        Self::Lag(Box::new(self), n)
    }

    /// Wraps `self` in a `Normalize` node.
    pub fn normalize(self, method: NormMethod, window: usize) -> Self {
        Self::Normalize(Box::new(self), method, window)
    }

    /// Wraps `self` in a `Threshold` node.
    pub fn threshold(self, level: Decimal, direction: Direction) -> Self {
        Self::Threshold(Box::new(self), level, direction)
    }

    /// Returns a flat list of all leaf signal names referenced by this expression.
    pub fn leaf_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        self.collect_leaf_names(&mut names);
        names
    }

    fn collect_leaf_names<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Raw(kind) => out.push(&kind.name),
            Self::Add(l, r) | Self::Sub(l, r) => {
                l.collect_leaf_names(out);
                r.collect_leaf_names(out);
            }
            Self::Mul(inner, _)
            | Self::Lag(inner, _)
            | Self::Normalize(inner, _, _)
            | Self::Threshold(inner, _, _) => inner.collect_leaf_names(out),
        }
    }
}

// ── ExprState ─────────────────────────────────────────────────────────────────

/// Internal mutable state for a single node in the expression tree.
///
/// The state tree mirrors the `SignalExpr` tree 1:1 so that each stateful node
/// (Lag buffer, Normalize rolling window) can be updated independently.
enum ExprState {
    Raw,
    Add(Box<ExprState>, Box<ExprState>),
    Sub(Box<ExprState>, Box<ExprState>),
    Mul(Box<ExprState>),
    Lag {
        inner: Box<ExprState>,
        buffer: VecDeque<SignalValue>,
        n: usize,
    },
    Normalize {
        inner: Box<ExprState>,
        window: VecDeque<Decimal>,
        window_size: usize,
        method: NormMethod,
        prev: SignalValue,
    },
    Threshold {
        inner: Box<ExprState>,
        level: Decimal,
        direction: Direction,
        prev: SignalValue,
    },
}

impl ExprState {
    /// Builds an `ExprState` tree from a `SignalExpr` tree.
    fn from_expr(expr: &SignalExpr) -> Self {
        match expr {
            SignalExpr::Raw(_) => Self::Raw,
            SignalExpr::Add(l, r) => {
                Self::Add(Box::new(Self::from_expr(l)), Box::new(Self::from_expr(r)))
            }
            SignalExpr::Sub(l, r) => {
                Self::Sub(Box::new(Self::from_expr(l)), Box::new(Self::from_expr(r)))
            }
            SignalExpr::Mul(inner, _) => Self::Mul(Box::new(Self::from_expr(inner))),
            SignalExpr::Lag(inner, n) => Self::Lag {
                inner: Box::new(Self::from_expr(inner)),
                buffer: VecDeque::new(),
                n: *n,
            },
            SignalExpr::Normalize(inner, method, window_size) => Self::Normalize {
                inner: Box::new(Self::from_expr(inner)),
                window: VecDeque::new(),
                window_size: *window_size,
                method: *method,
                prev: SignalValue::Unavailable,
            },
            SignalExpr::Threshold(inner, _, direction) => Self::Threshold {
                inner: Box::new(Self::from_expr(inner)),
                level: Decimal::ZERO, // overwritten during eval
                direction: *direction,
                prev: SignalValue::Unavailable,
            },
        }
    }

    /// Evaluates this state node given the raw signal values for all leaves.
    fn eval(
        &mut self,
        expr: &SignalExpr,
        leaf_values: &std::collections::HashMap<String, SignalValue>,
    ) -> SignalValue {
        match (self, expr) {
            (Self::Raw, SignalExpr::Raw(kind)) => leaf_values
                .get(&kind.name)
                .cloned()
                .unwrap_or(SignalValue::Unavailable),

            (Self::Add(ls, rs), SignalExpr::Add(le, re)) => {
                let l = ls.eval(le, leaf_values);
                let r = rs.eval(re, leaf_values);
                l.add(r)
            }

            (Self::Sub(ls, rs), SignalExpr::Sub(le, re)) => {
                let l = ls.eval(le, leaf_values);
                let r = rs.eval(re, leaf_values);
                l.sub(r)
            }

            (Self::Mul(inner_state), SignalExpr::Mul(inner_expr, factor)) => {
                let v = inner_state.eval(inner_expr, leaf_values);
                v.mul(*factor)
            }

            (
                Self::Lag { inner, buffer, n },
                SignalExpr::Lag(inner_expr, _),
            ) => {
                let v = inner.eval(inner_expr, leaf_values);
                if *n == 0 {
                    return v;
                }
                // Push the current value into the lag buffer.
                buffer.push_back(v);
                // Return the value that was at position `n` in the past.
                if buffer.len() > *n {
                    buffer.pop_front().unwrap_or(SignalValue::Unavailable)
                } else {
                    SignalValue::Unavailable
                }
            }

            (
                Self::Normalize { inner, window, window_size, method, .. },
                SignalExpr::Normalize(inner_expr, _, _),
            ) => {
                let v = inner.eval(inner_expr, leaf_values);
                match v {
                    SignalValue::Unavailable => SignalValue::Unavailable,
                    SignalValue::Scalar(d) => {
                        window.push_back(d);
                        if window.len() > *window_size {
                            window.pop_front();
                        }
                        if window.len() < *window_size {
                            return SignalValue::Unavailable;
                        }
                        compute_norm(window, *method, d)
                    }
                }
            }

            (
                Self::Threshold { inner, prev, direction, level },
                SignalExpr::Threshold(inner_expr, threshold_level, _),
            ) => {
                *level = *threshold_level;
                let v = inner.eval(inner_expr, leaf_values);
                let result = match direction {
                    Direction::Above => match &v {
                        SignalValue::Scalar(curr) if *curr > *level => {
                            SignalValue::Scalar(Decimal::ONE)
                        }
                        SignalValue::Scalar(_) => SignalValue::Scalar(Decimal::ZERO),
                        SignalValue::Unavailable => SignalValue::Unavailable,
                    },
                    Direction::Below => match &v {
                        SignalValue::Scalar(curr) if *curr < *level => {
                            SignalValue::Scalar(-Decimal::ONE)
                        }
                        SignalValue::Scalar(_) => SignalValue::Scalar(Decimal::ZERO),
                        SignalValue::Unavailable => SignalValue::Unavailable,
                    },
                    Direction::Cross => {
                        let result = match (&v, &*prev) {
                            (SignalValue::Scalar(curr), SignalValue::Scalar(p)) => {
                                if *curr > *level && *p <= *level {
                                    SignalValue::Scalar(Decimal::ONE)
                                } else if *curr < *level && *p >= *level {
                                    SignalValue::Scalar(-Decimal::ONE)
                                } else {
                                    SignalValue::Scalar(Decimal::ZERO)
                                }
                            }
                            _ => SignalValue::Unavailable,
                        };
                        result
                    }
                };
                *prev = v;
                result
            }

            // Mismatched arms should never occur if ExprState::from_expr is consistent.
            _ => SignalValue::Unavailable,
        }
    }

    /// Resets all buffered state recursively.
    fn reset(&mut self) {
        match self {
            Self::Raw | Self::Mul(_) => {}
            Self::Add(l, r) | Self::Sub(l, r) => {
                l.reset();
                r.reset();
            }
            Self::Lag { inner, buffer, .. } => {
                inner.reset();
                buffer.clear();
            }
            Self::Normalize { inner, window, prev, .. } => {
                inner.reset();
                window.clear();
                *prev = SignalValue::Unavailable;
            }
            Self::Threshold { inner, prev, .. } => {
                inner.reset();
                *prev = SignalValue::Unavailable;
            }
        }
    }
}

/// Computes a normalised value from a rolling window.
fn compute_norm(
    window: &VecDeque<Decimal>,
    method: NormMethod,
    current: Decimal,
) -> SignalValue {
    if window.is_empty() {
        return SignalValue::Unavailable;
    }
    match method {
        NormMethod::MinMax => {
            let min = window.iter().copied().fold(current, Decimal::min);
            let max = window.iter().copied().fold(current, Decimal::max);
            let range = max - min;
            if range.is_zero() {
                SignalValue::Scalar(Decimal::ZERO)
            } else {
                match (current - min).checked_div(range) {
                    Some(v) => SignalValue::Scalar(v),
                    None => SignalValue::Unavailable,
                }
            }
        }
        NormMethod::ZScore => {
            let n = window.len() as f64;
            if n < 2.0 {
                return SignalValue::Unavailable;
            }
            use rust_decimal::prelude::ToPrimitive;
            let mean: f64 = window.iter().filter_map(|v| v.to_f64()).sum::<f64>() / n;
            let variance: f64 = window
                .iter()
                .filter_map(|v| v.to_f64())
                .map(|v| (v - mean).powi(2))
                .sum::<f64>()
                / (n - 1.0);
            let std_dev = variance.sqrt();
            if std_dev == 0.0 {
                return SignalValue::Scalar(Decimal::ZERO);
            }
            let curr_f = current.to_f64().unwrap_or(mean);
            match Decimal::try_from((curr_f - mean) / std_dev) {
                Ok(z) => SignalValue::Scalar(z),
                Err(_) => SignalValue::Unavailable,
            }
        }
        NormMethod::Percentile => {
            let n = window.len();
            let count_below = window.iter().filter(|&&v| v < current).count();
            let count_equal = window.iter().filter(|&&v| v == current).count();
            // Percentile rank: (count_below + 0.5 * count_equal) / n
            let rank_f = (count_below as f64 + 0.5 * count_equal as f64) / n as f64;
            match Decimal::try_from(rank_f) {
                Ok(rank) => SignalValue::Scalar(rank),
                Err(_) => SignalValue::Unavailable,
            }
        }
    }
}

// ── ComposedSignal ────────────────────────────────────────────────────────────

/// Evaluates a [`SignalExpr`] expression tree on each new bar.
///
/// `ComposedSignal` holds a set of leaf [`Signal`] implementations and an
/// expression tree that describes how to combine them. On each call to
/// [`Signal::update`], it updates all leaves, then evaluates the expression
/// tree bottom-up to produce the final output.
///
/// The warmup period is the maximum leaf warmup period plus any lag introduced
/// by [`SignalExpr::Lag`] nodes at the outermost level.
///
/// # Construction
///
/// Use the [`SignalBuilder`] fluent API for the most ergonomic construction, or
/// build a [`SignalExpr`] tree manually and pass it to [`ComposedSignal::new`].
pub struct ComposedSignal {
    name: String,
    expr: SignalExpr,
    state: ExprState,
    leaves: Vec<Box<dyn Signal>>,
    bars_seen: usize,
}

impl ComposedSignal {
    /// Constructs a `ComposedSignal` from a name, expression tree, and leaf signals.
    ///
    /// The `leaves` vector must contain one signal per unique name referenced in the
    /// expression tree. Signals are looked up by name during evaluation.
    ///
    /// # Errors
    ///
    /// Returns [`FinError::InvalidInput`] if `leaves` is empty.
    pub fn new(
        name: impl Into<String>,
        expr: SignalExpr,
        leaves: Vec<Box<dyn Signal>>,
    ) -> Result<Self, FinError> {
        if leaves.is_empty() {
            return Err(FinError::InvalidInput(
                "ComposedSignal requires at least one leaf signal".into(),
            ));
        }
        let state = ExprState::from_expr(&expr);
        Ok(Self {
            name: name.into(),
            expr,
            state,
            leaves,
            bars_seen: 0,
        })
    }

    /// Returns the maximum warmup period across all leaf signals.
    pub fn leaf_warmup_period(&self) -> usize {
        self.leaves.iter().map(|s| s.period()).max().unwrap_or(0)
    }
}

impl Signal for ComposedSignal {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars_seen += 1;

        // Update all leaves and collect their values into a name-keyed map.
        let mut leaf_values = std::collections::HashMap::with_capacity(self.leaves.len());
        for leaf in &mut self.leaves {
            let val = leaf.update(bar)?;
            leaf_values.insert(leaf.name().to_owned(), val);
        }

        // Evaluate the expression tree using the collected leaf values.
        let result = self.state.eval(&self.expr, &leaf_values);
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.leaves.iter().all(|s| s.is_ready())
    }

    fn period(&self) -> usize {
        self.leaf_warmup_period()
    }

    fn reset(&mut self) {
        for leaf in &mut self.leaves {
            leaf.reset();
        }
        self.state.reset();
        self.bars_seen = 0;
    }
}

// ── SignalBuilder ─────────────────────────────────────────────────────────────

/// Fluent builder for [`ComposedSignal`] using method-chaining.
///
/// Start with any concrete signal type that implements [`Signal`] and chain
/// transformations. Each method wraps the accumulated expression in a new
/// [`SignalExpr`] node.
///
/// # Example
///
/// ```rust
/// use fin_primitives::signals::indicators::Sma;
/// use fin_primitives::signals::{BarInput, Signal};
/// use fin_primitives::signals::compose::{SignalBuilder, NormMethod, Direction};
/// use rust_decimal_macros::dec;
///
/// let sma = Sma::new("sma20", 20).unwrap();
/// let mut composed = SignalBuilder::new(sma)
///     .lag(2)
///     .normalize(NormMethod::MinMax)
///     .build();
///
/// let bar = BarInput::from_close(dec!(100));
/// let _ = composed.update(&bar);
/// ```
pub struct SignalBuilder<S: Signal + 'static> {
    signal: S,
    /// The accumulated expression tree (grows as builder methods are called).
    expr: SignalExpr,
    /// Window size used for `Normalize` nodes (default: 20).
    norm_window: usize,
}

impl<S: Signal + 'static> SignalBuilder<S> {
    /// Creates a builder from a concrete signal.
    ///
    /// The initial expression is `Raw(signal.name())`.
    pub fn new(signal: S) -> Self {
        let name = signal.name().to_owned();
        Self {
            signal,
            expr: SignalExpr::raw(name),
            norm_window: 20,
        }
    }

    /// Sets the rolling window size used by subsequent `normalize()` calls (default: 20).
    pub fn with_norm_window(mut self, window: usize) -> Self {
        self.norm_window = window;
        self
    }

    /// Wraps the current expression in a `Lag` of `n` bars.
    pub fn lag(mut self, n: usize) -> Self {
        self.expr = self.expr.lag(n);
        self
    }

    /// Wraps the current expression in a `Normalize` node using the configured window.
    pub fn normalize(mut self, method: NormMethod) -> Self {
        let window = self.norm_window;
        self.expr = self.expr.normalize(method, window);
        self
    }

    /// Wraps the current expression in a `Normalize` node with an explicit `window`.
    pub fn normalize_window(mut self, method: NormMethod, window: usize) -> Self {
        self.expr = self.expr.normalize(method, window);
        self
    }

    /// Wraps the current expression in a `Threshold` node.
    pub fn threshold(mut self, level: Decimal, direction: Direction) -> Self {
        self.expr = self.expr.threshold(level, direction);
        self
    }

    /// Scales the current expression by `factor`.
    pub fn scale(mut self, factor: Decimal) -> Self {
        self.expr = self.expr.mul(factor);
        self
    }

    /// Consumes the builder and produces a [`ComposedSignal`].
    ///
    /// The composed signal name is derived from the inner signal's name.
    ///
    /// # Panics
    ///
    /// Panics if signal construction fails (which cannot happen here since the
    /// leaf is already valid).
    pub fn build(self) -> ComposedSignal {
        let leaf_name = self.signal.name().to_owned();
        let composed_name = format!("composed({})", leaf_name);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(self.signal)];
        // Safety: leaves is non-empty by construction.
        ComposedSignal::new(composed_name, self.expr, leaves)
            .expect("ComposedSignal construction with a valid leaf signal cannot fail")
    }

    /// Consumes the builder and produces a [`ComposedSignal`] with an explicit name.
    pub fn build_named(self, name: impl Into<String>) -> ComposedSignal {
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(self.signal)];
        ComposedSignal::new(name, self.expr, leaves)
            .expect("ComposedSignal construction with a valid leaf signal cannot fail")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::indicators::{Ema, Rsi, Sma};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> BarInput {
        BarInput::from_close(close.parse().unwrap())
    }

    fn feed_n(signal: &mut impl Signal, close: &str, n: usize) {
        for _ in 0..n {
            signal.update(&bar(close)).unwrap();
        }
    }

    // ── SignalExpr leaf_names ────────────────────────────────────────────────

    #[test]
    fn test_expr_raw_leaf_name() {
        let expr = SignalExpr::raw("sma5");
        assert_eq!(expr.leaf_names(), vec!["sma5"]);
    }

    #[test]
    fn test_expr_add_leaf_names() {
        let expr = SignalExpr::raw("a").add(SignalExpr::raw("b"));
        let names = expr.leaf_names();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn test_expr_nested_leaf_names() {
        let expr = SignalExpr::raw("sma5")
            .lag(1)
            .normalize(NormMethod::ZScore, 20)
            .threshold(dec!(0), Direction::Above);
        assert_eq!(expr.leaf_names(), vec!["sma5"]);
    }

    // ── ComposedSignal: Raw passthrough ──────────────────────────────────────

    #[test]
    fn test_composed_raw_passthrough() {
        let sma = Sma::new("sma3", 3).unwrap();
        let expr = SignalExpr::raw("sma3");
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        feed_n(&mut composed, "10", 2);
        let v = composed.update(&bar("10")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_composed_raw_unavailable_during_warmup() {
        let sma = Sma::new("sma5", 5).unwrap();
        let expr = SignalExpr::raw("sma5");
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        let v = composed.update(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    // ── Mul ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_composed_mul_scales_value() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").mul(dec!(2));
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        let v = composed.update(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    // ── Add ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_composed_add_two_signals() {
        let sma1 = Sma::new("sma_a", 1).unwrap();
        let sma2 = Sma::new("sma_b", 1).unwrap();
        let expr = SignalExpr::raw("sma_a").add(SignalExpr::raw("sma_b"));
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma1), Box::new(sma2)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        let v = composed.update(&bar("15")).unwrap();
        // sma_a = 15, sma_b = 15, sum = 30
        assert_eq!(v, SignalValue::Scalar(dec!(30)));
    }

    // ── Lag ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_composed_lag_delays_values() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").lag(2);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        // bars 1,2: buffer fills but has not produced 2 prior values yet
        composed.update(&bar("10")).unwrap();
        composed.update(&bar("20")).unwrap();
        // bar 3: lag-2 should produce bar-1's value = 10
        let v = composed.update(&bar("30")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_composed_lag_zero_is_passthrough() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").lag(0);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        let v = composed.update(&bar("42")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(42)));
    }

    // ── Normalize: MinMax ────────────────────────────────────────────────────

    #[test]
    fn test_normalize_minmax_range_of_constant_returns_zero() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").normalize(NormMethod::MinMax, 3);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        // Feed 3 identical values → min==max → output 0
        composed.update(&bar("10")).unwrap();
        composed.update(&bar("10")).unwrap();
        let v = composed.update(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_normalize_minmax_high_value_approaches_one() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").normalize(NormMethod::MinMax, 3);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        // window: [0, 50, 100] → current=100, min=0, max=100 → 1.0
        composed.update(&bar("0")).unwrap();
        composed.update(&bar("50")).unwrap();
        let v = composed.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    // ── Normalize: ZScore ────────────────────────────────────────────────────

    #[test]
    fn test_normalize_zscore_mean_value_near_zero() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").normalize(NormMethod::ZScore, 5);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        // Feed 5 bars of [10, 10, 10, 10, 10] → z-score of 10 is 0
        for _ in 0..4 {
            composed.update(&bar("10")).unwrap();
        }
        let v = composed.update(&bar("10")).unwrap();
        if let SignalValue::Scalar(z) = v {
            assert!(z.abs() < dec!(0.001), "z-score of mean should be near 0, got {z}");
        } else {
            panic!("expected Scalar");
        }
    }

    // ── Normalize: Percentile ────────────────────────────────────────────────

    #[test]
    fn test_normalize_percentile_highest_value() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").normalize(NormMethod::Percentile, 4);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        composed.update(&bar("10")).unwrap();
        composed.update(&bar("20")).unwrap();
        composed.update(&bar("30")).unwrap();
        let v = composed.update(&bar("100")).unwrap(); // clearly the max
        if let SignalValue::Scalar(pct) = v {
            assert!(pct > dec!(0.5), "max value should have pct > 0.5, got {pct}");
        } else {
            panic!("expected Scalar");
        }
    }

    // ── Threshold ────────────────────────────────────────────────────────────

    #[test]
    fn test_threshold_above_emits_one_when_above() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").threshold(dec!(50), Direction::Above);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        let v = composed.update(&bar("75")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_threshold_above_emits_zero_when_below() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").threshold(dec!(50), Direction::Above);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        let v = composed.update(&bar("25")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_threshold_below_emits_neg_one_when_below() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").threshold(dec!(50), Direction::Below);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        let v = composed.update(&bar("20")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_threshold_cross_emits_one_on_upward_cross() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").threshold(dec!(50), Direction::Cross);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        composed.update(&bar("40")).unwrap(); // below threshold, prev = Unavailable
        composed.update(&bar("40")).unwrap(); // prev = 40 (below)
        let v = composed.update(&bar("60")).unwrap(); // crosses above
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_threshold_cross_emits_neg_one_on_downward_cross() {
        let sma = Sma::new("sma1", 1).unwrap();
        let expr = SignalExpr::raw("sma1").threshold(dec!(50), Direction::Cross);
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        composed.update(&bar("60")).unwrap(); // above
        composed.update(&bar("60")).unwrap(); // prev = 60 (above)
        let v = composed.update(&bar("40")).unwrap(); // crosses below
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    // ── SignalBuilder ────────────────────────────────────────────────────────

    #[test]
    fn test_builder_builds_composed_signal() {
        let sma = Sma::new("sma5", 5).unwrap();
        let mut composed = SignalBuilder::new(sma).lag(1).build();
        assert_eq!(composed.name(), "composed(sma5)");
        let v = composed.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable); // lag-1 not filled yet
    }

    #[test]
    fn test_builder_build_named() {
        let sma = Sma::new("sma5", 5).unwrap();
        let composed = SignalBuilder::new(sma).build_named("my_signal");
        assert_eq!(composed.name(), "my_signal");
    }

    #[test]
    fn test_builder_scale() {
        let sma = Sma::new("sma1", 1).unwrap();
        let mut composed = SignalBuilder::new(sma).scale(dec!(3)).build();
        let v = composed.update(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(30)));
    }

    #[test]
    fn test_builder_normalize_minmax() {
        let sma = Sma::new("sma1", 1).unwrap();
        let mut composed = SignalBuilder::new(sma)
            .normalize_window(NormMethod::MinMax, 3)
            .build();
        composed.update(&bar("0")).unwrap();
        composed.update(&bar("50")).unwrap();
        let v = composed.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_builder_threshold_above() {
        let sma = Sma::new("sma1", 1).unwrap();
        let mut composed = SignalBuilder::new(sma)
            .threshold(dec!(50), Direction::Above)
            .build();
        let v = composed.update(&bar("80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_builder_chain_lag_normalize_threshold() {
        let rsi = Rsi::new("rsi5", 5).unwrap();
        let mut composed = SignalBuilder::new(rsi)
            .lag(1)
            .normalize_window(NormMethod::ZScore, 10)
            .threshold(dec!(1), Direction::Above)
            .build();

        // Feed enough bars to warm up RSI + lag + z-score window
        for _ in 0..30 {
            composed.update(&bar("50")).unwrap();
        }
        // All warming should be done; the value is Scalar(0) since all bars are flat (z=0)
        let v = composed.update(&bar("50")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    // ── ComposedSignal reset ─────────────────────────────────────────────────

    #[test]
    fn test_composed_reset_restarts_warmup() {
        let sma = Sma::new("sma3", 3).unwrap();
        let expr = SignalExpr::raw("sma3");
        let leaves: Vec<Box<dyn Signal>> = vec![Box::new(sma)];
        let mut composed = ComposedSignal::new("test", expr, leaves).unwrap();

        feed_n(&mut composed, "10", 3);
        assert!(composed.is_ready());
        composed.reset();
        assert!(!composed.is_ready());
        let v = composed.update(&bar("10")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    // ── Period reporting ─────────────────────────────────────────────────────

    #[test]
    fn test_composed_period_reflects_max_leaf_period() {
        let ema = Ema::new("ema10", 10).unwrap();
        let composed = SignalBuilder::new(ema).build();
        assert_eq!(composed.period(), 10);
    }

    // ── Error cases ──────────────────────────────────────────────────────────

    #[test]
    fn test_composed_new_fails_with_empty_leaves() {
        let expr = SignalExpr::raw("nonexistent");
        let leaves: Vec<Box<dyn Signal>> = vec![];
        let result = ComposedSignal::new("test", expr, leaves);
        assert!(result.is_err());
    }
}
