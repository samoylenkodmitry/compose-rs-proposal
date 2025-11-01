#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{HashMap, HashSet};
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use hashbrown::{HashMap, HashSet};
}