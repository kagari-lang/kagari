//! Associated types are declaration-owned projections, never diagnostic names.
use super::{
    ConstraintTarget, TypeTable,
    ty::{TypeContext, resolve_named_type, resolve_type_in},
};
use crate::{
    hir,
    types::{NominalType, TypeId, associated_type_id},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};

pub(super) fn prepare(
    lowered: &crate::lower::LoweredModule,
    declarations: &crate::declarations::Declarations,
    table: &mut TypeTable,
    diagnostics: &mut smallvec::SmallVec<[kagari_common::Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    use kagari_common::{Diagnostic, DiagnosticKind};
    let error = |item: &hir::AssociatedType, reason: &str| {
        Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
            name: item.name.clone(),
            reason: reason.into(),
        })
        .with_span(lowered.source_map.type_span(item.name_ref))
    };
    for item in &lowered.module.traits {
        let Some(owner) = declarations.definition(crate::resolver::ResolvedName::Trait(item.id))
        else {
            continue;
        };
        let mut names = std::collections::HashSet::new();
        for member in &item.associated_types {
            if !names.insert(&member.name) {
                diagnostics.push(error(member, "duplicate declaration"));
            }
            if member.ty.is_some() {
                diagnostics.push(error(member, "associated type defaults are not supported"));
            }
            let mut parameter_names = std::collections::HashSet::new();
            for parameter in &member.generic_params {
                if !parameter_names.insert(&parameter.name) {
                    diagnostics.push(error(member, "duplicate generic parameter"));
                }
            }
            let generics = item
                .generic_params
                .iter()
                .chain(&member.generic_params)
                .cloned()
                .collect::<Vec<_>>();
            super::constraints::resolve_owner_in(
                lowered,
                &member.parameter_bounds,
                TypeContext {
                    declarations,
                    generics: &generics,
                    self_type: Some(item.id),
                    implementation: None,
                },
                table,
                diagnostics,
                cancel,
            );
            if !member.generic_params.is_empty() {
                table.associated_type_parameters.insert(
                    associated_type_id(owner, &member.name),
                    family_inputs(member, declarations, table),
                );
            } else if !member.parameter_bounds.is_empty() {
                diagnostics.push(error(
                    member,
                    "associated type predicates require type parameters",
                ));
            }
            let context = TypeContext {
                declarations,
                generics: &generics,
                self_type: Some(item.id),
                implementation: None,
            };
            for bound in &member.bounds {
                super::constraints::resolve_constraint(
                    lowered,
                    bound,
                    context,
                    table,
                    diagnostics,
                    cancel,
                );
            }
            table.associated_bounds.insert(
                associated_type_id(owner, &member.name),
                member
                    .bounds
                    .iter()
                    .filter_map(|bound| table.constraint(bound.ty))
                    .collect(),
            );
        }
    }
    for item in &lowered.module.impls {
        if cancel.check().is_err() {
            break;
        }
        let Some(reference) = &item.trait_ref else {
            for member in &item.associated_types {
                diagnostics.push(error(
                    member,
                    "associated types require a trait implementation",
                ));
            }
            continue;
        };
        let Some(ConstraintTarget::Trait(mut interface)) = table.constraint(reference.ty) else {
            continue;
        };
        let declared = members(&lowered.module, declarations, &interface.declaration);
        if !interface.associated_types.is_empty() {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                    name: super::ty::display_type(&lowered.module, reference.ty),
                    reason: "define associated types in the impl body, not the impl header".into(),
                })
                .with_span(lowered.source_map.type_span(reference.ty)),
            );
        }
        let mut names = std::collections::HashSet::new();
        for member in &item.associated_types {
            if !names.insert(&member.name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                        name: member.name.clone(),
                        reason: "duplicate definition".into(),
                    })
                    .with_span(lowered.source_map.type_span(member.name_ref)),
                );
            }
            if !declared.contains(&member.name) || !member.bounds.is_empty() || member.ty.is_none()
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                        name: member.name.clone(),
                        reason: "expected a definition of a type declared by this trait".into(),
                    })
                    .with_span(lowered.source_map.type_span(member.name_ref)),
                );
            }
            if member_arity(
                &lowered.module,
                declarations,
                &interface.declaration,
                &member.name,
            ) != Some(member.generic_params.len())
            {
                diagnostics.push(error(
                    member,
                    "generic parameter count differs from the trait declaration",
                ));
            }
            let mut parameter_names = std::collections::HashSet::new();
            for parameter in &member.generic_params {
                if !parameter_names.insert(&parameter.name) {
                    diagnostics.push(error(member, "duplicate generic parameter"));
                }
            }
            if let Some(ty) = member.ty {
                let generics = item
                    .generic_params
                    .iter()
                    .chain(&member.generic_params)
                    .cloned()
                    .collect::<Vec<_>>();
                super::constraints::resolve_owner_in(
                    lowered,
                    &member.parameter_bounds,
                    TypeContext {
                        declarations,
                        generics: &generics,
                        self_type: None,
                        implementation: Some(item.id),
                    },
                    table,
                    diagnostics,
                    cancel,
                );
                let context = TypeContext {
                    declarations,
                    generics: &generics,
                    self_type: None,
                    implementation: Some(item.id),
                };
                let value = resolve_type_in(&lowered.module, ty, context, table, cancel);
                if value.is_unresolved() {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                            name: member.name.clone(),
                            reason: "definition is unknown or recursively depends on itself".into(),
                        })
                        .with_span(lowered.source_map.type_span(ty)),
                    );
                }
                if member.generic_params.is_empty() {
                    interface.associated_types.insert(
                        associated_type_id(&interface.declaration, &member.name),
                        value,
                    );
                } else {
                    let owner = declarations.impl_identity(item.id).expect("impl identity");
                    table.associated_type_families.insert(
                        associated_type_id(owner, &member.name),
                        crate::types::AssociatedTypeFamily {
                            inputs: family_inputs(member, declarations, table),
                            value,
                        },
                    );
                }
            }
        }
        for name in declared {
            if !names.contains(&name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                        name,
                        reason: "missing definition in trait implementation".into(),
                    })
                    .with_span(lowered.source_map.impl_span(item.id)),
                );
            }
        }
        table.insert_constraint(reference.ty, Some(ConstraintTarget::Trait(interface)));
    }
}

fn family_inputs(
    member: &hir::AssociatedType,
    declarations: &crate::declarations::Declarations,
    table: &TypeTable,
) -> crate::types::AssociatedTypeParameters {
    let mut bounds =
        super::constraints::parameter_bounds(&member.generic_params, declarations, table);
    for bound in &member.parameter_bounds {
        if let Some(target) = table.type_ref(bound.target_ref) {
            bounds.entry(target.ty.clone()).or_default().extend(
                bound
                    .traits
                    .iter()
                    .filter_map(|bound| table.constraint(bound.ty)),
            );
        }
    }
    crate::types::AssociatedTypeParameters {
        parameters: member
            .generic_params
            .iter()
            .filter_map(|param| declarations.generic_type(param.id))
            .collect(),
        bounds,
    }
}

pub(super) fn member_arity(
    module: &hir::Module,
    declarations: &crate::declarations::Declarations,
    owner: &DefinitionId,
    name: &str,
) -> Option<usize> {
    if let Some(kind) = crate::builtin::traits::StandardTrait::from_id(owner) {
        let id = associated_type_id(owner, name);
        return kind.contract().associated_types.contains_key(&id).then(|| {
            kind.contract()
                .associated_type_parameters
                .get(&id)
                .map_or(0, |p| p.parameters.len())
        });
    }
    if let Some(item) = module.traits.iter().find(|item| {
        declarations.definition(crate::resolver::ResolvedName::Trait(item.id)) == Some(owner)
    }) {
        return item
            .associated_types
            .iter()
            .find(|member| member.name == name)
            .map(|member| member.generic_params.len());
    }
    declarations
        .imported_types()
        .by_declaration(owner)?
        .associated_arities
        .get(name)
        .copied()
}

pub(super) fn members(
    module: &hir::Module,
    declarations: &crate::declarations::Declarations,
    owner: &DefinitionId,
) -> Vec<String> {
    if let Some(kind) = crate::builtin::traits::StandardTrait::from_id(owner) {
        return kind
            .contract()
            .associated_types
            .keys()
            .filter_map(|id| id.path.last().map(|p| p.name.clone()))
            .collect();
    }
    if let Some(item) = module.traits.iter().find(|item| {
        declarations.definition(crate::resolver::ResolvedName::Trait(item.id)) == Some(owner)
    }) {
        return item
            .associated_types
            .iter()
            .map(|item| item.name.clone())
            .collect();
    }
    declarations
        .imported_types()
        .by_declaration(owner)
        .map(|item| item.associated_types.clone())
        .unwrap_or_default()
}

pub(super) fn resolve_projection_name(
    module: &hir::Module,
    name: &str,
    arguments: Vec<TypeId>,
    context: TypeContext<'_>,
    table: &mut TypeTable,
    cancel: &CancellationToken,
) -> TypeId {
    let Some((base, member)) = name.rsplit_once("::") else {
        return TypeId::Error;
    };
    if base == "Self"
        && let Some(id) = context.implementation
    {
        let item = module
            .impls
            .iter()
            .find(|item| item.id == id)
            .expect("impl context");
        let Some(reference) = &item.trait_ref else {
            return TypeId::Error;
        };
        let interface = resolve_type_in(module, reference.ty, context, table, cancel);
        let Some(receiver) = item.for_type else {
            return TypeId::Error;
        };
        let receiver = resolve_type_in(module, receiver, context, table, cancel);
        let TypeId::Trait(interface) = interface else {
            return TypeId::Error;
        };
        let mut owners = inherited_traits(
            module,
            context.declarations,
            &interface,
            &receiver,
            table,
            cancel,
        );
        owners.retain(|owner| {
            members(module, context.declarations, &owner.declaration)
                .iter()
                .any(|name| name == member)
        });
        if owners.len() != 1 {
            return TypeId::Error;
        }
        return qualified_projection(
            module,
            receiver,
            TypeId::Trait(owners.remove(0)),
            (member, arguments),
            context,
            table,
            cancel,
        );
    }
    let receiver = resolve_named_type(base, context).ty;
    let mut candidates = Vec::new();
    if let TypeId::SelfType(owner) = &receiver {
        candidates.push(NominalType {
            declaration: owner.clone(),
            arguments: context
                .declarations
                .parameters_of(owner)
                .into_iter()
                .map(TypeId::Generic)
                .collect(),
            associated_types: Default::default(),
        });
    }
    if let TypeId::Generic(parameter) = &receiver {
        let Some(param) =
            context.generics.iter().rev().find(|param| {
                context.declarations.generic_type(param.id).as_ref() == Some(parameter)
            })
        else {
            return TypeId::Error;
        };
        let mut references = param.bounds.iter().collect::<Vec<_>>();
        for function in &module.functions {
            if function.generic_params == context.generics {
                references.extend(
                    function
                        .bounds
                        .iter()
                        .filter(|bound| bound.target == base)
                        .flat_map(|bound| &bound.traits),
                );
            }
        }
        for reference in references {
            let resolved = table
                .constraint(reference.ty)
                .and_then(|constraint| match constraint {
                    ConstraintTarget::Trait(ty) => Some(ty),
                    _ => None,
                })
                .or_else(
                    || match resolve_type_in(module, reference.ty, context, table, cancel) {
                        TypeId::Trait(ty) => Some(ty),
                        _ => None,
                    },
                );
            if let Some(interface) = resolved
                && !candidates.contains(&interface)
            {
                candidates.push(interface);
            }
        }
    }
    candidates = candidates
        .into_iter()
        .flat_map(|interface| {
            inherited_traits(
                module,
                context.declarations,
                &interface,
                &receiver,
                table,
                cancel,
            )
        })
        .collect();
    let mut unique = Vec::new();
    for candidate in candidates {
        if members(module, context.declarations, &candidate.declaration)
            .iter()
            .any(|name| name == member)
            && !unique.contains(&candidate)
        {
            unique.push(candidate);
        }
    }
    let mut candidates = unique;
    if candidates.len() != 1 {
        return TypeId::Error;
    }
    qualified_projection(
        module,
        receiver,
        TypeId::Trait(candidates.remove(0)),
        (member, arguments),
        context,
        table,
        cancel,
    )
}

pub(super) fn qualified_projection(
    module: &hir::Module,
    receiver: TypeId,
    interface: TypeId,
    member: (&str, Vec<TypeId>),
    context: TypeContext<'_>,
    table: &mut TypeTable,
    cancel: &CancellationToken,
) -> TypeId {
    let (member, arguments) = member;
    let TypeId::Trait(mut interface) = interface else {
        return TypeId::Error;
    };
    if member_arity(module, context.declarations, &interface.declaration, member)
        != Some(arguments.len())
    {
        return TypeId::Error;
    }
    let id = associated_type_id(&interface.declaration, member);
    if let TypeId::Generic(parameter) = &receiver {
        // A qualified projection must be justified by a bound on its receiver.
        let references = context
            .generics
            .iter()
            .filter(|param| context.declarations.generic_type(param.id).as_ref() == Some(parameter))
            .flat_map(|param| &param.bounds)
            .chain(
                module
                    .functions
                    .iter()
                    .filter(|function| function.generic_params == context.generics)
                    .flat_map(|function| &function.bounds)
                    .filter(|bound| bound.target == parameter.name)
                    .flat_map(|bound| &bound.traits),
            )
            .collect::<Vec<_>>();
        let mut justified = None;
        for reference in references {
            let bound = match table.constraint(reference.ty) {
                Some(ConstraintTarget::Trait(bound)) => Some(bound),
                _ => match resolve_type_in(module, reference.ty, context, table, cancel) {
                    TypeId::Trait(bound) => Some(bound),
                    _ => None,
                },
            };
            if let Some(bound) = bound {
                for parent in inherited_traits(
                    module,
                    context.declarations,
                    &bound,
                    &receiver,
                    table,
                    cancel,
                ) {
                    if parent.satisfies(&interface) {
                        justified = Some(parent);
                    }
                }
            }
        }
        let Some(bound) = justified else {
            return TypeId::Error;
        };
        interface = bound;
    }
    if matches!(receiver, TypeId::Generic(_))
        && let Some(ty) = interface.associated_types.get(&id)
    {
        return ty.clone();
    }
    if !arguments.is_empty() {
        return TypeId::Projection {
            receiver: Box::new(receiver),
            interface: Box::new(interface),
            member: id,
            arguments,
        };
    }
    // Preserve explicit concrete projections until the shared catalog can prove
    // the selected implementation's bounds across the dependency closure.
    if context.implementation.is_none()
        && !matches!(receiver, TypeId::SelfType(_) | TypeId::Generic(_))
    {
        return TypeId::Projection {
            arguments,
            receiver: Box::new(receiver),
            interface: Box::new(interface),
            member: id,
        };
    }
    let mut values = Vec::new();
    if !matches!(receiver, TypeId::SelfType(_) | TypeId::Generic(_)) {
        for item in &module.impls {
            let Some(reference) = &item.trait_ref else {
                continue;
            };
            let Some(target) = item.for_type else {
                continue;
            };
            let impl_context = TypeContext {
                generics: &item.generic_params,
                implementation: Some(item.id),
                self_type: None,
                ..context
            };
            let implemented = resolve_type_in(module, reference.ty, impl_context, table, cancel);
            let pattern = resolve_type_in(module, target, impl_context, table, cancel);
            let TypeId::Trait(implemented) = implemented else {
                continue;
            };
            let parameters = item
                .generic_params
                .iter()
                .filter_map(|param| context.declarations.generic_type(param.id))
                .collect::<Vec<_>>();
            // Associated bindings are outputs. Select by receiver and trait inputs,
            // then check the requested output rather than selecting another impl.
            let mut requested = interface.clone();
            requested.associated_types.clear();
            let Some(substitution) = super::match_implementation(
                &implemented,
                &requested,
                &pattern,
                &receiver,
                &parameters,
            ) else {
                continue;
            };
            let Some(member_definition) = item
                .associated_types
                .iter()
                .find(|value| value.name == member)
            else {
                continue;
            };
            let Some(value) = member_definition.ty else {
                continue;
            };
            let generics = item
                .generic_params
                .iter()
                .chain(&member_definition.generic_params)
                .cloned()
                .collect::<Vec<_>>();
            let mut substitution = substitution;
            substitution.extend(
                member_definition
                    .generic_params
                    .iter()
                    .filter_map(|param| context.declarations.generic_type(param.id))
                    .zip(arguments.iter().cloned()),
            );
            let value = resolve_type_in(
                module,
                value,
                TypeContext {
                    generics: &generics,
                    ..impl_context
                },
                table,
                cancel,
            )
            .instantiate(&substitution);
            if interface
                .associated_types
                .get(&id)
                .is_some_and(|required| required != &value)
            {
                return TypeId::Error;
            }
            values.push(value);
        }
    }
    if values.len() == 1 {
        return values.remove(0);
    }
    TypeId::Projection {
        arguments,
        receiver: Box::new(receiver),
        interface: Box::new(interface),
        member: id,
    }
}

/// Declaration surfaces are sufficient to resolve projections before function
/// signatures and the complete implementation catalog have been assembled.
fn inherited_traits(
    module: &hir::Module,
    declarations: &crate::declarations::Declarations,
    interface: &NominalType,
    receiver: &TypeId,
    table: &mut TypeTable,
    cancel: &CancellationToken,
) -> Vec<NominalType> {
    let table = std::cell::RefCell::new(table);
    crate::aggregates::trait_inheritance_closure(interface, receiver, cancel, &|owner| {
        if let Some(item) = module.traits.iter().find(|item| {
            declarations.definition(crate::resolver::ResolvedName::Trait(item.id)) == Some(owner)
        }) {
            let params = item
                .generic_params
                .iter()
                .filter_map(|param| declarations.generic_type(param.id))
                .collect();
            let parents = item
                .supertraits
                .iter()
                .filter_map(|reference| {
                    let mut table = table.borrow_mut();
                    match table.constraint(reference.ty) {
                        Some(ConstraintTarget::Trait(parent)) => Some(parent),
                        _ => match resolve_type_in(
                            module,
                            reference.ty,
                            TypeContext {
                                declarations,
                                generics: &item.generic_params,
                                self_type: Some(item.id),
                                implementation: None,
                            },
                            &mut table,
                            cancel,
                        ) {
                            TypeId::Trait(parent) => Some(parent),
                            _ => None,
                        },
                    }
                })
                .collect();
            Some((params, parents))
        } else {
            if let Some(kind) = crate::builtin::traits::StandardTrait::from_id(owner) {
                return Some((
                    kind.contract().generic_params.clone(),
                    kind.contract().supertraits.clone(),
                ));
            }
            let imported = declarations.imported_types().by_declaration(owner)?;
            let TypeId::Trait(ty) = &imported.ty else {
                return None;
            };
            Some((
                ty.arguments
                    .iter()
                    .filter_map(|ty| match ty {
                        TypeId::Generic(param) => Some(param.clone()),
                        _ => None,
                    })
                    .collect(),
                imported.supertraits.clone(),
            ))
        }
    })
    .unwrap_or_default()
}

/// Normalize projections using already checked associated bindings. Recursive
/// projections stop at an explicit budget rather than recursing indefinitely.
pub(crate) fn normalize(
    ty: &TypeId,
    lookup: &impl Fn(&NominalType, &TypeId, &DefinitionId, &[TypeId]) -> Option<TypeId>,
) -> TypeId {
    if !ty.contains_projection() {
        return ty.clone();
    }
    fn walk(
        ty: &TypeId,
        lookup: &impl Fn(&NominalType, &TypeId, &DefinitionId, &[TypeId]) -> Option<TypeId>,
        depth: usize,
        remaining: &mut usize,
    ) -> TypeId {
        if depth > 64 || *remaining == 0 {
            return TypeId::Error;
        }
        *remaining -= 1;
        let ty = ty.map_children(|child| walk(child, lookup, depth + 1, remaining));
        if let TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = &ty
        {
            let replacement = arguments
                .is_empty()
                .then(|| interface.associated_types.get(member).cloned())
                .flatten()
                .or_else(|| {
                    if let TypeId::Trait(actual) = receiver.as_ref() {
                        arguments
                            .is_empty()
                            .then(|| actual.associated_types.get(member).cloned())
                            .flatten()
                            .or_else(|| lookup(interface, receiver, member, arguments))
                    } else {
                        lookup(interface, receiver, member, arguments)
                    }
                });
            if let Some(value) = replacement {
                if value == ty {
                    return TypeId::Error;
                }
                return walk(&value, lookup, depth + 1, remaining);
            }
        }
        ty
    }
    walk(ty, lookup, 0, &mut 8192)
}
