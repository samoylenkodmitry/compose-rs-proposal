use core::hash::Hash;
use std::hash::Hasher;

#[cfg(feature = "std-hash")]
pub mod default {
    pub use std::collections::hash_map::DefaultHasher;

    #[inline]
    pub fn new() -> DefaultHasher {
        DefaultHasher::new()
    }
}

#[cfg(not(feature = "std-hash"))]
pub mod default {
    pub use ahash::AHasher as DefaultHasher;

    #[inline]
    pub fn new() -> DefaultHasher {
        DefaultHasher::default()
    }
}

/// convenience: hash a single value with whichever default is active
#[inline]
pub fn hash_one<T: Hash>(v: &T) -> u64 {
    #[allow(unused_imports)]
    use crate::hash::default;
    let mut h = default::new();
    v.hash(&mut h);
    h.finish()
}