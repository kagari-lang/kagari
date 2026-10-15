//! Validate applied aggregate contracts from the shared checked catalog.

use crate::{
    DiagnosticBuffer,
    aggregates::AggregateCatalog,
    declarations::Declarations,
    host::HostDeclarations,
    lower::LoweredModule,
    native::NativeTypeKind,
    typeck::{
        GenericBounds, ModuleSignatures,
        check::{self, MethodComparison},
        constraints::{self, type_satisfies_standard_constraint},
        families, interfaces,
        table::ConstraintTarget,
    },
    types::{TypeId, TypeSubstitution},
};

use kagari_common::{
    cancellation::CancellationToken, identity::table::DefinitionTable, span::Span,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{
    declaration::native::NativeStorageLayout, language::Protocol, range::RangeKind,
    surface::StandardTypeConstraint,
};

/// Checked declaration sources and their retained identity scope for application validation.
pub(super) struct ApplicationContext<'a> {
    pub definitions: &'a DefinitionTable,
    pub catalog: &'a AggregateCatalog,
    pub hosts: &'a HostDeclarations,
    /// Concrete receiver of the root impl application; nested trait values keep their own Self.
    pub receiver: Option<&'a TypeId>,
}

pub(super) fn validate(
    context: ApplicationContext<'_>,
    ty: &TypeId,
    bounds: &GenericBounds,
    span: Span,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let ApplicationContext {
        definitions,
        catalog,
        hosts,
        receiver,
    } = context;
    let expanded = catalog
        .expanded_bounds(bounds, cancel)
        .unwrap_or_else(|_| bounds.clone());
    let bounds = &expanded;
    let mut pending = vec![(ty, receiver)];
    while let Some((ty, receiver)) = pending.pop() {
        if cancel.check().is_err() {
            return;
        }
        if let TypeId::Range(element, kind) = ty
            && *kind != RangeKind::Full
            && element.is_concrete()
            && !element.is_integer()
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                    type_name: element.display_name(definitions),
                    constraint: "integer range endpoint".into(),
                    reason: "range endpoints must have a builtin integer type".into(),
                })
                .with_span(span),
            );
        }
        let key = match ty {
            TypeId::Map { key, .. } => Some(key.as_ref()),
            TypeId::Set(key, _) => Some(key.as_ref()),
            _ => None,
        };
        if let Some(key) = key
            && !type_satisfies_standard_constraint(
                definitions,
                key,
                StandardTypeConstraint::HashKey,
                bounds,
                Some(catalog),
            )
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                    type_name: key.display_name(definitions),
                    constraint: "Eq + Hash".into(),
                    reason: "every key component must support stable equality and hashing".into(),
                })
                .with_span(span),
            );
        }
        match ty {
            TypeId::Reference { target, .. } => {
                if let TypeId::NativeObject(nominal) = target.as_ref()
                    && !catalog.native_type(&nominal.declaration).is_some_and(|ty| {
                        matches!(
                            ty.representation,
                            NativeTypeKind::Storage {
                                layout: NativeStorageLayout::Opaque,
                                ..
                            }
                        )
                    })
                {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidReference {
                            reason: "this native storage has no host payload reference contract",
                        })
                        .with_span(span),
                    );
                }
                pending.push((target, None));
            }
            TypeId::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                if let Some(contract) = catalog.trait_(&interface.declaration)
                    && let Some(inputs) = contract.associated_type_parameters.get(member)
                {
                    let mut substitution: TypeSubstitution = contract
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
                    substitution.insert_receiver(contract.id, receiver.as_ref().clone());
                    if inputs.parameters.len() != arguments.len() {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                                name: definitions
                                    .resolve(*member)
                                    .expect("scoped associated member")
                                    .last()
                                    .expect("associated member")
                                    .name
                                    .to_owned(),
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
                                    type_satisfies_standard_constraint(
                                        definitions,
                                        &actual,
                                        *required,
                                        bounds,
                                        Some(catalog),
                                    )
                                }
                                ConstraintTarget::Trait(required) => {
                                    let required = required.instantiate(&substitution);
                                    bounds.get(&actual).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(available) if available.satisfies(&required))))
                                        || catalog.intrinsic_implementation(definitions, &required, &actual, bounds) || catalog.implementation_count(definitions, &required, &actual) + usize::from(hosts.implements(&required, &actual)) == 1
                                }
                            };
                            if !satisfied && !actual.is_unresolved() {
                                diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidAssociatedType { name: definitions.resolve(*member).expect("scoped associated member").last().expect("associated member").name.to_owned(), reason: "generic associated type input does not satisfy its bound".into() }).with_span(span));
                            }
                        }
                    }
                }
                if receiver.is_concrete()
                    && catalog.implementation_count(definitions, interface, receiver)
                        + usize::from(hosts.implements(interface, receiver))
                        != 1
                {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                            name: definitions
                                .resolve(*member)
                                .expect("scoped associated member")
                                .last()
                                .expect("associated member")
                                .name
                                .to_owned(),
                            reason:
                                "projection requires a unique implementation satisfying its bounds"
                                    .into(),
                        })
                        .with_span(span),
                    );
                }
                pending.push((receiver, None));
                pending.extend(arguments.iter().map(|ty| (ty, None)));
                pending.extend(interface.arguments.iter().map(|ty| (ty, None)));
                pending.extend(interface.associated_types.values().map(|ty| (ty, None)));
            }
            TypeId::NativeObject(instance)
            | TypeId::Struct(instance)
            | TypeId::Enum(instance)
            | TypeId::Trait(instance) => {
                let contract = match ty {
                    TypeId::NativeObject(_) => catalog
                        .native_type(&instance.declaration)
                        .map(|s| (&s.generic_params, &s.bounds)),
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
                    let mut substitution: TypeSubstitution = parameters
                        .iter()
                        .cloned()
                        .zip(instance.arguments.iter().cloned())
                        .collect();
                    if let Some(receiver) = receiver {
                        substitution.insert_receiver(instance.declaration, receiver.clone());
                    }
                    for (parameter, actual) in parameters.iter().zip(&instance.arguments) {
                        for constraint in required
                            .get(&TypeId::Generic(parameter.clone()))
                            .into_iter()
                            .flatten()
                        {
                            // Trait implementation identity needs complete members;
                            // standard constraint recovery belongs to the shared checker.
                            if actual.is_unresolved()
                                && matches!(constraint, ConstraintTarget::Trait(t) if Protocol::from_definition(&t.declaration, definitions).is_none())
                            {
                                continue;
                            }
                            match constraint {
                                ConstraintTarget::Standard(constraint) => {
                                    check::validate_standard_constraint_type(
                                        definitions,
                                        actual,
                                        *constraint,
                                        bounds,
                                        span,
                                        diagnostics,
                                        Some(catalog),
                                    )
                                }
                                ConstraintTarget::Trait(id) => {
                                    let applied = id.instantiate(&substitution);
                                    let satisfied = catalog.intrinsic_implementation(definitions, &applied, actual, bounds) || bounds.get(actual).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(available) if available.satisfies(&applied)))) || match actual {
                                        TypeId::Generic(parameter) => bounds
                                            .get(&TypeId::Generic(parameter.clone()))
                                            .is_some_and(|bounds| {
                                                bounds.contains(&ConstraintTarget::Trait(
                                                    applied.clone(),
                                                ))
                                            }),
                                        _ => catalog.implementation_count(definitions, &applied, actual)
                                            + usize::from(hosts.implements(&applied, actual)) == 1,
                                    };
                                    if !satisfied {
                                        diagnostics.push(
                                            Diagnostic::error(
                                                DiagnosticKind::GenericBoundNotSatisfied {
                                                    type_name: actual.display_name(definitions),
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
                    let mut substitution: TypeSubstitution = contract
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(instance.arguments.iter().cloned())
                        .collect();
                    if let Some(receiver) = receiver {
                        substitution.insert_receiver(instance.declaration, receiver.clone());
                    }
                    for (member, actual) in &instance.associated_types {
                        for constraint in
                            contract.associated_types.get(member).into_iter().flatten()
                        {
                            let satisfied = match constraint {
                                ConstraintTarget::Standard(standard) => {
                                    constraints::type_satisfies_standard_constraint(
                                        definitions,
                                        actual,
                                        *standard,
                                        bounds,
                                        Some(catalog),
                                    )
                                }
                                ConstraintTarget::Trait(required) => {
                                    let TypeId::Trait(required) =
                                        TypeId::Trait(required.instantiate(&substitution))
                                            .with_associated_types(
                                                instance,
                                                receiver.unwrap_or(&TypeId::SelfType(
                                                    instance.declaration,
                                                )),
                                            )
                                    else {
                                        unreachable!()
                                    };
                                    bounds.get(actual).is_some_and(|constraints| constraints.iter().any(|constraint| matches!(constraint, ConstraintTarget::Trait(available) if available.satisfies(&required))))
                                        || catalog.intrinsic_implementation(definitions, &required, actual, bounds) || catalog.implementation_count(definitions, &required, actual) == 1
                                        || hosts.implements(&required, actual)
                                }
                            };
                            if !satisfied && !actual.is_unresolved() {
                                diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                                        name: definitions
                                            .resolve(*member)
                                            .expect("scoped associated member")
                                            .last()
                                            .expect("associated member")
                                            .name
                                            .to_owned(),
                                        reason: format!(
                                            "type `{}` does not satisfy its declared bound",
                                            actual.display_name(definitions)
                                        ),
                                    })
                                    .with_span(span),
                                );
                            }
                        }
                    }
                }
                pending.extend(instance.arguments.iter().map(|ty| (ty, None)));
                pending.extend(instance.associated_types.values().map(|ty| (ty, None)));
            }
            TypeId::Tuple(types) => pending.extend(types.iter().map(|ty| (ty, None))),
            TypeId::Function { params, result } => {
                pending.extend(params.iter().map(|ty| (ty, None)));
                pending.push((result, None));
            }
            TypeId::Array(ty) | TypeId::Set(ty, _) | TypeId::Iter(ty) | TypeId::Range(ty, _) => {
                pending.push((ty, None))
            }
            TypeId::Map { key, value, .. } => {
                pending.push((key, None));
                pending.push((value, None));
            }
            _ => {}
        }
    }
}

pub(crate) fn validate_signatures(
    lowered: &LoweredModule,
    declarations: &Declarations,
    signatures: &ModuleSignatures,
    catalog: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let definitions = declarations.definitions();
    families::validate(lowered, declarations, catalog, diagnostics, cancel);
    for function in signatures.functions() {
        if cancel.check().is_err() {
            return;
        }
        for parameter in &function.params {
            interfaces::validate_type(
                definitions,
                &parameter.ty,
                catalog,
                lowered.source_map.param_span(parameter.id),
                diagnostics,
                cancel,
            );
            validate(
                ApplicationContext {
                    definitions,
                    catalog,
                    hosts: &declarations.hosts,
                    receiver: None,
                },
                &parameter.ty,
                &function.bounds,
                lowered.source_map.param_span(parameter.id),
                diagnostics,
                cancel,
            );
        }
        interfaces::validate_type(
            definitions,
            &function.return_type,
            catalog,
            lowered.source_map.function_span(function.id),
            diagnostics,
            cancel,
        );
        validate(
            ApplicationContext {
                definitions,
                catalog,
                hosts: &declarations.hosts,
                receiver: None,
            },
            &function.return_type,
            &function.bounds,
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
        let Some(receiver) = impl_block
            .for_type
            .and_then(|ty| signatures.type_table().type_ref(ty))
            .map(|resolved| &resolved.ty)
        else {
            continue;
        };
        let available =
            constraints::implementation_bounds(impl_block, declarations, signatures.type_table());
        validate(
            ApplicationContext {
                definitions,
                catalog,
                hosts: &declarations.hosts,
                receiver: Some(receiver),
            },
            &TypeId::Trait(instance.clone()),
            &available,
            lowered.source_map.type_span(trait_ref.ty),
            diagnostics,
            cancel,
        );
        let span = lowered.source_map.impl_span(impl_block.id);
        for method in &contract.methods {
            let Some(implementation) = impl_block
                .methods
                .iter()
                .find(|candidate| candidate.name == method.name)
            else {
                if method.default.is_some() {
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
            if !method.allows_override() {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::TraitMethodMismatch {
                        trait_name: contract.declaration.name.clone(),
                        method_name: method.name.clone(),
                        reason: "this method declaration forbids overriding".into(),
                    })
                    .with_span(span),
                );
                continue;
            }
            check::compare_method_contract(
                method,
                actual,
                &MethodComparison {
                    definitions,
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
    for structure in catalog.structures().filter(|s| {
        definitions
            .resolve(s.id)
            .expect("scoped aggregate")
            .module()
            == identity
    }) {
        for field in &structure.fields {
            interfaces::validate_type(
                definitions,
                &field.ty,
                catalog,
                field.declaration.location.range,
                diagnostics,
                cancel,
            );
            validate(
                ApplicationContext {
                    definitions,
                    catalog,
                    hosts: &declarations.hosts,
                    receiver: None,
                },
                &field.ty,
                &structure.bounds,
                field.declaration.location.range,
                diagnostics,
                cancel,
            );
        }
    }
    for enumeration in catalog.enumerations().filter(|s| {
        definitions
            .resolve(s.id)
            .expect("scoped aggregate")
            .module()
            == identity
    }) {
        for variant in &enumeration.variants {
            for payload in &variant.payload {
                interfaces::validate_type(
                    definitions,
                    payload,
                    catalog,
                    variant.declaration.location.range,
                    diagnostics,
                    cancel,
                );
                validate(
                    ApplicationContext {
                        definitions,
                        catalog,
                        hosts: &declarations.hosts,
                        receiver: None,
                    },
                    payload,
                    &enumeration.bounds,
                    variant.declaration.location.range,
                    diagnostics,
                    cancel,
                );
            }
        }
    }
}
