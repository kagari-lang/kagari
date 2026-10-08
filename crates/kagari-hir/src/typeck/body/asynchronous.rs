//! Await checks the current callable context and preserves its completed output.

use crate::{
    hir::ids::ExprId,
    typeck::{BodyTypeEnv, asynchronous, body::BodyChecker, ty::display_type_id},
    types::TypeId,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

impl BodyChecker<'_> {
    pub(super) fn infer_await(
        &mut self,
        site: ExprId,
        operand: ExprId,
        env: &mut BodyTypeEnv,
        _expected: Option<&TypeId>,
    ) -> TypeId {
        if !self.async_body {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::AwaitOutsideAsync)
                    .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        // The outer expectation constrains the completed output; imposing
        // Future<T> on the operand would incorrectly reject a Task<T>.
        let operand_type = self.infer_expr_type_expected(operand, env, None);
        if operand_type.is_never() {
            return operand_type;
        }
        if let Some(output) = asynchronous::await_output(self.declarations, &operand_type) {
            return output.clone();
        }
        if !operand_type.is_unresolved() {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidAwaitOperand {
                    type_name: display_type_id(&operand_type),
                })
                .with_span(self.lowered.source_map.expr_span(operand)),
            );
        }
        TypeId::Error
    }
}
