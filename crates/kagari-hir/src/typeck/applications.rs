//! Validate applied aggregate contracts from the shared checked catalog.
use super::{ConstraintTarget, GenericBounds, ModuleSignatures, TypeTable};
use crate::{aggregates::AggregateCatalog, lower::LoweredModule, types::TypeId};
use kagari_common::{Diagnostic, DiagnosticKind, Span, cancellation::CancellationToken};

pub(super) fn validate(
    ty: &TypeId,
    bounds: &GenericBounds,
    sources: (&AggregateCatalog, &crate::host::HostDeclarations),
    table: &TypeTable,
    span: Span,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let (catalog, hosts) = sources;
    let expanded = catalog
        .expanded_bounds(bounds, cancel)
        .unwrap_or_else(|_| bounds.clone());
    let bounds = &expanded;
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return;
        }
        let key = match ty {
            TypeId::Map { key, .. } => Some(key.as_ref()),
            TypeId::Set(key, _) => Some(key.as_ref()),
            _ => None,
        };
        if let Some(key) = key {
            use crate::builtin::traits::{StandardTrait, intrinsic_holds};
            if super::type_satisfies_standard_constraint(
                key,
                crate::builtin::surface::StandardTypeConstraint::HashKey,
                bounds,
            ) && [StandardTrait::Eq, StandardTrait::Hash]
                .into_iter()
                .any(|p| !intrinsic_holds(p, key, Some(catalog), bounds))
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                        type_name: key.display_name(),
                        constraint: "Eq + Hash".into(),
                        reason: "every key component must support stable equality and hashing"
                            .into(),
                    })
                    .with_span(span),
                );
            }
        }
        match ty {
            TypeId::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                if let Some(contract) = catalog.trait_(&interface.declaration)
                    && let Some(inputs) = contract.associated_type_parameters.get(member)
                {
                    let mut substitution: crate::types::TypeSubstitution = contract
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(interface.arguments.iter().cloned())
                        .chain(
                            inputs
                                .parameters
                                .iter()
                                .cloned()
                                .zip(arguments.iter().cloned()),
                        )
                        .collect();
                    substitution.insert_receiver(contract.id.clone(), receiver.as_ref().clone());
                    if inputs.parameters.len() != arguments.len() {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                                name: member.path.last().expect("member").name.clone(),
                                reason: "generic associated type argument count differs".into(),
                            })
                            .with_span(span),
                        );
                    }
                    for (parameter, constraints) in &inputs.bounds {
                        let actual = parameter.instantiate(&substitution);
                        for constraint in constraints {
                            let satisfied = match constraint {
                                ConstraintTarget::Standard(required) => {
                                    super::type_satisfies_standard_constraint(
                                        &actual, *required, bounds,
                                    )
                                }
                                ConstraintTarget::Trait(required) => {
                                    let required = required.instantiate(&substitution);
                                    bounds.get(&actual).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(available) if available.satisfies(&required))))
                                        || catalog.intrinsic_implementation(&required, &actual, bounds) || catalog.implementation_count(&required, &actual) + usize::from(hosts.implements(&required, &actual)) == 1
                                }
                            };
                            if !satisfied && !actual.is_unresolved() {
                                diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidAssociatedType { name: member.path.last().expect("member").name.clone(), reason: "generic associated type input does not satisfy its bound".into() }).with_span(span));
                            }
                        }
                    }
                }
                if receiver.is_concrete()
                    && catalog.implementation_count(interface, receiver)
                        + usize::from(hosts.implements(interface, receiver))
                        != 1
                {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                            name: member
                                .path
                                .last()
                                .expect("associated identity")
                                .name
                                .clone(),
                            reason:
                                "projection requires a unique implementation satisfying its bounds"
                                    .into(),
                        })
                        .with_span(span),
                    );
                }
                pending.push(receiver);
                pending.extend(arguments);
                pending.extend(&interface.arguments);
                pending.extend(interface.associated_types.values());
            }
            TypeId::Struct(instance) | TypeId::Enum(instance) | TypeId::Trait(instance) => {
                let contract = match ty {
                    TypeId::Struct(_) => catalog
                        .structure(&instance.declaration)
                        .map(|s| (&s.generic_params, &s.bounds)),
                    TypeId::Enum(_) => catalog
                        .enumeration(&instance.declaration)
                        .map(|s| (&s.generic_params, &s.bounds)),
                    TypeId::Trait(_) => catalog
                        .trait_(&instance.declaration)
                        .map(|s| (&s.generic_params, &s.bounds)),
                    _ => unreachable!(),
                };
                if let Some((parameters, required)) = contract {
                    let substitution = parameters
                        .iter()
                        .cloned()
                        .zip(instance.arguments.iter().cloned())
                        .collect();
                    for (parameter, actual) in parameters.iter().zip(&instance.arguments) {
                        for constraint in required
                            .get(&TypeId::Generic(parameter.clone()))
                            .into_iter()
                            .flatten()
                        {
                            // Trait implementation identity needs complete members;
                            // standard constraint recovery belongs to the shared checker.
                            if actual.is_unresolved()
                                && matches!(constraint, ConstraintTarget::Trait(t) if crate::builtin::traits::StandardTrait::from_id(&t.declaration).is_none())
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
                                    let applied = id.instantiate(&substitution);
                                    let satisfied = catalog.intrinsic_implementation(&applied, actual, bounds) || bounds.get(actual).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(available) if available.satisfies(&applied)))) || match actual {
                                        TypeId::Generic(parameter) => bounds
                                            .get(&TypeId::Generic(parameter.clone()))
                                            .is_some_and(|bounds| {
                                                bounds.contains(&ConstraintTarget::Trait(
                                                    applied.clone(),
                                                ))
                                            }),
                                        _ => match catalog.implementation_count(&applied, actual)
                                            + usize::from(hosts.implements(&applied, actual))
                                        {
                                            0 => crate::builtin::traits::StandardTrait::from_id(&applied.declaration).is_none() && table.implements(&applied, actual),
                                            1 => true,
                                            _ => false,
                                        },
                                    };
                                    if !satisfied {
                                        diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticKind::GenericBoundNotSatisfied {
                                                    type_name: actual.display_name(),
                                                    trait_name: catalog
                                                        .trait_(&id.declaration)
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
                if let TypeId::Trait(_) = ty
                    && let Some(contract) = catalog.trait_(&instance.declaration)
                {
                    let substitution = contract
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(instance.arguments.iter().cloned())
                        .collect();
                    for (member, actual) in &instance.associated_types {
                        for constraint in
                            contract.associated_types.get(member).into_iter().flatten()
                        {
                            let satisfied = match constraint {
                                ConstraintTarget::Standard(standard) => {
                                    super::constraints::type_satisfies_standard_constraint(
                                        actual, *standard, bounds,
                                    )
                                }
                                ConstraintTarget::Trait(required) => {
                                    let TypeId::Trait(required) =
                                        TypeId::Trait(required.instantiate(&substitution))
                                            .with_associated_types(instance)
                                    else {
                                        unreachable!()
                                    };
                                    bounds.get(actual).is_some_and(|constraints| constraints.iter().any(|constraint| matches!(constraint, ConstraintTarget::Trait(available) if available.satisfies(&required))))
                                        || catalog.intrinsic_implementation(&required, actual, bounds) || catalog.implementation_count(&required, actual) == 1
                                        || hosts.implements(&required, actual)
                                }
                            };
                            if !satisfied && !actual.is_unresolved() {
                                diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                                        name: member
                                            .path
                                            .last()
                                            .expect("associated identity")
                                            .name
                                            .clone(),
                                        reason: format!(
                                            "type `{}` does not satisfy its declared bound",
                                            actual.display_name()
                                        ),
                                    })
                                    .with_span(span),
                                );
                            }
                        }
                    }
                }
                pending.extend(&instance.arguments);
                pending.extend(instance.associated_types.values());
            }
            TypeId::Tuple(types) | TypeId::StandardEnum { args: types, .. } => {
                pending.extend(types)
            }
            TypeId::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            TypeId::Array(ty, _) | TypeId::Set(ty, _) | TypeId::Cursor(ty) => pending.push(ty),
            TypeId::Map { key, value, .. } => {
                pending.push(key);
                pending.push(value);
            }
            _ => {}
        }
    }
}

pub(crate) fn validate_signatures(
    lowered: &LoweredModule,
    declarations: &crate::declarations::Declarations,
    signatures: &ModuleSignatures,
    catalog: &AggregateCatalog,
    diagnostics: &mut crate::DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    super::families::validate(
        lowered,
        declarations,
        signatures,
        catalog,
        diagnostics,
        cancel,
    );
    for function in signatures.functions() {
        if cancel.check().is_err() {
            return;
        }
        for parameter in &function.params {
            validate_imported_interface_type(
                &parameter.ty,
                catalog,
                lowered.source.module_identity(),
                lowered.source_map.param_span(parameter.id),
                diagnostics,
                cancel,
            );
            validate(
                &parameter.ty,
                &function.bounds,
                (catalog, &declarations.hosts),
                signatures.type_table(),
                lowered.source_map.param_span(parameter.id),
                diagnostics,
                cancel,
            );
        }
        validate_imported_interface_type(
            &function.return_type,
            catalog,
            lowered.source.module_identity(),
            lowered.source_map.function_span(function.id),
            diagnostics,
            cancel,
        );
        validate(
            &function.return_type,
            &function.bounds,
            (catalog, &declarations.hosts),
            signatures.type_table(),
            lowered.source_map.function_span(function.id),
            diagnostics,
            cancel,
        );
    }
    for impl_block in &lowered.module.impls {
        if cancel.check().is_err() {
            return;
        }
        let Some(trait_ref) = &impl_block.trait_ref else {
            continue;
        };
        let Some(ConstraintTarget::Trait(instance)) =
            signatures.type_table().constraint(trait_ref.ty)
        else {
            continue;
        };
        let Some(contract) = catalog.trait_(&instance.declaration) else {
            continue;
        };
        let available = super::constraints::implementation_bounds(
            impl_block,
            declarations,
            signatures.type_table(),
        );
        validate(
            &TypeId::Trait(instance.clone()),
            &available,
            (catalog, &declarations.hosts),
            signatures.type_table(),
            lowered.source_map.type_span(trait_ref.ty),
            diagnostics,
            cancel,
        );
        let Some(receiver) = impl_block
            .for_type
            .and_then(|ty| signatures.type_table().type_ref(ty))
            .map(|resolved| &resolved.ty)
        else {
            continue;
        };
        let span = lowered.source_map.impl_span(impl_block.id);
        for method in &contract.methods {
            let Some(implementation) = impl_block
                .methods
                .iter()
                .find(|candidate| candidate.name == method.name)
            else {
                if method.has_default {
                    continue;
                }
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                        trait_name: contract.declaration.name.clone(),
                        method_name: method.name.clone(),
                        reason: "missing impl method".into(),
                    })
                    .with_span(span),
                );
                continue;
            };
            let Some(actual) = signatures
                .functions()
                .iter()
                .find(|function| function.id == implementation.function)
            else {
                continue;
            };
            super::check::compare_method_contract(
                method,
                actual,
                &super::check::MethodComparison {
                    trait_name: &contract.declaration.name,
                    method_name: &method.name,
                    trait_generic_count: contract.generic_params.len(),
                    impl_generic_count: impl_block.generic_params.len(),
                    receiver,
                    trait_owner: &contract.id,
                    trait_arguments: &instance.arguments,
                    catalog,
                    span,
                },
                diagnostics,
            );
        }
        for method in &impl_block.methods {
            if !contract
                .methods
                .iter()
                .any(|declared| declared.name == method.name)
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                        trait_name: contract.declaration.name.clone(),
                        method_name: method.name.clone(),
                        reason: "method is not declared by trait".into(),
                    })
                    .with_span(span),
                );
            }
        }
    }
    let identity = lowered.source.module_identity();
    for structure in catalog.structures().filter(|s| &s.id.module == identity) {
        for field in &structure.fields {
            validate(
                &field.ty,
                &structure.bounds,
                (catalog, &declarations.hosts),
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
                    (catalog, &declarations.hosts),
                    signatures.type_table(),
                    variant.declaration.location.range,
                    diagnostics,
                    cancel,
                );
            }
        }
    }
}

pub(super) fn validate_imported_interface_type(
    ty: &TypeId,
    catalog: &AggregateCatalog,
    module: &kagari_common::identity::ModuleIdentity,
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
            TypeId::Trait(instance) => {
                if crate::builtin::traits::StandardTrait::from_id(&instance.declaration).is_some() {
                    diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidInterfaceType { trait_name: ty.display_name(), reason: "standard protocols currently support static bounds and dispatch only".into() }).with_span(span));
                }
                let erased = catalog.trait_closure(instance, ty, cancel);
                let preserved = catalog.trait_closure(
                    instance,
                    &TypeId::SelfType(instance.declaration.clone()),
                    cancel,
                );
                if erased != preserved {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                            trait_name: ty.display_name(),
                            reason: "supertrait arguments cannot depend on erased Self".into(),
                        })
                        .with_span(span),
                    );
                }
                if let Ok(parents) = catalog.trait_closure(instance, ty, cancel) {
                    for (index, parent) in parents.into_iter().enumerate() {
                        if index == 0 && &instance.declaration.module == module {
                            continue;
                        }
                        let Some(contract) = catalog.trait_(&parent.declaration) else {
                            continue;
                        };
                        if crate::builtin::traits::StandardTrait::from_id(&parent.declaration)
                            .is_some()
                            && index != 0
                        {
                            diagnostics.push(
                                Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                                    trait_name: ty.display_name(),
                                    reason: "standard supertraits require static dispatch".into(),
                                })
                                .with_span(span),
                            );
                        }
                        if !contract.associated_consts.is_empty() {
                            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                                trait_name: contract.declaration.name.clone(), reason: "traits with associated constants only support static dispatch".into(),
                            }).with_span(span));
                        }
                        if !contract.associated_type_parameters.is_empty() {
                            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidInterfaceType { trait_name: contract.declaration.name.clone(), reason: "traits with generic associated types only support static dispatch".into() }).with_span(span));
                        }
                        if contract
                            .associated_types
                            .keys()
                            .any(|member| !parent.associated_types.contains_key(member))
                        {
                            diagnostics.push(
                                Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                                    trait_name: contract.declaration.name.clone(),
                                    reason:
                                        "interface values require bindings for all associated types"
                                            .into(),
                                })
                                .with_span(span),
                            );
                        }
                        for method in &contract.methods {
                            if !super::check::interface_method_compatible(
                                method.generic_params.len(),
                                contract.generic_params.len(),
                                method
                                    .params
                                    .first()
                                    .is_some_and(|param| param.name == "self"),
                                &method.return_type.with_associated_types(&parent),
                                method
                                    .params
                                    .iter()
                                    .skip(1)
                                    .map(|param| param.ty.with_associated_types(&parent))
                                    .collect::<Vec<_>>()
                                    .iter(),
                            ) {
                                diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::InvalidInterfaceType {
                                        trait_name: contract.declaration.name.clone(),
                                        reason: format!(
                                            "method `{}` is not interface-compatible",
                                            method.name
                                        ),
                                    })
                                    .with_span(span),
                                );
                            }
                        }
                    }
                }
                pending.extend(&instance.arguments);
            }
            TypeId::Struct(instance) | TypeId::Enum(instance) => {
                pending.extend(&instance.arguments)
            }
            TypeId::Tuple(items) | TypeId::StandardEnum { args: items, .. } => {
                pending.extend(items)
            }
            TypeId::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            TypeId::Array(item, _) | TypeId::Set(item, _) | TypeId::Cursor(item) => {
                pending.push(item)
            }
            TypeId::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            _ => {}
        }
    }
}
