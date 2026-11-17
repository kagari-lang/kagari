//! HIR hash-collection policy, shared by analysis and its public collection types.
//!
//! Fx hashing favors trusted compilation inputs; it is not HashDoS-resistant.
//! Keep the hasher choice here so a future policy change does not require rewriting
//! semantic collection owners. Iteration remains unordered: serialization, selection
//! and diagnostics must establish their own required order.

use rustc_hash::{FxHashMap, FxHashSet};

/// An unordered HIR lookup table using the current compiler hashing policy.
pub type HashMap<K, V> = FxHashMap<K, V>;

/// An unordered HIR membership set using the current compiler hashing policy.
pub type HashSet<T> = FxHashSet<T>;
