//! Validate applied aggregate contracts from the shared checked catalog.
use super::{ConstraintTarget, GenericBounds, ModuleSignatures, TypeTable};
use crate::{aggregates::AggregateCatalog, lower::LoweredModule, types::TypeId};
use kagari_common::{Diagnostic, DiagnosticKind, Span, cancellation::CancellationToken};

pub(super) fn validate(
    ty: &TypeId,
    bounds: &GenericBounds,
    catalog: &AggregateCatalog,
    table: &TypeTable,
    span: Span,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return;
        }
        match ty {
            TypeId::Struct(instance) | TypeId::Enum(instance) => {
                let contract = match ty {
                    TypeId::Struct(_) => catalog
                        .structure(&instance.declaration)
                        .map(|s| (&s.generic_params, &s.bounds)),
                    _ => catalog
                        .enumeration(&instance.declaration)
                        .map(|s| (&s.generic_params, &s.bounds)),
                };
                if let Some((parameters, required)) = contract {
                    for (parameter, actual) in parameters.iter().zip(&instance.arguments) {
                        for constraint in required.get(parameter).into_iter().flatten() {
                            // Trait implementation identity and recursive equality may
                            // depend on missing members. Other standard constraints
                            // can already reject a known outer type such as [Error].
                            if actual.is_unresolved()
                                && matches!(constraint,
                                    ConstraintTarget::Trait(_)
                                    | ConstraintTarget::Standard(crate::builtin::surface::StandardTypeConstraint::Comparable))
                            {
                                continue;
                            }
                            match constraint {
                                ConstraintTarget::Standard(constraint) => {
                                    super::check::validate_standard_constraint_type(
                                        actual,
                                        *constraint,
                                        bounds,
                                        span,
                                        diagnostics,
                                    )
                                }
                                ConstraintTarget::Trait(id) => {
                                    let satisfied = match actual {
                                        TypeId::Generic(parameter) => bounds
                                            .get(parameter)
                                            .is_some_and(|bounds| bounds.contains(constraint)),
                                        _ => table.implements(id, actual),
                                    };
                                    if !satisfied {
                                        diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticKind::GenericBoundNotSatisfied {
                                                    type_name: actual.display_name(),
                                                    trait_name: catalog
                                                        .trait_(id)
                                                        .expect("checked trait contract")
                                                        .declaration
                                                        .name
                                                        .clone(),
                                                },
                                            )
                                            .with_span(span),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                pending.extend(&instance.arguments);
            }
            TypeId::Trait(instance) => pending.extend(&instance.arguments),
            TypeId::Tuple(types) | TypeId::StandardEnum { args: types, .. } => {
                pending.extend(types)
            }
            TypeId::Array(ty) | TypeId::Set(ty) => pending.push(ty),
            TypeId::Map { key, value } => {
                pending.push(key);
                pending.push(value);
            }
            _ => {}
        }
    }
}

pub(crate) fn validate_signatures(
    lowered: &LoweredModule,
    signatures: &ModuleSignatures,
    catalog: &AggregateCatalog,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    for function in signatures.functions() {
        if cancel.check().is_err() {
            return;
        }
        for parameter in &function.params {
            validate(
                &parameter.ty,
                &function.bounds,
                catalog,
                signatures.type_table(),
                lowered.source_map.param_span(parameter.id),
                diagnostics,
                cancel,
            );
        }
        validate(
            &function.return_type,
            &function.bounds,
            catalog,
            signatures.type_table(),
            lowered.source_map.function_span(function.id),
            diagnostics,
            cancel,
        );
    }
    let identity = lowered.source.module_identity();
    for structure in catalog.structures().filter(|s| &s.id.module == identity) {
        for field in &structure.fields {
            validate(
                &field.ty,
                &structure.bounds,
                catalog,
                signatures.type_table(),
                field.declaration.location.range,
                diagnostics,
                cancel,
            );
        }
    }
    for enumeration in catalog.enumerations().filter(|s| &s.id.module == identity) {
        for variant in &enumeration.variants {
            for payload in &variant.payload {
                validate(
                    payload,
                    &enumeration.bounds,
                    catalog,
                    signatures.type_table(),
                    variant.declaration.location.range,
                    diagnostics,
                    cancel,
                );
            }
        }
    }
}
