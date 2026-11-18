//! Validate applied aggregate contracts from the shared checked catalog.

use crate::{
    DiagnosticBuffer,
    aggregates::{
        AggregateCatalog,
        implementations::{
            ImplementationSearchContext, ImplementationSearchError, ImplementationSearchLimits,
        },
    },
    declarations::Declarations,
    language::types::payload_reference_eligible,
    lower::LoweredModule,
    typeck::{
        GenericBounds, ModuleSignatures,
        aliases::{self, AliasValidation},
        check::{self, MethodComparison},
        constraints, families, interfaces, native_contracts,
    },
    types::{NominalType, TypeId, TypeSubstitution},
};

use kagari_common::{
    cancellation::CancellationToken, identity::table::DefinitionTable, span::Span,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{language::Protocol, range::RangeKind};

/// Checked declaration sources and their retained identity scope for application validation.
pub(super) struct ApplicationContext<'a> {
    pub definitions: &'a DefinitionTable,
    pub catalog: &'a AggregateCatalog,
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
    if let Err(reason) = validate_shape(context.catalog, ty, cancel) {
        if cancel.check().is_ok() {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::StandardConstraintNotSatisfied {
                    type_name: ty.display_name(context.definitions),
                    constraint: "well-formed type application".into(),
                    reason: reason.into(),
                })
                .with_span(span),
            );
        }
        return;
    }
    // Signature completion and body-query publication recheck this same token;
    // cancellation is returned there, not converted into a semantic diagnostic here.
    if let Err(ImplementationSearchError::LimitExceeded) =
        validate_inner(context, ty, bounds, span, diagnostics, cancel)
    {
        diagnostics.push(
            Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                resource: "trait implementation search",
                limit: 4096,
            })
            .with_span(span),
        );
    }
}

/// Check the complete application tree before any zip, substitution or trait search.
/// Associated-output keys are members of their declaring trait, not type applications.
fn validate_shape(
    catalog: &AggregateCatalog,
    ty: &TypeId,
    cancel: &CancellationToken,
) -> Result<(), &'static str> {
    let trait_shape = |instance: &NominalType| {
        let contract = catalog
            .trait_(&instance.declaration)
            .ok_or("trait declaration is missing or has another semantic category")?;
        if contract.generic_params.len() != instance.arguments.len() {
            return Err("trait type argument count differs from its declaration");
        }
        if instance.associated_types.keys().any(|member| {
            !contract.associated_types.contains_key(member)
                || contract
                    .associated_type_parameters
                    .get(member)
                    .is_some_and(|inputs| !inputs.parameters.is_empty())
        }) {
            return Err("associated output is not a non-generic member of this trait");
        }
        Ok(())
    };
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        cancel
            .check()
            .map_err(|_| "type application validation cancelled")?;
        let nominal = match ty {
            TypeId::Trait(instance) => {
                trait_shape(instance)?;
                Some(instance)
            }
            TypeId::NativeObject(instance) | TypeId::Struct(instance) | TypeId::Enum(instance) => {
                let arity = match ty {
                    TypeId::Struct(_) => catalog
                        .structure(&instance.declaration)
                        .map(|contract| contract.generic_params.len()),
                    TypeId::Enum(_) => catalog
                        .enumeration(&instance.declaration)
                        .map(|contract| contract.generic_params.len()),
                    TypeId::NativeObject(_) => catalog
                        .native_type(&instance.declaration)
                        .filter(|contract| contract.host.is_none())
                        .map(|contract| contract.generic_params.len()),
                    _ => unreachable!(),
                }
                .ok_or("nominal declaration is missing or has another semantic category")?;
                if arity != instance.arguments.len() {
                    return Err("nominal type argument count differs from its declaration");
                }
                if !instance.associated_types.is_empty() {
                    return Err("only trait applications can bind associated outputs");
                }
                Some(instance)
            }
            TypeId::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                trait_shape(interface)?;
                let contract = catalog
                    .trait_(&interface.declaration)
                    .expect("validated trait");
                if !contract.associated_types.contains_key(member)
                    || contract
                        .associated_type_parameters
                        .get(member)
                        .map_or(0, |inputs| inputs.parameters.len())
                        != arguments.len()
                {
                    return Err(
                        "projection member or type argument count differs from its declaration",
                    );
                }
                pending.push(receiver);
                pending.extend(arguments);
                Some(interface.as_ref())
            }
            TypeId::Host(id) => {
                if !catalog.native_type(id).is_some_and(|contract| {
                    contract.generic_params.is_empty() && contract.host.is_some()
                }) {
                    return Err("host type is missing its declared Host representation");
                }
                None
            }
            TypeId::Reference { target, .. } => {
                pending.push(target);
                None
            }
            TypeId::Tuple(items) => {
                pending.extend(items);
                None
            }
            TypeId::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
                None
            }
            TypeId::Array(item) | TypeId::Range(item, _) => {
                pending.push(item);
                None
            }
            _ => None,
        };
        if let Some(instance) = nominal {
            pending.extend(&instance.arguments);
            pending.extend(instance.associated_types.values());
        }
    }
    Ok(())
}

/// Resolve declared projection bounds through the same applied-interface query as
/// member lookup, retaining trait/family arguments and associated-output equalities.
fn requirement_satisfied(
    definitions: &DefinitionTable,
    catalog: &AggregateCatalog,
    required: &NominalType,
    actual: &TypeId,
    search: ImplementationSearchContext<'_>,
) -> Result<bool, ImplementationSearchError> {
    if search.assumptions.get(actual).is_some_and(|available| {
        available
            .iter()
            .any(|available| available.satisfies(required))
    }) {
        return Ok(true);
    }
    // Owner-qualified Self carries its declaring trait's formal application,
    // not an arbitrary application of that trait with different arguments/outputs.
    if let TypeId::SelfType(owner) = actual
        && let Some(contract) = catalog.trait_(owner)
    {
        let own = NominalType {
            declaration: *owner,
            arguments: contract
                .generic_params
                .iter()
                .cloned()
                .map(TypeId::Generic)
                .collect(),
            associated_types: Default::default(),
        };
        if catalog
            .trait_closure(&own, actual, search.cancel)?
            .iter()
            .any(|available| available.satisfies(required))
        {
            return Ok(true);
        }
    }
    if matches!(actual, TypeId::Projection { .. })
        && catalog
            .declared_interfaces(definitions, actual, search.assumptions, search.cancel)?
            .iter()
            .any(|available| available.satisfies(required))
    {
        return Ok(true);
    }
    match actual {
        TypeId::Generic(_) => {
            Ok(catalog.intrinsic_implementation(definitions, required, actual, search.assumptions))
        }
        _ => Ok(catalog.implementation_count_bounded(definitions, required, actual, search)? == 1),
    }
}

/// Proves declared generic requirements after a simultaneous substitution.
/// Shared by nominal applications and transparent aliases, including erased inputs.
pub(super) fn validate_requirements(
    context: ApplicationContext<'_>,
    required: &GenericBounds,
    substitution: &TypeSubstitution,
    bounds: &GenericBounds,
    span: Span,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) -> Result<(), ImplementationSearchError> {
    let ApplicationContext {
        definitions,
        catalog,
        ..
    } = context;
    let expanded = catalog.expanded_bounds(bounds, cancel)?;
    let bounds = &expanded;
    let search = ImplementationSearchContext {
        assumptions: bounds,
        limits: ImplementationSearchLimits {
            max_checks: 4096,
            max_depth: 64,
        },
        cancel,
    };
    for (parameter, constraints) in required {
        // Unbound Self predicates belong to implementation validation, not to
        // constructing an interface value. Projection/input predicates remain
        // obligations even when an alias erases all of its input parameters.
        if matches!(parameter, TypeId::SelfType(owner) if substitution.receiver(owner).is_none()) {
            continue;
        }
        if !aliases::within_expansion_limit(parameter, substitution, cancel) {
            cancel
                .check()
                .map_err(|_| ImplementationSearchError::Cancelled)?;
            return Err(ImplementationSearchError::LimitExceeded);
        }
        let actual = catalog.normalize_type(definitions, &parameter.instantiate(substitution));
        for constraint in constraints {
            cancel
                .check()
                .map_err(|_| ImplementationSearchError::Cancelled)?;
            // Recovery holes cannot authorize an implementation, but should not
            // duplicate the unresolved-type diagnostic from their owning position.
            if matches!(actual, TypeId::Unknown | TypeId::Error)
                || actual.is_unresolved()
                    && Protocol::from_definition(&constraint.declaration, definitions).is_none()
            {
                continue;
            }
            if !aliases::within_expansion_limit(
                &TypeId::Trait(constraint.clone()),
                substitution,
                cancel,
            ) {
                cancel
                    .check()
                    .map_err(|_| ImplementationSearchError::Cancelled)?;
                return Err(ImplementationSearchError::LimitExceeded);
            }
            let applied = constraint.instantiate(substitution);
            let satisfied = requirement_satisfied(definitions, catalog, &applied, &actual, search)?;
            if !satisfied {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::GenericBoundNotSatisfied {
                        type_name: actual.display_name(definitions),
                        trait_name: catalog
                            .trait_(&constraint.declaration)
                            .expect("checked trait contract")
                            .declaration
                            .name
                            .clone(),
                    })
                    .with_span(span),
                );
            }
        }
    }
    Ok(())
}

fn validate_outputs(
    context: ApplicationContext<'_>,
    instance: &NominalType,
    bounds: &GenericBounds,
    span: Span,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) -> Result<(), ImplementationSearchError> {
    let ApplicationContext {
        definitions,
        catalog,
        receiver,
    } = context;
    let contract = catalog
        .trait_(&instance.declaration)
        .expect("validated trait");
    let mut substitution: TypeSubstitution = contract
        .generic_params
        .iter()
        .cloned()
        .zip(instance.arguments.iter().cloned())
        .collect();
    if let Some(receiver) = receiver {
        substitution.insert_receiver(instance.declaration, receiver.clone());
    }
    let self_type = TypeId::SelfType(instance.declaration);
    let search = ImplementationSearchContext {
        assumptions: bounds,
        limits: ImplementationSearchLimits {
            max_checks: 4096,
            max_depth: 64,
        },
        cancel,
    };
    for (member, actual) in &instance.associated_types {
        for constraint in contract.associated_types.get(member).into_iter().flatten() {
            cancel
                .check()
                .map_err(|_| ImplementationSearchError::Cancelled)?;
            let TypeId::Trait(required) = TypeId::Trait(constraint.instantiate(&substitution))
                .with_associated_types(instance, receiver.unwrap_or(&self_type))
            else {
                unreachable!()
            };
            let satisfied = requirement_satisfied(definitions, catalog, &required, actual, search)?;
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
    Ok(())
}

fn validate_inner(
    context: ApplicationContext<'_>,
    ty: &TypeId,
    bounds: &GenericBounds,
    span: Span,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) -> Result<(), ImplementationSearchError> {
    let ApplicationContext {
        definitions,
        catalog,
        receiver,
    } = context;
    let expanded = catalog.expanded_bounds(bounds, cancel)?;
    let bounds = &expanded;
    let search = ImplementationSearchContext {
        assumptions: bounds,
        limits: ImplementationSearchLimits {
            max_checks: 4096,
            max_depth: 64,
        },
        cancel,
    };

    let mut pending = vec![(ty, receiver)];
    while let Some((ty, receiver)) = pending.pop() {
        cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
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
        match ty {
            TypeId::Reference { target, .. } => {
                if let TypeId::NativeObject(nominal) = target.as_ref()
                    && !payload_reference_eligible(nominal.declaration, definitions)
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
                let contract = catalog
                    .trait_(&interface.declaration)
                    .expect("validated projection trait");
                let mut substitution: TypeSubstitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(interface.arguments.iter().cloned())
                    .collect();
                substitution.insert_receiver(contract.id, receiver.as_ref().clone());
                validate_requirements(
                    ApplicationContext {
                        definitions,
                        catalog,
                        receiver: Some(receiver),
                    },
                    &contract.bounds,
                    &substitution,
                    bounds,
                    span,
                    diagnostics,
                    cancel,
                )?;
                validate_outputs(
                    ApplicationContext {
                        definitions,
                        catalog,
                        receiver: Some(receiver),
                    },
                    interface,
                    bounds,
                    span,
                    diagnostics,
                    cancel,
                )?;
                if let Some(inputs) = contract.associated_type_parameters.get(member) {
                    substitution.extend(
                        inputs
                            .parameters
                            .iter()
                            .cloned()
                            .zip(arguments.iter().cloned()),
                    );

                    for (parameter, constraints) in &inputs.bounds {
                        let actual = parameter.instantiate(&substitution);
                        for constraint in constraints {
                            let required = constraint.instantiate(&substitution);
                            let satisfied = requirement_satisfied(
                                definitions,
                                catalog,
                                &required,
                                &actual,
                                search,
                            )?;
                            if !satisfied && !actual.is_unresolved() {
                                diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidAssociatedType { name: definitions.resolve(*member).expect("scoped associated member").last().expect("associated member").name.to_owned(), reason: "generic associated type input does not satisfy its bound".into() }).with_span(span));
                            }
                        }
                    }
                }
                if !receiver.is_unresolved()
                    && !requirement_satisfied(definitions, catalog, interface, receiver, search)?
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
                                "projection receiver must satisfy its declaring trait and bounds"
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
                    validate_requirements(
                        ApplicationContext {
                            definitions,
                            catalog,
                            receiver,
                        },
                        required,
                        &substitution,
                        bounds,
                        span,
                        diagnostics,
                        cancel,
                    )?;
                }
                if let TypeId::Trait(_) = ty {
                    validate_outputs(
                        ApplicationContext {
                            definitions,
                            catalog,
                            receiver,
                        },
                        instance,
                        bounds,
                        span,
                        diagnostics,
                        cancel,
                    )?;
                }
                pending.extend(instance.arguments.iter().map(|ty| (ty, None)));
                pending.extend(instance.associated_types.values().map(|ty| (ty, None)));
            }
            TypeId::Tuple(types) => pending.extend(types.iter().map(|ty| (ty, None))),
            TypeId::Function { params, result } => {
                pending.extend(params.iter().map(|ty| (ty, None)));
                pending.push((result, None));
            }
            TypeId::Array(ty) | TypeId::Range(ty, _) => pending.push((ty, None)),

            _ => {}
        }
    }
    Ok(())
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
    native_contracts::validate(
        lowered,
        declarations,
        signatures,
        catalog,
        diagnostics,
        cancel,
    );
    AliasValidation {
        lowered,
        declarations,
        table: signatures.type_table(),
        catalog,
        cancel,
    }
    .signatures(signatures, diagnostics);
    for item in &lowered.module.statics {
        if cancel.check().is_err() {
            return;
        }
        let Some(ty) = signatures.statics.get(&item.id) else {
            continue;
        };
        let span = item.ty.map_or_else(
            || lowered.source_map.static_span(item.id),
            |ty| lowered.module.type_ref(ty).origin.diagnostic_span(),
        );
        let normalized = catalog.normalize_type(definitions, ty);
        if !ty.is_unresolved() && !normalized.is_concrete() {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownTypeAnnotation {
                    type_name: normalized.display_name(definitions),
                })
                .with_span(span),
            );
        }
        check::validate_reference_position(&normalized, span, diagnostics);
        if normalized.contains_stored_reference() {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidReference {
                    reason: "a scoped view cannot be stored in a static",
                })
                .with_span(span),
            );
        }
        interfaces::validate_type(definitions, &normalized, catalog, span, diagnostics, cancel);
        validate(
            ApplicationContext {
                definitions,
                catalog,
                receiver: None,
            },
            ty,
            &Default::default(),
            span,
            diagnostics,
            cancel,
        );
    }
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
        let Some(instance) = signatures.type_table().constraint(trait_ref.ty) else {
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
        let available = constraints::implementation_bounds(
            &lowered.module,
            impl_block,
            declarations,
            signatures.type_table(),
        );
        validate(
            ApplicationContext {
                definitions,
                catalog,
                receiver: Some(receiver),
            },
            &TypeId::Trait(instance.clone()),
            &available,
            lowered
                .module
                .type_ref(trait_ref.ty)
                .origin
                .diagnostic_span(),
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
    for (_, bounds, fields) in catalog
        .structures()
        .map(|s| (s.id, &s.bounds, &s.fields))
        .chain(catalog.native_types().map(|s| (s.id, &s.bounds, &s.fields)))
        .filter(|(id, _, _)| {
            definitions.resolve(*id).expect("scoped aggregate").module() == identity
        })
    {
        for field in fields {
            interfaces::validate_type(
                definitions,
                &field.ty,
                catalog,
                field.declaration.diagnostic_span(),
                diagnostics,
                cancel,
            );
            validate(
                ApplicationContext {
                    definitions,
                    catalog,
                    receiver: None,
                },
                &field.ty,
                bounds,
                field.declaration.diagnostic_span(),
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
                    variant.declaration.diagnostic_span(),
                    diagnostics,
                    cancel,
                );
                validate(
                    ApplicationContext {
                        definitions,
                        catalog,
                        receiver: None,
                    },
                    payload,
                    &enumeration.bounds,
                    variant.declaration.diagnostic_span(),
                    diagnostics,
                    cancel,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::{cancellation::CancellationToken, identity::map::DefinitionContext};
    use kagari_types::scalar::BuiltinType;

    #[test]
    fn deep_container_validation_uses_explicit_stack_and_honors_cancellation() {
        let definitions = DefinitionContext::new().unwrap().snapshot();
        let mut ty = TypeId::Range(
            Box::new(TypeId::Builtin(BuiltinType::F32)),
            kagari_types::range::RangeKind::Exclusive,
        );
        for _ in 0..10_000 {
            ty = TypeId::Array(Box::new(ty));
        }
        let mut diagnostics = DiagnosticBuffer::new();
        let catalog = AggregateCatalog::default();
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        validate(
            ApplicationContext {
                definitions: &definitions,
                catalog: &catalog,
                receiver: None,
            },
            &ty,
            &GenericBounds::default(),
            Default::default(),
            &mut diagnostics,
            &cancelled,
        );
        let cancelled_count = diagnostics.len();
        validate(
            ApplicationContext {
                definitions: &definitions,
                catalog: &catalog,
                receiver: None,
            },
            &ty,
            &GenericBounds::default(),
            Default::default(),
            &mut diagnostics,
            &Default::default(),
        );
        // Drop the synthetic deep input iteratively as well: this test exercises
        // validation, not the recursive representation's destructor.
        while let TypeId::Array(inner) = ty {
            ty = *inner;
        }
        assert_eq!(cancelled_count, 0);
        assert_eq!(diagnostics.len(), 1);
        assert!(matches!(&diagnostics[0].kind,
            DiagnosticKind::StandardConstraintNotSatisfied { type_name, .. } if type_name == "f32"
        ));
    }
}
