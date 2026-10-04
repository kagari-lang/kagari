//! Call-site adaptation for ordinary shared script and native entries.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_common::span::Span;
use kagari_hir::{
    callable::AppliedCallSignature,
    declarations::DeclarationId,
    resolver::resolved::ResolvedName,
    typeck::table::CallTarget as CheckedCallTarget,
    types::{TypeId, semantic::lower_type},
};
use kagari_mir::instruction::CallTarget;
use kagari_types::callable::Signature;
use std::slice;

impl FunctionLowerer<'_, '_> {
    pub(super) fn shared_function_call(
        &mut self,
        target: &CheckedCallTarget,
        arguments: &[TypeId],
        signature: &AppliedCallSignature,
        span: Span,
    ) -> Result<CallTarget, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("shared call target");
        let declaration = match target {
            CheckedCallTarget::Function(id) => {
                let DeclarationId::Definition(declaration) = &self
                    .analyzed
                    .declarations
                    .target(ResolvedName::Function(*id))
                    .ok_or_else(invalid)?
                    .id
                else {
                    return Err(invalid());
                };
                declaration.clone()
            }
            CheckedCallTarget::SourceFunction(id) => self
                .analyzed
                .imported_functions
                .target(id)
                .ok_or_else(invalid)?
                .declaration
                .clone(),
            _ => return Err(invalid()),
        };
        let params =
            self.planner
                .arguments(&signature.params, &self.instance.substitution, span)?;
        let result = self
            .planner
            .arguments(
                slice::from_ref(&signature.return_type),
                &self.instance.substitution,
                span,
            )?
            .remove(0);
        let signature = Signature {
            params: params.iter().map(lower_type).collect(),
            result: lower_type(&result),
        };
        Ok(CallTarget::Shared(Box::new(self.planner.shared_call(
            &declaration,
            arguments,
            signature,
            span,
        )?)))
    }
}
