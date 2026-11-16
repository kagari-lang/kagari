//! Shared static validation for Native contract sidecars and structured inputs.

use crate::{
    DiagnosticBuffer,
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    declarations::{DeclarationId, Declarations},
    hir::item::{
        Item,
        function::{FunctionImplementation, FunctionKind},
    },
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
    typeck::{
        GenericBounds, ModuleSignatures,
        applications::{self, ApplicationContext},
    },
    types::{
        NominalType, TypeId, TypeSubstitution,
        semantic::{lower_type, raise_nominal_type, raise_type},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, mapping::DefinitionMappingError, table::DefinitionId},
    span::Span,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FnDecl, Param, host::function_policy_valid, names::NameNamespace,
        native::NativeTypeConstructor, requirement::NativeCallableRequirement, verify::type_valid,
    },
    ty::{GenericParam, Ty},
};
use std::collections::HashSet;

#[cfg(test)]
mod tests;

/// Check real type heads through namespace access, but associated-output keys
/// through their actual trait owner/member table. Binder owners are not type uses.
fn validate_type_access(
    ty: &Ty,
    declarations: &Declarations,
    catalog: &AggregateCatalog,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    let definitions = declarations.definitions();
    let type_head = |id| {
        let resolved = declarations
            .resolve_definition(id, NameNamespace::Type, cancel)
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        let selected = resolved.and_then(|resolved| {
            declarations
                .definition(resolved.clone())
                .copied()
                .or_else(|| {
                    let imported = declarations.imported_types().resolved(resolved)?;
                    match imported.declaration.id {
                        DeclarationId::Definition(id) => Some(id),
                        _ => None,
                    }
                })
        });
        if selected == Some(id) {
            Ok(())
        } else {
            Err(DefinitionMappingError::InvalidContract)
        }
    };
    let member = |owner: DefinitionId, id: DefinitionId, arity: usize| {
        let valid = definitions.parent(id) == Ok(Some(owner))
            && definitions
                .resolve(id)
                .ok()
                .and_then(|view| view.last())
                .is_some_and(|part| {
                    part.kind == DefinitionKind::AssociatedType && part.occurrence == 0
                })
            && catalog.trait_(&owner).is_some_and(|contract| {
                contract.associated_types.contains_key(&id)
                    && contract
                        .associated_type_parameters
                        .get(&id)
                        .map_or(0, |parameters| parameters.parameters.len())
                        == arity
            });
        if valid {
            Ok(())
        } else {
            Err(DefinitionMappingError::InvalidContract)
        }
    };
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        match ty {
            Ty::Projection {
                receiver,
                interface,
                member: id,
                arguments,
            } => {
                member(interface.declaration, *id, arguments.len())?;
                pending.push(receiver);
                pending.extend(arguments);
            }
            Ty::Host(id) => type_head(*id)?,
            Ty::Reference { target, .. } | Ty::Array(target) | Ty::Range(target, _) => {
                pending.push(target)
            }
            Ty::Tuple(items) => pending.extend(items),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::Struct(_)
            | Ty::Enum(_)
            | Ty::NativeObject(_)
            | Ty::Trait(_)
            | Ty::Parameter { .. }
            | Ty::SelfType(_)
            | Ty::Builtin(_) => {}
        }
        let nominal = match ty {
            Ty::Projection { interface, .. } => Some(interface.as_ref()),
            Ty::Struct(nominal)
            | Ty::Enum(nominal)
            | Ty::NativeObject(nominal)
            | Ty::Trait(nominal) => Some(nominal),
            _ => None,
        };
        if let Some(nominal) = nominal {
            type_head(nominal.declaration)?;
            pending.extend(&nominal.arguments);
            for (id, value) in &nominal.associated_types {
                member(nominal.declaration, *id, 0)?;
                pending.push(value);
            }
        }
    }
    Ok(())
}

/// Prove the operation's receiver capability and every fully supplied method bound.
/// Empty method arguments intentionally defer method-owned binders, not known obligations.
fn validate_operation(
    context: ApplicationContext<'_>,
    required: &NativeCallableRequirement,
    available: &GenericBounds,
    self_identity: &TypeSubstitution,
    span: Span,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) -> Result<(), ImplementationSearchError> {
    let catalog = context.catalog;
    let interface = raise_nominal_type(&required.interface);
    let receiver = raise_type(&required.receiver);
    let contract = catalog
        .trait_(&interface.declaration)
        .expect("validated operation trait");
    let method = catalog
        .trait_method(&required.member)
        .expect("validated operation member");
    let mut substitution: TypeSubstitution = contract
        .generic_params
        .iter()
        .cloned()
        .zip(interface.arguments.iter().cloned())
        .chain(
            method
                .generic_params
                .iter()
                .filter(|parameter| parameter.owner == method.id)
                .cloned()
                .zip(required.arguments.iter().map(raise_type)),
        )
        .collect();
    substitution.insert_receiver(contract.id, receiver.clone());
    let apply = |ty: &TypeId| {
        ty.instantiate(&substitution)
            .with_associated_types(&interface, &receiver)
    };
    let mut method_bounds = GenericBounds::new();
    for (parameter, constraints) in &method.bounds {
        let parameter = apply(parameter);
        let constraints = constraints
            .iter()
            .map(|constraint| {
                let TypeId::Trait(applied) = apply(&TypeId::Trait(constraint.clone())) else {
                    unreachable!()
                };
                applied
            })
            .filter(|constraint| {
                !required.arguments.is_empty()
                    || !mentions_binder(&parameter, method.id)
                        && !mentions_binder(&TypeId::Trait(constraint.clone()), method.id)
            })
            .collect();
        method_bounds.insert(parameter, constraints);
    }
    applications::validate_requirements(
        ApplicationContext {
            definitions: context.definitions,
            catalog,
            receiver: Some(&receiver),
        },
        &method_bounds,
        self_identity,
        available,
        span,
        diagnostics,
        cancel,
    )?;
    applications::validate_requirements(
        context,
        &[(receiver, vec![interface])].into(),
        self_identity,
        available,
        span,
        diagnostics,
        cancel,
    )
}

fn mentions_binder(ty: &TypeId, owner: DefinitionId) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            TypeId::Generic(parameter) if parameter.owner == owner => return true,
            TypeId::NativeObject(instance)
            | TypeId::Struct(instance)
            | TypeId::Enum(instance)
            | TypeId::Trait(instance) => {
                pending.extend(&instance.arguments);
                pending.extend(instance.associated_types.values());
            }
            TypeId::Projection {
                receiver,
                interface,
                arguments,
                ..
            } => {
                pending.push(receiver);
                pending.extend(arguments);
                pending.extend(&interface.arguments);
                pending.extend(interface.associated_types.values());
            }
            TypeId::Reference { target, .. } | TypeId::Array(target) | TypeId::Range(target, _) => {
                pending.push(target)
            }
            TypeId::Tuple(items) => pending.extend(items),
            TypeId::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            _ => {}
        }
    }
    false
}

pub(super) fn validate(
    lowered: &LoweredModule,
    declarations: &Declarations,
    signatures: &ModuleSignatures,
    catalog: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let definitions = declarations.definitions();
    for item in &lowered.module.native_types {
        let Some(id) = declarations.definition(ResolvedName::NativeType(item.id)) else {
            continue;
        };
        if let Some(constructor) = NativeTypeConstructor::from_definition(*id, definitions)
            && (item.host.is_some()
                || constructor.arity() != item.generic_params.len()
                || !item.fields.is_empty())
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidNativeSignature {
                    function: item.name.clone(),
                    binding: "invalid language constructor declaration shape".into(),
                })
                .with_span(lowered.source_map.item_span(Item::NativeType(item.id))),
            );
        }
    }
    for method in lowered.module.traits.iter().flat_map(|item| &item.methods) {
        if !method.override_allowed
            && matches!(
                lowered.module.functions[method.function.index()].implementation,
                FunctionImplementation::Required
            )
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidNativeSignature {
                    function: method.name.clone(),
                    binding: "required methods must permit implementation".into(),
                })
                .with_span(lowered.source_map.function_span(method.function)),
            );
        }
    }
    for function in &lowered.module.functions {
        if cancel.check().is_err() {
            return;
        }
        let Some(signature) = signatures
            .functions()
            .iter()
            .find(|signature| signature.id == function.id)
        else {
            continue;
        };
        if let FunctionImplementation::Native(contract) = &function.implementation
            && contract.receiver_access.is_some()
            && (function.kind != FunctionKind::ImplMethod
                || !signature.params.first().is_some_and(|param| {
                    param.name == "self"
                        && !param.ty.is_unresolved()
                        && lower_type(&param.ty).reference_target_valid()
                }))
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidNativeSignature {
                    function: function.name.clone(),
                    binding: "receiver policy requires an eligible nominal self parameter".into(),
                })
                .with_span(lowered.source_map.function_span(function.id)),
            );
        }
        let FunctionImplementation::Native(contract) = &function.implementation else {
            continue;
        };
        if signature.return_type.is_unresolved()
            || signature
                .params
                .iter()
                .any(|param| param.ty.is_unresolved())
        {
            continue;
        }
        let declaration = declarations
            .definition(ResolvedName::Function(function.id))
            .expect("checked function identity");
        let policy_surface = FnDecl {
            name: signature.name.clone(),
            implementation: CallableImplementation::Native(*declaration),
            host_policy: contract.host_policy.clone(),
            method_policy: MethodPolicy {
                receiver_access: contract.receiver_access,
                ..Default::default()
            },
            generic_params: signature
                .generic_params
                .iter()
                .map(|parameter| GenericParam {
                    owner: parameter.owner,
                    position: parameter.position,
                })
                .collect(),
            bounds: Vec::new(),
            params: signature
                .params
                .iter()
                .map(|param| Param {
                    name: param.name.clone(),
                    ty: lower_type(&param.ty),
                    mutable: param.writeability.is_var(),
                })
                .collect(),
            return_type: lower_type(&signature.return_type),
        };
        if !function_policy_valid(&policy_surface, definitions) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidNativeSignature {
                    function: function.name.clone(),
                    binding: "invalid Host passing or effect policy".into(),
                })
                .with_span(lowered.source_map.function_span(function.id)),
            );
        }
        if contract.callable_requirements.is_empty() && contract.concrete_result.is_none() {
            continue;
        }
        let params = signature
            .generic_params
            .iter()
            .map(|param| (param.owner, param.position))
            .collect::<HashSet<_>>();
        let self_owner = (function.kind == FunctionKind::TraitMethod)
            .then(|| {
                lowered
                    .module
                    .traits
                    .iter()
                    .find(|item| {
                        item.methods
                            .iter()
                            .any(|method| method.function == function.id)
                    })
                    .and_then(|item| declarations.definition(ResolvedName::Trait(item.id)))
            })
            .flatten();
        // Native body obligations must not use declaration applications' unbound-Self deferral.
        let mut self_identity = TypeSubstitution::default();
        if let Some(owner) = self_owner {
            self_identity.insert_receiver(*owner, TypeId::SelfType(*owner));
        }
        let mut available = signature.bounds.clone();
        if let Some(owner) = self_owner.and_then(|owner| catalog.trait_(owner)) {
            available
                .entry(TypeId::SelfType(owner.id))
                .or_default()
                .push(NominalType {
                    declaration: owner.id,
                    arguments: owner
                        .generic_params
                        .iter()
                        .cloned()
                        .map(TypeId::Generic)
                        .collect(),
                    associated_types: Default::default(),
                });
        }
        let valid = |ty: &Ty| {
            if !ty.within_wire_limits()
                || !ty.reference_position_valid()
                || !type_valid(ty, &params, self_owner, cancel, definitions)
            {
                return Err("invalid type scope or shape");
            }
            validate_type_access(ty, declarations, catalog, cancel)
                .map_err(|_| "type dependency, visibility or associated member is not admitted")
        };
        let mut malformed = contract.callable_requirements.len() > 4096;
        let span = lowered.source_map.function_span(function.id);
        let mut invalid_types = Vec::new();
        let mut check_application = |ty: &Ty, receiver: Option<&TypeId>| {
            if let Err(reason) = valid(ty) {
                invalid_types.push(format!(
                    "{}: {reason}",
                    raise_type(ty).display_name(definitions)
                ));
                return false;
            }
            let diagnostic_start = diagnostics.len();
            applications::validate(
                ApplicationContext {
                    definitions,
                    catalog,
                    receiver,
                },
                &raise_type(ty),
                &available,
                span,
                diagnostics,
                cancel,
            );
            diagnostics.len() == diagnostic_start
        };
        for required in &contract.callable_requirements {
            if cancel.check().is_err() {
                return;
            }
            let method = catalog.trait_method(&required.member);
            let interface = catalog.trait_(&required.interface.declaration);
            malformed |= !method.is_some_and(|method| {
                method.owner == required.interface.declaration
                    // Empty arguments retain the existing deferred generic-method
                    // selection contract; explicit applications must be complete.
                    && (required.arguments.is_empty() || method
                        .generic_params
                        .iter()
                        .filter(|parameter| parameter.owner == method.id)
                        .count()
                        == required.arguments.len())
            }) || !interface.is_some_and(|interface| {
                interface.generic_params.len() == required.interface.arguments.len()
            });
            let receiver_valid = check_application(&required.receiver, None);
            malformed |= !receiver_valid;
            let receiver = raise_type(&required.receiver);
            malformed |= !check_application(
                &Ty::Trait(required.interface.clone()),
                receiver_valid.then_some(&receiver),
            );
            for argument in &required.arguments {
                malformed |= !check_application(argument, None);
            }
        }
        if let Some(result) = &contract.concrete_result {
            malformed |= !matches!(signature.return_type, TypeId::Trait(_))
                || result.contains_stored_reference()
                || !result.reference_position_valid()
                || !check_application(result, None);
        }
        if !malformed {
            for required in &contract.callable_requirements {
                if let Err(ImplementationSearchError::LimitExceeded) = validate_operation(
                    ApplicationContext {
                        definitions,
                        catalog,
                        receiver: None,
                    },
                    required,
                    &available,
                    &self_identity,
                    span,
                    diagnostics,
                    cancel,
                ) {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                            resource: "Native operation proof",
                            limit: 4096,
                        })
                        .with_span(span),
                    );
                }
            }
        }
        if !malformed
            && let (Some(result), TypeId::Trait(interface)) =
                (&contract.concrete_result, &signature.return_type)
        {
            let required = [(raise_type(result), vec![interface.clone()])].into();
            if let Err(ImplementationSearchError::LimitExceeded) =
                applications::validate_requirements(
                    ApplicationContext {
                        definitions,
                        catalog,
                        receiver: None,
                    },
                    &required,
                    &self_identity,
                    &available,
                    span,
                    diagnostics,
                    cancel,
                )
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                        resource: "Native concrete-result proof",
                        limit: 4096,
                    })
                    .with_span(span),
                );
            }
        }
        if malformed {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidNativeSignature {
                    function: function.name.clone(),
                    binding: format!(
                        "invalid static Native operation or concrete-result contract: {}",
                        if invalid_types.is_empty() {
                            "trait/member identity or generic arity".into()
                        } else {
                            invalid_types.join("; ")
                        }
                    ),
                })
                .with_span(span),
            );
        }
    }
}
