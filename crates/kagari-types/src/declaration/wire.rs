//! File-local records for ABI codecs, separate from scoped semantic declarations.
//! Serialized records carry no type-check, registration or execution evidence.
pub mod declarations;
pub mod leaf;
pub mod path;
pub mod signature;

#[doc(hidden)]
pub mod native;
