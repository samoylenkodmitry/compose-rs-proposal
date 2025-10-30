//! Asset loading and management primitives for Compose-RS.

/// Placeholder asset manager.
pub struct AssetManager;

impl AssetManager {
    /// Creates a new placeholder asset manager.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}
