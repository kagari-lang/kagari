//! Call signatures are projections of the selected declaration's checked facts.

use crate::{
    analysis::FileAnalysis,
    declarations::DeclarationId,
    hir::ExprKind,
    host,
    resolver::ResolvedName,
    typeck::{CallTarget, TypedFunction},
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
        let source_signature = |declaration, signature: &TypedFunction| {
            let substitution: TypeSubstitution = signature
                .generic_params
                .iter()
                .cloned()
                .zip(call.type_arguments.iter().cloned())
                .collect();
            CallSignature {
                declaration,
                parameters: signature
                    .params
                    .iter()
                    .skip(receiver_offset)
                    .map(|parameter| {
                        (
                            parameter.name.clone(),
                            facts
                                .aggregates
                                .normalize_type(&parameter.ty.instantiate(&substitution)),
                        )
                    })
                    .collect(),
                result: facts
                    .aggregates
                    .normalize_type(&signature.return_type.instantiate(&substitution)),
            }
        };
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
                Some(source_signature(declaration.id.clone(), signature))
            }
            CallTarget::SourceFunction(function) => {
                let imported = facts.imported_functions.target(*function)?;
                Some(source_signature(
                    imported.site.id.clone(),
                    &imported.signature,
                ))
            }
            CallTarget::HostFunction(function) => {
                let declaration = facts.names.hosts.function(*function)?;
                Some(CallSignature {
                    declaration: DeclarationId::Definition(declaration.id.clone()),
                    parameters: declaration
                        .params
                        .iter()
                        .skip(receiver_offset)
                        .map(|parameter| {
                            (parameter.name.clone(), host::signature_type(&parameter.ty))
                        })
                        .collect(),
                    result: host::signature_type(&declaration.return_type),
                })
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
