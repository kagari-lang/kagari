//! Admit applied dynamic trait values through the same catalog for every declaration source.

use crate::{
    DiagnosticBuffer,
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    typeck::GenericBounds,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::CancellationToken, identity::table::DefinitionTable, span::Span,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

pub(super) fn validate_type(
    definitions: &DefinitionTable,
    ty: &TypeId,
    catalog: &AggregateCatalog,
    span: Span,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return;
        }
        match ty {
            TypeId::Trait(instance) => {
                match catalog.interface_incompatibility(definitions, instance, cancel) {
                    Ok(Some(reason)) => diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                            trait_name: ty.display_name(definitions),
                            reason,
                        })
                        .with_span(span),
                    ),
                    Ok(None) => {}
                    Err(ImplementationSearchError::Cancelled) => return,
                    Err(ImplementationSearchError::LimitExceeded) => diagnostics.push(
                        Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                            resource: "interface compatibility",
                            limit: 4096,
                        })
                        .with_span(span),
                    ),
                }
                pending.extend(&instance.arguments);
                pending.extend(instance.associated_types.values());
            }
            TypeId::NativeObject(instance) | TypeId::Struct(instance) | TypeId::Enum(instance) => {
                pending.extend(&instance.arguments);
                pending.extend(instance.associated_types.values());
            }
            TypeId::Tuple(items) => pending.extend(items),
            TypeId::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            TypeId::Array(item)
            | TypeId::Set(item, _)
            | TypeId::Iter(item)
            | TypeId::Range(item, _) => pending.push(item),
            TypeId::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            _ => {}
        }
    }
}

impl AggregateCatalog {
    /// Explain why an applied trait cannot erase its receiver, or admit its whole surface.
    ///
    /// Static trait references remain legal. This checks all associated outputs,
    /// constraints and parents; cancellation or bounded traversal errors propagate.
    pub fn interface_incompatibility(
        &self,
        definitions: &DefinitionTable,
        instance: &NominalType,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, ImplementationSearchError> {
        let receiver = TypeId::Trait(instance.clone());
        let erased = self.trait_closure(instance, &receiver, cancel)?;
        let preserved =
            self.trait_closure(instance, &TypeId::SelfType(instance.declaration), cancel)?;
        if erased != preserved {
            return Ok(Some(
                "supertrait arguments cannot depend on erased Self".into(),
            ));
        }
        for parent in self.interface_closure(definitions, instance, &receiver, cancel)? {
            let Some(contract) = self.trait_(&parent.declaration) else {
                continue;
            };
            let owner = &contract.declaration.name;
            let reason = if !contract.associated_consts.is_empty() {
                Some("associated constants only support static dispatch")
            } else if !contract.associated_type_parameters.is_empty() {
                Some("generic associated types only support static dispatch")
            } else if contract
                .associated_types
                .keys()
                .any(|member| !parent.associated_types.contains_key(member))
            {
                Some("interface values require bindings for all associated types")
            } else {
                None
            };
            if let Some(reason) = reason {
                return Ok(Some(format!("trait `{owner}`: {reason}")));
            }
            let substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(parent.arguments.iter().cloned())
                .collect();
            let self_type = TypeId::SelfType(parent.declaration);
            let apply = |ty: &TypeId| {
                ty.instantiate(&substitution)
                    .with_associated_types(&parent, &self_type)
            };
            if bounds_depend_on_self(&contract.bounds, &parent, &apply) {
                return Ok(Some(format!(
                    "trait `{owner}` has a constraint depending on erased Self"
                )));
            }
            for (member, constraints) in &contract.associated_types {
                let bound = [(parent.associated_types[member].clone(), constraints.clone())]
                    .into_iter()
                    .collect();
                if bounds_depend_on_self(&bound, &parent, &apply) {
                    return Ok(Some(format!(
                        "trait `{owner}` has an associated type constraint depending on erased Self"
                    )));
                }
            }
            for method in &contract.methods {
                cancel
                    .check()
                    .map_err(|_| ImplementationSearchError::Cancelled)?;
                if !method
                    .params
                    .first()
                    .is_some_and(|param| param.name == "self" && param.ty == self_type)
                    || apply(&method.return_type).contains_self_type()
                    || method
                        .params
                        .iter()
                        .skip(1)
                        .any(|param| apply(&param.ty).contains_self_type())
                {
                    return Ok(Some(format!(
                        "method `{}` is not interface-compatible",
                        method.name
                    )));
                }
                if bounds_depend_on_self(&method.bounds, &parent, &apply) {
                    return Ok(Some(format!(
                        "method `{owner}.{}` has a call constraint depending on erased Self",
                        method.name
                    )));
                }
            }
        }
        Ok(None)
    }
}

fn bounds_depend_on_self(
    bounds: &GenericBounds,
    interface: &NominalType,
    apply: &impl Fn(&TypeId) -> TypeId,
) -> bool {
    bounds.iter().any(|(subject, constraints)| {
        let subject = apply(subject);
        constraints.iter().any(|constraint| {
            let required = apply(&TypeId::Trait(constraint.clone()));
            // Membership in this interface is already carried by the receiver.
            if subject == TypeId::SelfType(interface.declaration)
                && matches!(&required, TypeId::Trait(required) if interface.satisfies(required))
            {
                return false;
            }
            subject.contains_self_type() || required.contains_self_type()
        })
    })
}
