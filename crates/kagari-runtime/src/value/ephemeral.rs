//! Inert Runtime temporary identity, without managed-reference or Host authority.

/// An opaque Runtime temporary. Copying this identity neither retains a script
/// object nor grants a Rust loan. Such values cannot enter persistent payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EphemeralValueId(pub u64);
