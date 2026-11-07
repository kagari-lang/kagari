//! Expand selected HIR facts into short-lived explicit operations before MIR building.
//!
//! Elaboration does no inference, name lookup or overload selection. Every input
//! comes from an admitted source body and its fixed declaration environment.
//! Missing required facts fail here; there is no fallback to the syntax lowerer.

pub(super) mod build;
pub mod model;

use crate::source::elaborate::{build::Builder, model::CheckedBody};
use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_hir::compile_input::body::CheckedBodyInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElaborationError {
    Cancelled,
    InvalidInput {
        span: Option<Span>,
        reason: &'static str,
    },
}

/// Elaborates one function/initializer/closure root, retaining source IDs and spans.
///
/// Nested callable roots retain their own keys and are expanded on demand. The
/// result owns its operations and contains no reference to mutable analysis state.
pub fn elaborate(
    input: CheckedBodyInput<'_>,
    cancel: &CancellationToken,
) -> Result<CheckedBody, ElaborationError> {
    Builder::new(input, cancel)?.finish()
}

#[cfg(test)]
mod tests;
