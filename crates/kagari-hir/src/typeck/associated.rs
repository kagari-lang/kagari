//! Associated types are declaration-owned projections, never diagnostic names.

use crate::{
    aggregates::traits::trait_inheritance_closure,
    collections::HashSet,
    declarations::{DeclarationId, Declarations},
    hir::{
        ids::GenericParamId,
        item::{
            Module,
            behavior::{AssociatedType, TraitRef},
        },
    },
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
    typeck::{
        constraints,
        state::TypeCheckState,
        table::{ResolvedTypeRef, ResolvedTypeTarget, TypeTable, match_implementation},
        ty::{self, TypeContext, resolve_named_type, resolve_type_in},
    },
    types::{AssociatedTypeFamily, AssociatedTypeParameters, NominalType, TypeId},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionId},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use smallvec::SmallVec;
use std::cell::RefCell;

pub(super) fn prepare(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeCheckState,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
    cancel: &CancellationToken,
) {
    let error = |item: &AssociatedType, reason: &str| {
        Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
            name: item.name.clone(),
            reason: reason.into(),
        })
        .with_span(
            lowered
                .module
                .type_ref(item.name_ref)
                .origin
                .diagnostic_span(),
        )
    };
    for item in &lowered.module.traits {
        let mut names = HashSet::default();
        for member in &item.associated_types {
            let DeclarationId::Definition(member_id) = declarations
                .associated_type(member.name_ref)
                .expect("collected associated type")
                .id
            else {
                unreachable!("associated type is a named definition");
            };
            if !names.insert(&member.name) {
                diagnostics.push(error(member, "duplicate declaration"));
            }
            if member.ty.is_some() {
                diagnostics.push(error(member, "associated type defaults are not supported"));
            }
            let mut parameter_names = HashSet::default();
            for parameter in &member.generic_params {
                if !parameter_names.insert(&lowered.module.generic_parameter(*parameter).name) {
                    diagnostics.push(error(member, "duplicate generic parameter"));
                }
            }
            let generics = item
                .generic_params
                .iter()
                .chain(&member.generic_params)
                .cloned()
                .collect::<Vec<_>>();
            constraints::resolve_owner_in(
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
                let inputs = family_inputs(&lowered.module, member, declarations, table);
                table.associated_type_parameters.insert(member_id, inputs);
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
                constraints::resolve_constraint(
                    lowered,
                    bound,
                    context,
                    table,
                    diagnostics,
                    cancel,
                );
            }
            let bounds = member
                .bounds
                .iter()
                .filter_map(|bound| table.constraint(bound.ty))
                .collect();
            table.associated_bounds.insert(member_id, bounds);
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
        let Some(mut interface) = table.constraint(reference.ty) else {
            continue;
        };
        let declared = members(&lowered.module, declarations, &interface.declaration);
        if !interface.associated_types.is_empty() {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                    name: ty::display_type(&lowered.module, reference.ty),
                    reason: "define associated types in the impl body, not the impl header".into(),
                })
                .with_span(
                    lowered
                        .module
                        .type_ref(reference.ty)
                        .origin
                        .diagnostic_span(),
                ),
            );
        }
        let mut names = HashSet::default();
        for member in &item.associated_types {
            let DeclarationId::Definition(member_id) = declarations
                .associated_type(member.name_ref)
                .expect("collected associated type")
                .id
            else {
                unreachable!("associated type is a named definition");
            };
            if !names.insert(&member.name) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                        name: member.name.clone(),
                        reason: "duplicate definition".into(),
                    })
                    .with_span(
                        lowered
                            .module
                            .type_ref(member.name_ref)
                            .origin
                            .diagnostic_span(),
                    ),
                );
            }
            if !declared.contains(&member.name) || !member.bounds.is_empty() || member.ty.is_none()
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssociatedType {
                        name: member.name.clone(),
                        reason: "expected a definition of a type declared by this trait".into(),
                    })
                    .with_span(
                        lowered
                            .module
                            .type_ref(member.name_ref)
                            .origin
                            .diagnostic_span(),
                    ),
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
            let mut parameter_names = HashSet::default();
            for parameter in &member.generic_params {
                if !parameter_names.insert(&lowered.module.generic_parameter(*parameter).name) {
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
                constraints::resolve_owner_in(
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
                        .with_span(lowered.module.type_ref(ty).origin.diagnostic_span()),
                    );
                }
                if member.generic_params.is_empty() {
                    if let Some(target) = declarations.definitions().lookup_child(
                        interface.declaration,
                        DefinitionKind::AssociatedType,
                        &member.name,
                        0,
                    ) {
                        interface.associated_types.insert(target, value);
                    }
                } else {
                    let inputs = family_inputs(&lowered.module, member, declarations, table);
                    table
                        .associated_type_families
                        .insert(member_id, AssociatedTypeFamily { inputs, value });
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
                    .with_span(
                        lowered.module.impls[item.id.index()]
                            .origin
                            .diagnostic_span(),
                    ),
                );
            }
        }
        table.insert_constraint(reference.ty, Some(interface));
    }
}

pub(super) fn family_inputs(
    module: &Module,
    member: &AssociatedType,
    declarations: &Declarations,
    table: &TypeTable,
) -> AssociatedTypeParameters {
    let mut bounds =
        constraints::parameter_bounds(module, &member.generic_params, declarations, table);
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
    AssociatedTypeParameters {
        parameters: member
            .generic_params
            .iter()
            .filter_map(|param| declarations.generic_type(*param))
            .collect(),
        bounds,
    }
}

pub(super) fn member_arity(
    module: &Module,
    declarations: &Declarations,
    owner: &DefinitionId,
    name: &str,
) -> Option<usize> {
    if let Some(item) = module
        .traits
        .iter()
        .find(|item| declarations.definition(ResolvedName::Trait(item.id)) == Some(owner))
    {
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
    module: &Module,
    declarations: &Declarations,
    owner: &DefinitionId,
) -> Vec<String> {
    if let Some(item) = module
        .traits
        .iter()
        .find(|item| declarations.definition(ResolvedName::Trait(item.id)) == Some(owner))
    {
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
    module: &Module,
    name: &str,
    arguments: Vec<TypeId>,
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
    cancel: &CancellationToken,
) -> ResolvedTypeRef {
    let Some((base, member)) = name.rsplit_once("::") else {
        return ResolvedTypeRef {
            exposure: None,
            ty: TypeId::Error,
            target: None,
        };
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
            return ResolvedTypeRef {
                exposure: None,
                ty: TypeId::Error,
                target: None,
            };
        };
        let interface = resolve_type_in(module, reference.ty, context, table, cancel);
        let Some(receiver) = item.for_type else {
            return ResolvedTypeRef {
                exposure: None,
                ty: TypeId::Error,
                target: None,
            };
        };
        let receiver = resolve_type_in(module, receiver, context, table, cancel);
        let TypeId::Trait(interface) = interface else {
            return ResolvedTypeRef {
                exposure: None,
                ty: TypeId::Error,
                target: None,
            };
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
            return ResolvedTypeRef {
                exposure: None,
                ty: TypeId::Error,
                target: None,
            };
        }
        let owner = owners.remove(0);
        let Some(id) = context.declarations.definitions().lookup_child(
            owner.declaration,
            DefinitionKind::AssociatedType,
            member,
            0,
        ) else {
            return ResolvedTypeRef {
                exposure: None,
                ty: TypeId::Error,
                target: None,
            };
        };
        let ty = qualified_projection(
            module,
            receiver,
            TypeId::Trait(owner),
            (member, arguments),
            context,
            table,
            cancel,
        );
        return projection_reference(ty, id);
    }
    let receiver = resolve_named_type(module, base, context).ty;
    let mut candidates = Vec::new();
    if let TypeId::SelfType(owner) = &receiver {
        candidates.push(NominalType {
            declaration: *owner,
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
            context.generics.iter().rev().copied().find(|param| {
                context.declarations.generic_type(*param).as_ref() == Some(parameter)
            })
        else {
            return ResolvedTypeRef {
                exposure: None,
                ty: TypeId::Error,
                target: None,
            };
        };
        let references = module
            .generic_parameter(param)
            .bounds
            .iter()
            .chain(owner_predicates(module, context, param));
        for reference in references {
            let resolved = table.constraint(reference.ty).or_else(|| {
                match resolve_type_in(module, reference.ty, context, table, cancel) {
                    TypeId::Trait(ty) => Some(ty),
                    _ => None,
                }
            });
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
        return ResolvedTypeRef {
            exposure: None,
            ty: TypeId::Error,
            target: None,
        };
    }
    let owner = candidates.remove(0);
    let Some(id) = context.declarations.definitions().lookup_child(
        owner.declaration,
        DefinitionKind::AssociatedType,
        member,
        0,
    ) else {
        return ResolvedTypeRef {
            exposure: None,
            ty: TypeId::Error,
            target: None,
        };
    };
    let ty = qualified_projection(
        module,
        receiver,
        TypeId::Trait(owner),
        (member, arguments),
        context,
        table,
        cancel,
    );
    projection_reference(ty, id)
}

// Generic IDs in the parameter vectors identify the declaration owner; names
// alone would incorrectly import predicates from unrelated same-spelled binders.
fn owner_predicates<'a>(
    module: &'a Module,
    context: TypeContext<'_>,
    parameter: GenericParamId,
) -> Vec<&'a TraitRef> {
    let target = &module.generic_parameter(parameter).name;
    // Impl predicates are inherited by both associated definitions and methods,
    // but a method binder shadowing the impl parameter does not inherit its bounds.
    let inherited = module
        .impls
        .iter()
        .filter(|item| item.generic_params.contains(&parameter))
        .flat_map(|item| &item.bounds);
    module
        .functions
        .iter()
        .map(|item| (&item.generic_params, &item.bounds))
        .chain(
            module
                .type_aliases
                .iter()
                .map(|item| (&item.generic_params, &item.bounds)),
        )
        .filter(|(parameters, _)| parameters.as_slice() == context.generics)
        .flat_map(|(_, bounds)| bounds)
        .chain(inherited)
        .filter(|bound| &bound.target == target)
        .flat_map(|bound| &bound.traits)
        .collect()
}

fn projection_reference(ty: TypeId, member: DefinitionId) -> ResolvedTypeRef {
    let target = (ty != TypeId::Error).then_some(ResolvedTypeTarget::AssociatedType(member));
    ResolvedTypeRef {
        ty,
        target,
        exposure: None,
    }
}

pub(super) fn qualified_projection(
    module: &Module,
    receiver: TypeId,
    interface: TypeId,
    member: (&str, Vec<TypeId>),
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
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
    let Some(id) = context.declarations.definitions().lookup_child(
        interface.declaration,
        DefinitionKind::AssociatedType,
        member,
        0,
    ) else {
        return TypeId::Error;
    };
    if let TypeId::Generic(parameter) = &receiver {
        // A qualified projection must be justified by a bound on its receiver.
        let Some(param) =
            context.generics.iter().copied().find(|param| {
                context.declarations.generic_type(*param).as_ref() == Some(parameter)
            })
        else {
            return TypeId::Error;
        };
        let references = module
            .generic_parameter(param)
            .bounds
            .iter()
            .chain(owner_predicates(module, context, param));
        let mut justified = None;
        for reference in references {
            let bound = match table.constraint(reference.ty) {
                Some(bound) => Some(bound),
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
                .filter_map(|param| context.declarations.generic_type(*param))
                .collect::<Vec<_>>();
            // Associated bindings are outputs. Select by receiver and trait inputs,
            // then check the requested output rather than selecting another impl.
            let mut requested = interface.clone();
            requested.associated_types.clear();
            let Some(substitution) =
                match_implementation(&implemented, &requested, &pattern, &receiver, &parameters)
            else {
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
                    .filter_map(|param| context.declarations.generic_type(*param))
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
    module: &Module,
    declarations: &Declarations,
    interface: &NominalType,
    receiver: &TypeId,
    table: &mut TypeCheckState,
    cancel: &CancellationToken,
) -> Vec<NominalType> {
    let table = RefCell::new(table);
    trait_inheritance_closure(interface, receiver, cancel, &|owner| {
        if let Some(surface) = declarations.imported_types().by_declaration(owner)
            && let TypeId::Trait(ty) = &surface.ty
        {
            return Some((
                ty.arguments
                    .iter()
                    .filter_map(|ty| match ty {
                        TypeId::Generic(param) => Some(param.clone()),
                        _ => None,
                    })
                    .collect(),
                surface.supertraits.clone(),
            ));
        }
        if let Some(item) = module
            .traits
            .iter()
            .find(|item| declarations.definition(ResolvedName::Trait(item.id)) == Some(owner))
        {
            let params = item
                .generic_params
                .iter()
                .filter_map(|param| declarations.generic_type(*param))
                .collect();
            let parents = item
                .supertraits
                .iter()
                .filter_map(|reference| {
                    let mut table = table.borrow_mut();
                    match table.constraint(reference.ty) {
                        Some(parent) => Some(parent),
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
            None
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{declare_analysis, lower::lower_module};
    use kagari_common::identity::map::DefinitionContext;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};
    use kagari_types::{language as standard_traits, language::Protocol};
    use std::sync::Arc;

    #[test]
    fn trait_identity_alone_does_not_replace_source_parameter_or_member_declarations() {
        let identity = standard_traits::identity(Protocol::Add);
        let mut sources = SourceDatabase::default();
        let file = sources
            .bind_module("untrusted.kgr", identity.module.clone())
            .unwrap();
        sources
            .set(
                "untrusted.kgr",
                "pub trait Add<Rhs, Extra> { type Output<T>; type Local; }".into(),
                SourceLayer::Base,
            )
            .unwrap();
        let source = sources.snapshot().file(file).unwrap().clone();
        let lowered = Arc::new(lower_module(&source));
        let declared = declare_analysis(
            lowered,
            Default::default(),
            Default::default(),
            &DefinitionContext::new().unwrap(),
            &Default::default(),
        )
        .unwrap();
        let declaration = declared
            .record
            .declarations
            .definition(ResolvedName::Trait(declared.lowered.module.traits[0].id))
            .unwrap();
        assert_eq!(
            declared.record.declarations.definitions().lookup(&identity),
            Some(*declaration)
        );
        assert_eq!(
            declared
                .record
                .declarations
                .parameters_of(declaration)
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["Rhs", "Extra"]
        );
        assert_eq!(
            members(
                &declared.lowered.module,
                &declared.record.declarations,
                declaration
            ),
            ["Output", "Local"]
        );
        assert_eq!(
            member_arity(
                &declared.lowered.module,
                &declared.record.declarations,
                declaration,
                "Output"
            ),
            Some(1)
        );
        assert_eq!(
            member_arity(
                &declared.lowered.module,
                &declared.record.declarations,
                declaration,
                "Local"
            ),
            Some(0)
        );
        assert_eq!(
            member_arity(
                &declared.lowered.module,
                &declared.record.declarations,
                declaration,
                "Unknown"
            ),
            None
        );
    }
}
