//! Call signatures are projections of the selected declaration's checked facts.

use crate::{
    aggregates::AggregateCatalog,
    analysis::FileAnalysis,
    callable::CallableSignature,
    declarations::DeclarationId,
    hir::ExprKind,
    resolver::ResolvedName,
    typeck::{CallTarget, ResolvedCall},
    types::{TypeId, TypeSubstitution},
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
                Some(callable_signature(
                    declaration.id.clone(),
                    signature,
                    &call,
                    &facts.aggregates,
                ))
            }
            CallTarget::SourceFunction(function) => {
                let imported = facts.imported_functions.target(*function)?;
                Some(callable_signature(
                    imported.site.id.clone(),
                    &imported.signature,
                    &call,
                    &facts.aggregates,
                ))
            }
            CallTarget::HostFunction(function) => {
                let callable = facts.names.hosts.callable(*function)?;
                Some(callable_signature(
                    DeclarationId::Definition(callable.contract().id.clone()),
                    &callable,
                    &call,
                    &facts.aggregates,
                ))
            }
            CallTarget::TraitMethod { method, interface } => {
                let method = facts.aggregates.trait_method(method)?;
                let owner = facts.aggregates.trait_(&method.owner)?;
                let mut substitution: TypeSubstitution = owner
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(interface.arguments.iter().cloned())
                    .collect();
                substitution.extend(
                    method
                        .generic_params
                        .iter()
                        .skip(owner.generic_params.len())
                        .cloned()
                        .zip(call.type_arguments.iter().cloned()),
                );
                let receiver = facts
                    .typed
                    .type_table
                    .protocol_receiver(id)
                    .cloned()
                    .or_else(|| {
                        call.receiver
                            .and_then(|receiver| facts.typed.type_table.expr_type(receiver))
                    })?;
                substitution.insert_receiver(owner.id.clone(), receiver);
                let instantiate = |ty: &TypeId| {
                    facts.aggregates.normalize_type(
                        &ty.instantiate(&substitution)
                            .with_associated_types(interface),
                    )
                };
                Some(CallSignature {
                    declaration: method.declaration.id.clone(),
                    parameters: method
                        .params
                        .iter()
                        .skip(receiver_offset)
                        .map(|parameter| (parameter.name.clone(), instantiate(&parameter.ty)))
                        .collect(),
                    result: instantiate(&method.return_type),
                })
            }
            _ => None,
        }
    }
}

fn callable_signature(
    declaration: DeclarationId,
    signature: &impl CallableSignature,
    call: &ResolvedCall,
    aggregates: &AggregateCatalog,
) -> CallSignature {
    let substitution: TypeSubstitution = signature
        .generic_params()
        .iter()
        .cloned()
        .zip(call.type_arguments.iter().cloned())
        .collect();
    CallSignature {
        declaration,
        parameters: signature
            .parameters()
            .skip(usize::from(call.receiver.is_some()))
            .map(|(name, ty)| {
                (
                    name.to_owned(),
                    aggregates.normalize_type(&ty.instantiate(&substitution)),
                )
            })
            .collect(),
        result: aggregates.normalize_type(&signature.return_type().instantiate(&substitution)),
    }
}
