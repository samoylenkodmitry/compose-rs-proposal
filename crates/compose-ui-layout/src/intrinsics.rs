//! Intrinsic measurement APIs

/// Specifies how to size a component based on its intrinsic measurements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrinsicSize {
    /// Use the minimum intrinsic size of the content.
    Min,
    /// Use the maximum intrinsic size of the content.
    Max,
}
