//! Call signatures are projections of the selected declaration's checked facts.

use crate::{
    analysis::FileAnalysis,
    callable::{AppliedCallSignature, CallableSignature},
    declarations::DeclarationId,
    hir::ExprKind,
    resolver::ResolvedName,
    typeck::CallTarget,
    types::TypeId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSignature {
    pub declaration: DeclarationId,
    /// Receiver parameters are omitted for method syntax. Missing or invalid
    /// arguments do not replace the declared parameter types.
    pub parameters: Vec<(String, TypeId)>,
    pub result: TypeId,
}

impl FileAnalysis {
    /// The smallest enclosing call wins, including positions in its arguments.
    /// Source, engine-native and offline host declarations share this query.
    pub fn call_signature_at(&self, offset: usize) -> Option<CallSignature> {
        let facts = self.result.facts();
        let (_, id) = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expression)| {
                if !matches!(expression.kind, ExprKind::Call { .. }) {
                    return None;
                }
                let span = facts.lowered.source_map.expr_span(id);
                (span.start <= offset && offset < span.end).then_some((span.end - span.start, id))
            })
            .min_by_key(|(length, _)| *length)?;
        let call = facts.typed.type_table.call_resolution(id)?;
        let applied = call.signature.as_ref()?;
        let receiver_offset = usize::from(call.receiver.is_some());
        match &call.target {
            CallTarget::Function(function) => {
                let declaration = facts
                    .declarations
                    .target(ResolvedName::Function(*function))?;
                let signature = facts
                    .typed
                    .functions
                    .iter()
                    .find(|candidate| candidate.id == *function)?;
                callable_signature(
                    declaration.id.clone(),
                    signature.parameters().map(|(name, _)| name),
                    applied,
                    receiver_offset,
                )
            }
            CallTarget::SourceFunction(function) => {
                let imported = facts.imported_functions.target(*function)?;
                callable_signature(
                    imported.site.id.clone(),
                    imported.signature.parameters().map(|(name, _)| name),
                    applied,
                    receiver_offset,
                )
            }
            CallTarget::HostFunction(function) => {
                let callable = facts.names.hosts.callable(*function)?;
                callable_signature(
                    DeclarationId::Definition(callable.contract().id.clone()),
                    callable.parameters().map(|(name, _)| name),
                    applied,
                    receiver_offset,
                )
            }
            CallTarget::TraitMethod { method, .. } => {
                let method = facts.aggregates.trait_method(method)?;
                callable_signature(
                    method.declaration.id.clone(),
                    method
                        .params
                        .iter()
                        .map(|parameter| parameter.name.as_str()),
                    applied,
                    receiver_offset,
                )
            }
            _ => None,
        }
    }
}

fn callable_signature<'a>(
    declaration: DeclarationId,
    names: impl ExactSizeIterator<Item = &'a str>,
    applied: &AppliedCallSignature,
    receiver_offset: usize,
) -> Option<CallSignature> {
    if names.len() != applied.params.len() {
        return None;
    }
    Some(CallSignature {
        declaration,
        parameters: names
            .zip(&applied.params)
            .skip(receiver_offset)
            .map(|(name, ty)| (name.to_owned(), ty.clone()))
            .collect(),
        result: applied.return_type.clone(),
    })
}
