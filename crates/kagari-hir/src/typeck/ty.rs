//! Translate HIR type spelling into semantic types using declaration/generic context.
//! Resolved types and target identities are recorded by TypeRefId for later queries;
//! invalid or incomplete spelling produces diagnostics and recovery types.

use crate::{
    declarations::{DeclarationId, Declarations},
    hir::{
        ids::{BodyOwner, HirOwner, ImplId, TraitId, TypeRefId},
        item::{Module, behavior::GenericParam},
        ty::{DefinitionExposure, TypeKind},
    },
    imports::types::aliases::CanonicalAlias,
    language::types::TypeConstructor,
    resolver::resolved::ResolvedName,
    typeck::{
        aliases::ExpansionBudget,
        associated,
        state::TypeCheckState,
        table::{ResolvedTypeRef, ResolvedTypeTarget},
    },
    types::{NominalType, TypeId},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{declaration::names::NameNamespace, language::Protocol, scalar::BuiltinType};
use std::iter;

/// Borrowed declaration environment for resolving one written type.
#[derive(Debug, Clone, Copy)]
pub(super) struct TypeContext<'a> {
    /// Declaration and imported type names visible at the type occurrence.
    pub declarations: &'a Declarations,
    /// Generic parameter names mapped to their canonical semantic parameters.
    pub generics: &'a [GenericParam],
    /// Semantic replacement for Self, when the enclosing declaration defines one.
    pub self_type: Option<TraitId>,
    /// Enclosing implementation identity, when resolving implementation-specific types.
    pub implementation: Option<ImplId>,
}

fn alias_target<'a>(
    target: Option<&ResolvedTypeTarget>,
    declarations: &'a Declarations,
) -> Option<&'a CanonicalAlias> {
    let ResolvedTypeTarget::Source(id) = target? else {
        return None;
    };
    declarations.imported_types().alias(id)
}

pub(super) fn native_type(
    target: Option<ResolvedTypeTarget>,
    declarations: &Declarations,
) -> Option<TypeConstructor> {
    match target? {
        ResolvedTypeTarget::NativeType(id) => {
            if let Some(kind) = declarations.native_type(id) {
                return Some(TypeConstructor::registered(&kind));
            }
            let declaration = *declarations.definition(ResolvedName::NativeType(id))?;
            Some(TypeConstructor::Native {
                declaration,
                arity: declarations.parameters_of(&declaration).len(),
            })
        }
        ResolvedTypeTarget::Source(id) => {
            let imported = declarations.imported_types().by_declaration(&id)?;
            if let Some(kind) = &imported.native_type {
                return Some(TypeConstructor::registered(kind));
            }
            let TypeId::NativeObject(nominal) = &imported.ty else {
                return None;
            };
            Some(TypeConstructor::Native {
                declaration: nominal.declaration,
                arity: nominal.arguments.len(),
            })
        }
        _ => None,
    }
}

pub(super) fn resolve_named_type(name: &str, context: TypeContext<'_>) -> ResolvedTypeRef {
    resolve_named_reference(name, context, true)
}

fn resolve_named_reference(
    name: &str,
    context: TypeContext<'_>,
    materialize_alias: bool,
) -> ResolvedTypeRef {
    let mut target = None;
    let ty = (|| {
        if let Some(param) = context
            .generics
            .iter()
            .rev()
            .find(|param| param.name == name)
        {
            target = Some(ResolvedTypeTarget::Generic(param.id));
            context
                .declarations
                .generic_type(param.id)
                .map(TypeId::Generic)
        } else if name == "Self" && context.self_type.is_some() {
            target = context.self_type.map(ResolvedTypeTarget::Trait);
            context
                .declarations
                .definition(ResolvedName::Trait(context.self_type?))
                .cloned()
                .map(TypeId::SelfType)
        } else if let Some(ty) =
            TypeId::from_name(name).filter(|ty| *ty != TypeId::Builtin(BuiltinType::String))
        {
            Some(ty)
        } else {
            context
                .declarations
                .resolve_name(name, NameNamespace::Type)
                .and_then(|resolved| {
                    resolve_declaration_reference(resolved, context, materialize_alias, &mut target)
                })
        }
    })();
    ResolvedTypeRef {
        ty: ty.unwrap_or(TypeId::Error),
        target,
        exposure: None,
    }
}

fn resolve_declaration_reference(
    resolved: ResolvedName,
    context: TypeContext<'_>,
    materialize_alias: bool,
    target: &mut Option<ResolvedTypeTarget>,
) -> Option<TypeId> {
    if let Some(imported) = context
        .declarations
        .imported_types()
        .resolved(resolved.clone())
    {
        *target = Some(ResolvedTypeTarget::Source(match &imported.declaration.id {
            DeclarationId::Definition(id) => *id,
            _ => return None,
        }));
        if !materialize_alias && alias_target(target.as_ref(), context.declarations).is_some() {
            return Some(TypeId::Error);
        }
        return Some(imported.ty.clone());
    }
    let definition = *context.declarations.definition(resolved.clone())?;
    if let ResolvedName::TypeAlias(_) = resolved {
        *target = Some(ResolvedTypeTarget::Source(definition));
        if !materialize_alias {
            return Some(TypeId::Error);
        }
        return Some(
            context
                .declarations
                .imported_types()
                .alias(&definition)?
                .target
                .clone(),
        );
    }
    let arguments = context
        .declarations
        .parameters_of(&definition)
        .into_iter()
        .map(TypeId::Generic)
        .collect::<Vec<_>>();
    Some(match resolved {
        ResolvedName::NativeType(id) => {
            *target = Some(ResolvedTypeTarget::NativeType(id));
            return native_type(target.clone(), context.declarations)?.apply(&arguments);
        }
        ResolvedName::Struct(id) => {
            *target = Some(ResolvedTypeTarget::Struct(id));
            TypeId::Struct(NominalType {
                associated_types: Default::default(),
                declaration: definition,
                arguments,
            })
        }
        ResolvedName::Enum(id) => {
            *target = Some(ResolvedTypeTarget::Enum(id));
            TypeId::Enum(NominalType {
                associated_types: Default::default(),
                declaration: definition,
                arguments,
            })
        }
        ResolvedName::Trait(id) => {
            *target = Some(ResolvedTypeTarget::Trait(id));
            TypeId::Trait(NominalType {
                associated_types: Default::default(),
                declaration: definition,
                arguments,
            })
        }
        _ => return None,
    })
}

fn resolve_reference(
    name: &str,
    exact: Option<(DefinitionId, Option<&DefinitionExposure>)>,
    context: TypeContext<'_>,
    cancel: &CancellationToken,
) -> ResolvedTypeRef {
    let Some((exact, exposure)) = exact else {
        return resolve_named_reference(name, context, false);
    };
    let selected = context
        .declarations
        .resolve_definition_exposure(exact, NameNamespace::Type, exposure, cancel)
        .ok()
        .flatten();
    let mut selected_exposure = None;
    let mut target = None;
    let ty = selected.and_then(|selection| {
        let resolved = selection
            .hit
            .target
            .resolved(context.declarations.names.unit.as_ref());
        let ty = resolve_declaration_reference(resolved, context, false, &mut target)?;
        selected_exposure = Some(selection.exposure);
        Some(ty)
    });
    ResolvedTypeRef {
        ty: ty.unwrap_or(TypeId::Error),
        target,
        exposure: selected_exposure,
    }
}

pub(super) fn resolve_type(
    module: &Module,
    ty: TypeRefId,
    declarations: &Declarations,
    table: &mut TypeCheckState,
    cancel: &CancellationToken,
) -> TypeId {
    resolve_type_in(
        module,
        ty,
        TypeContext {
            declarations,
            generics: &[],
            self_type: None,
            implementation: None,
        },
        table,
        cancel,
    )
}

/// Resolves a TypeRefId with explicit generic/Self context, recording facts and recovery diagnostics.
pub(super) fn resolve_type_in(
    module: &Module,
    ty: TypeRefId,
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
    cancel: &CancellationToken,
) -> TypeId {
    resolve_type_bounded(
        module,
        ty,
        context,
        table,
        cancel,
        &mut ExpansionBudget::new(4096),
    )
}

fn resolve_type_bounded(
    module: &Module,
    ty: TypeRefId,
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
    cancel: &CancellationToken,
    budget: &mut ExpansionBudget,
) -> TypeId {
    if let Some(inferred) = table.inference_holes.get(&ty) {
        let inferred = if let Some(size) = budget.check(inferred, &Default::default(), cancel) {
            budget.remaining -= size;
            inferred.clone()
        } else {
            TypeId::Error
        };
        table.insert_type_ref(
            ty,
            ResolvedTypeRef {
                ty: inferred.clone(),
                target: None,
                exposure: None,
            },
        );
        return inferred;
    }

    if cancel.check().is_err() || !table.resolving_types.insert(ty) {
        return TypeId::Error;
    }
    let context = if let HirOwner::Body(BodyOwner::Function(function)) = ty.owner() {
        TypeContext {
            self_type: context.self_type.or_else(|| {
                module
                    .traits
                    .iter()
                    .find(|item| {
                        item.methods
                            .iter()
                            .any(|method| method.function == function)
                    })
                    .map(|item| item.id)
            }),
            implementation: context.implementation.or_else(|| {
                module
                    .impls
                    .iter()
                    .find(|item| {
                        item.methods
                            .iter()
                            .any(|method| method.function == function)
                    })
                    .map(|item| item.id)
            }),
            ..context
        }
    } else {
        context
    };
    let mut target = None;
    let mut selected_exposure = None;
    let authored = &module.type_ref(ty).kind;
    let (normalized, exact) = if let TypeKind::Definition {
        declaration,
        name,
        exposure,
        args,
        bindings,
    } = authored
    {
        let kind = if args.is_empty() && bindings.is_empty() {
            TypeKind::Named(name.clone())
        } else {
            TypeKind::Generic {
                name: name.clone(),
                args: args.clone(),
                bindings: bindings.clone(),
                positional_after_binding: false,
                callable_syntax: false,
            }
        };
        (Some(kind), Some((*declaration, exposure.as_ref())))
    } else {
        (None, None)
    };
    let resolved = match normalized.as_ref().unwrap_or(authored) {
        TypeKind::Definition { .. } => unreachable!("normalized structured application"),
        TypeKind::Named(name)
            if exact.is_none() && name == "Self" && context.implementation.is_some() =>
        {
            module
                .impls
                .iter()
                .find(|item| Some(item.id) == context.implementation)
                .and_then(|item| item.for_type)
                .map(|receiver| {
                    resolve_type_argument(module, receiver, context, table, cancel, budget)
                })
                .unwrap_or(TypeId::Error)
        }
        TypeKind::Named(name) => {
            let reference = resolve_reference(name, exact, context, cancel);
            target = reference.target;
            selected_exposure = reference.exposure;
            if exact.is_none()
                && reference.ty == TypeId::Error
                && name.contains("::")
                && alias_target(target.as_ref(), context.declarations).is_none()
            {
                let resolved = associated::resolve_projection_name(
                    module,
                    name,
                    Vec::new(),
                    context,
                    table,
                    cancel,
                );
                target = resolved.target;
                resolved.ty
            } else {
                match &reference.ty {
                    _ if alias_target(target.as_ref(), context.declarations).is_some() => {
                        alias_target(target.as_ref(), context.declarations)
                            .expect("alias target")
                            .apply(&[], budget, cancel)
                    }
                    _ if native_type(target.clone(), context.declarations)
                        .is_some_and(|kind| kind.arity() != 0) =>
                    {
                        TypeId::Error
                    }
                    TypeId::NativeObject(ty)
                    | TypeId::Struct(ty)
                    | TypeId::Enum(ty)
                    | TypeId::Trait(ty)
                        if !ty.arguments.is_empty() =>
                    {
                        TypeId::Error
                    }
                    _ => reference.ty,
                }
            }
        }
        TypeKind::Generic {
            name,
            args,
            bindings,
            positional_after_binding,
            callable_syntax,
        } => {
            let reference = resolve_reference(name, exact, context, cancel);
            target = reference.target;
            selected_exposure = reference.exposure;
            // Visit every argument even if an earlier one cannot resolve.
            let args = args
                .iter()
                .map(|arg| resolve_type_argument(module, *arg, context, table, cancel, budget))
                .collect::<Vec<_>>();
            let resolved_bindings = bindings
                .iter()
                .map(|(name, ty)| {
                    (
                        name.clone(),
                        resolve_type_argument(module, *ty, context, table, cancel, budget),
                    )
                })
                .collect::<Vec<_>>();
            if exact.is_none()
                && reference.ty.is_unresolved()
                && name.contains("::")
                && bindings.is_empty()
                && alias_target(target.as_ref(), context.declarations).is_none()
            {
                let resolved =
                    associated::resolve_projection_name(module, name, args, context, table, cancel);
                target = resolved.target;
                resolved.ty
            } else if let Some(alias) = alias_target(target.as_ref(), context.declarations) {
                if bindings.is_empty() && !*callable_syntax && !*positional_after_binding {
                    alias.apply(&args, budget, cancel)
                } else {
                    TypeId::Error
                }
            } else {
                match native_type(target.clone(), context.declarations) {
                    Some(kind) if kind.arity() != 0 && bindings.is_empty() && !*callable_syntax => {
                        kind.apply(&args).unwrap_or(TypeId::Error)
                    }
                    Some(_) => TypeId::Error,
                    None => match reference.ty {
                        TypeId::Struct(mut nominal)
                            if !nominal.arguments.is_empty()
                                && nominal.arguments.len() == args.len()
                                && bindings.is_empty() =>
                        {
                            nominal.arguments = args;
                            TypeId::Struct(nominal)
                        }
                        TypeId::Enum(mut nominal)
                            if !nominal.arguments.is_empty()
                                && nominal.arguments.len() == args.len()
                                && bindings.is_empty() =>
                        {
                            nominal.arguments = args;
                            TypeId::Enum(nominal)
                        }
                        TypeId::Trait(mut nominal)
                            if nominal.arguments.len() == args.len()
                                && (!nominal.arguments.is_empty() || !bindings.is_empty())
                                && !*positional_after_binding
                                && (!*callable_syntax
                                    || Protocol::from_definition(
                                        &nominal.declaration,
                                        context.declarations.definitions(),
                                    ) == Some(Protocol::Fn)) =>
                        {
                            nominal.arguments = args;
                            let members = associated::members(
                                module,
                                context.declarations,
                                &nominal.declaration,
                            );
                            let mut valid = true;
                            for (name, value) in resolved_bindings {
                                let Some(id) = context.declarations.definitions().lookup_child(
                                    nominal.declaration,
                                    DefinitionKind::AssociatedType,
                                    &name,
                                    0,
                                ) else {
                                    valid = false;
                                    continue;
                                };
                                valid &= members.contains(&name)
                                    && associated::member_arity(
                                        module,
                                        context.declarations,
                                        &nominal.declaration,
                                        &name,
                                    ) == Some(0)
                                    && nominal.associated_types.insert(id, value).is_none();
                            }
                            if valid {
                                TypeId::Trait(nominal)
                            } else {
                                TypeId::Error
                            }
                        }
                        _ => TypeId::Error,
                    },
                }
            }
        }
        TypeKind::Projection {
            receiver,
            trait_ref,
            member,
            arguments,
        } => {
            let receiver = resolve_type_argument(module, *receiver, context, table, cancel, budget);
            let interface =
                resolve_type_argument(module, *trait_ref, context, table, cancel, budget);
            let arguments = arguments
                .iter()
                .map(|arg| resolve_type_argument(module, *arg, context, table, cancel, budget))
                .collect();
            let member_id = match &interface {
                TypeId::Trait(interface) => context.declarations.definitions().lookup_child(
                    interface.declaration,
                    DefinitionKind::AssociatedType,
                    member,
                    0,
                ),
                _ => None,
            };
            let resolved = associated::qualified_projection(
                module,
                receiver,
                interface,
                (member, arguments),
                context,
                table,
                cancel,
            );
            if resolved != TypeId::Error {
                target = member_id.map(ResolvedTypeTarget::AssociatedType);
            }
            resolved
        }
        TypeKind::Tuple(elements) => {
            if elements.is_empty() {
                TypeId::from_name("()").expect("unit builtin")
            } else {
                resolve_composite_members(
                    module,
                    elements.iter().copied(),
                    context,
                    table,
                    cancel,
                    budget,
                )
                .map_or(TypeId::Error, TypeId::Tuple)
            }
        }
        TypeKind::Array(element) => {
            resolve_composite_members(module, iter::once(*element), context, table, cancel, budget)
                .map_or(TypeId::Error, |mut members| {
                    TypeId::Array(Box::new(members.pop().expect("array element")))
                })
        }
        TypeKind::Function { params, result } => resolve_composite_members(
            module,
            params.iter().copied().chain(iter::once(*result)),
            context,
            table,
            cancel,
            budget,
        )
        .map_or(TypeId::Error, |mut members| {
            let result = Box::new(members.pop().expect("function result"));
            TypeId::Function {
                params: members,
                result,
            }
        }),
    };

    let resolved = if let Some(size) = budget.check(&resolved, &Default::default(), cancel) {
        budget.remaining -= size;
        resolved
    } else {
        TypeId::Error
    };
    table.resolving_types.remove(&ty);
    table.insert_type_ref(
        ty,
        ResolvedTypeRef {
            ty: resolved.clone(),
            target,
            exposure: selected_exposure,
        },
    );
    resolved
}

// Alias arguments can disappear from the final type, so they have independent
// shape limits. Once the enclosing composite is full, only recovery facts remain.
fn resolve_type_argument(
    module: &Module,
    ty: TypeRefId,
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
    cancel: &CancellationToken,
    enclosing: &ExpansionBudget,
) -> TypeId {
    let limit = if enclosing.exceeded || enclosing.remaining == 0 {
        0
    } else {
        4096
    };
    resolve_type_bounded(
        module,
        ty,
        context,
        table,
        cancel,
        &mut ExpansionBudget::new(limit),
    )
}

fn resolve_composite_members(
    module: &Module,
    members: impl Iterator<Item = TypeRefId>,
    context: TypeContext<'_>,
    table: &mut TypeCheckState,
    cancel: &CancellationToken,
    budget: &mut ExpansionBudget,
) -> Option<Vec<TypeId>> {
    let mut children = ExpansionBudget::new(budget.remaining.saturating_sub(1));
    let mut resolved = Vec::new();
    for member in members {
        let ty = resolve_type_bounded(module, member, context, table, cancel, &mut children);
        if !children.exceeded {
            resolved.push(ty);
        }
    }
    if children.exceeded {
        budget.exceeded = true;
        budget.remaining = 0;
        None
    } else {
        Some(resolved)
    }
}

pub(super) fn display_type(module: &Module, ty: TypeRefId) -> String {
    match &module.type_ref(ty).kind {
        TypeKind::Definition {
            name,
            args,
            bindings,
            ..
        } => {
            let mut arguments = args
                .iter()
                .map(|arg| display_type(module, *arg))
                .collect::<Vec<_>>();
            arguments.extend(
                bindings
                    .iter()
                    .map(|(name, ty)| format!("{name} = {}", display_type(module, *ty))),
            );
            if arguments.is_empty() {
                name.clone()
            } else {
                format!("{name}<{}>", arguments.join(", "))
            }
        }
        TypeKind::Named(name) => name.clone(),
        TypeKind::Generic {
            name,
            args,
            bindings,
            ..
        } => {
            let inner = args
                .iter()
                .map(|arg| display_type(module, *arg))
                .chain(
                    bindings
                        .iter()
                        .map(|(name, ty)| format!("{name} = {}", display_type(module, *ty))),
                )
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{inner}>")
        }
        TypeKind::Projection {
            receiver,
            trait_ref,
            member,
            arguments,
        } => {
            let suffix = if arguments.is_empty() {
                String::new()
            } else {
                format!(
                    "<{}>",
                    arguments
                        .iter()
                        .map(|arg| display_type(module, *arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            format!(
                "<{} as {}>::{member}{suffix}",
                display_type(module, *receiver),
                display_type(module, *trait_ref)
            )
        }
        TypeKind::Tuple(elements) => {
            let inner = elements
                .iter()
                .map(|element| display_type(module, *element))
                .collect::<Vec<_>>()
                .join(", ");
            if elements.len() == 1 {
                format!("({inner},)")
            } else {
                format!("({inner})")
            }
        }
        TypeKind::Array(element) => format!("[{}]", display_type(module, *element)),
        TypeKind::Function { params, result } => format!(
            "fn({}) -> {}",
            params
                .iter()
                .map(|param| display_type(module, *param))
                .collect::<Vec<_>>()
                .join(", "),
            display_type(module, *result),
        ),
    }
}

pub(crate) fn display_type_id(definitions: &DefinitionTable, ty: &TypeId) -> String {
    ty.display_name(definitions)
}
