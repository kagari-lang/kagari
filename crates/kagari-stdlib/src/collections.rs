//! Library-owned native collections and retained host handles.
#[cfg(feature = "execution-diagnostics")]
pub mod diagnostics;
pub(crate) mod hash;
pub mod map;
pub mod set;
pub mod vector;
