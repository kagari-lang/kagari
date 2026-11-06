//! Short-lived normal-completion queries against one admitted immutable body.

use crate::{
    compile_input::body::CheckedBodyInput,
    hir::{
        expr::ExprKind,
        ids::{ExprId, PlaceId, StmtId},
    },
    typeck::completion::CompletionCache,
};
use kagari_common::cancellation::{CancellationToken, Cancelled};

/// A reusable view of whether evaluation can produce a value or a location.
///
/// In `(loop {})?`, the operand cannot return, so no propagation operation runs
/// and no selected `Try` contract is required for it. Completion is distinct
/// from a stored type: preparing a place can complete even when its value type
/// is uninhabited. These queries use the same work-stack algorithm as checking.
///
/// The memo belongs only to this immutable input and is dropped after elaboration.
/// Cancellation returns an error even for a cached query. No solver state, mutable
/// semantic facts or cross-revision cache is retained or exposed.
pub struct BodyCompletion<'a> {
    input: CheckedBodyInput<'a>,
    cache: CompletionCache<'a>,
}

impl<'a> CheckedBodyInput<'a> {
    /// Creates completion queries tied to this exact checked source body.
    pub fn completion(self) -> BodyCompletion<'a> {
        BodyCompletion {
            input: self,
            cache: CompletionCache::new(self.nodes, self.names, self.types),
        }
    }
}

impl BodyCompletion<'_> {
    /// Returns whether matching can continue past an arm, assuming that arm is reached.
    ///
    /// For `match x { _ if { return 1; } => 0, _ => 2 }`, arm zero has no
    /// continuation. A refutable pattern may fail, and a normally completing
    /// guard may be false. The query does not evaluate boolean constants.
    /// Foreign IDs, non-match expressions and missing arm positions return `None`;
    /// cancellation returns an error, including for cached results.
    pub fn match_arm_can_continue(
        &mut self,
        expr: ExprId,
        index: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<bool>, Cancelled> {
        cancel.check()?;
        let Some(ExprKind::Match { arms, .. }) = self
            .input
            .expression(expr)
            .map(|expression| &expression.kind)
        else {
            return Ok(None);
        };
        let Some(arm) = arms.get(index) else {
            return Ok(None);
        };
        self.cache.match_arm_can_continue(arm, cancel).map(Some)
    }

    /// Returns whether an expression has a normal exit, or `None` for a foreign ID.
    pub fn expression(
        &mut self,
        id: ExprId,
        cancel: &CancellationToken,
    ) -> Result<Option<bool>, Cancelled> {
        if self.input.expression(id).is_none() {
            cancel.check()?;
            return Ok(None);
        }
        self.cache.expression(id, cancel).map(Some)
    }

    /// Returns whether a statement can fall through to the next statement.
    pub fn statement(
        &mut self,
        id: StmtId,
        cancel: &CancellationToken,
    ) -> Result<Option<bool>, Cancelled> {
        if self.input.statement(id).is_none() {
            cancel.check()?;
            return Ok(None);
        }
        self.cache.statement(id, cancel).map(Some)
    }

    /// Returns whether computing a mutation location can finish; it does not read that location.
    pub fn place(
        &mut self,
        id: PlaceId,
        cancel: &CancellationToken,
    ) -> Result<Option<bool>, Cancelled> {
        if self.input.place(id).is_none() {
            cancel.check()?;
            return Ok(None);
        }
        self.cache.place(id, cancel).map(Some)
    }
}
