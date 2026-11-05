//! Fixed-source contracts admitted for semantic elaboration and instance planning.
//!
//! [`program::CheckedCompilation`] separates declaration environments from body
//! inputs. Tooling uses recoverable analysis instead; no constructor here accepts
//! arbitrary analysis records or a caller-provided diagnostic list.
//!
//! Admission covers the complete checked dependency closure. Body lookup keeps
//! module, arena and owner together; a local slot is never a portable body key.

pub mod body;
pub mod environment;
pub mod program;
