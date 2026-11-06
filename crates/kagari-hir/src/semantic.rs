//! Retained semantic models shared by analysis and checked compilation inputs.
//!
//! These records describe selected contracts, not the mutable process that checks
//! them. Recovery can retain error types; compilation eligibility is a separate gate.

pub mod body;
pub mod signatures;
