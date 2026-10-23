//! Translate HIR type spelling into semantic types using declaration/generic context.
//! Resolved types and target identities are recorded by TypeRefId for later queries;
//! invalid or incomplete spelling produces diagnostics and recovery types.

use crate::{
    declarations::{DeclarationId, Declarations},
    hir::{
        ids::{BodyOwner, HirOwner, ImplId, TraitId, TypeRefId},
        item::{Module, behavior::GenericParam},
        ty::TypeKind,
    },
    imports::types::aliases::CanonicalAlias,
    native::NativeTypeKind,
    resolver::resolved::ResolvedName,
    typeck::{
        aliases::ExpansionBudget,
        associated,
        table::{ResolvedTypeRef, TypeTable, TypeTarget},
    },
    types::{NominalType, TypeId},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_types::{
    collection::CollectionAccess, declaration::names::NameNamespace, language::Protocol,
    scalar::BuiltinType,
};
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
    target: Option<&TypeTarget>,
    declarations: &'a Declarations,
) -> Option<&'a CanonicalAlias> {
    let TypeTarget::Source(id) = target? else {
        return None;
    };
    declarations.imported_types().alias(id)
}

pub(super) fn native_type(
    target: Option<TypeTarget>,
    declarations: &Declarations,
) -> Option<NativeTypeKind> {
    match target? {
        TypeTarget::OpaqueType(id) => declarations.native_type(id),
        TypeTarget::Source(id) => declarations
            .imported_types()
            .by_declaration(&id)?
            .native_type
            .clone(),
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
            target = Some(TypeTarget::Generic(param.id));
            context
                .declarations
                .generic_type(param.id)
                .map(TypeId::Generic)
        } else if name == "Self" && context.self_type.is_some() {
            target = context.self_type.map(TypeTarget::Trait);
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
                    if let Some(imported) = context
                        .declarations
                        .imported_types()
                        .resolved(resolved.clone())
                    {
                        target = Some(TypeTarget::Source(match &imported.declaration.id {
                            DeclarationId::Definition(id) => *id,
                            _ => return None,
                        }));
                        if !materialize_alias
                            && alias_target(target.as_ref(), context.declarations).is_some()
                        {
                            return Some(TypeId::Error);
                        }
                        return Some(imported.ty.clone());
                    }
                    let definition = *context.declarations.definition(resolved.clone())?;
                    if let ResolvedName::TypeAlias(_) = resolved {
                        target = Some(TypeTarget::Source(definition));
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
                        ResolvedName::OpaqueType(id) => {
                            target = Some(TypeTarget::OpaqueType(id));
                            return context.declarations.native_type(id)?.apply(&arguments);
                        }
                        ResolvedName::Struct(id) => {
                            target = Some(TypeTarget::Struct(id));
                            TypeId::Struct(NominalType {
                                associated_types: Default::default(),
                                declaration: definition,
                                arguments,
                            })
                        }
                        ResolvedName::Enum(id) => {
                            target = Some(TypeTarget::Enum(id));
                            TypeId::Enum(NominalType {
                                associated_types: Default::default(),
                                declaration: definition,
                                arguments,
                            })
                        }
                        ResolvedName::Trait(id) => {
                            target = Some(TypeTarget::Trait(id));
                            TypeId::Trait(NominalType {
                                associated_types: Default::default(),
                                declaration: definition,
                                arguments,
                            })
                        }
                        _ => return None,
                    })
                })
        }
    })();
    ResolvedTypeRef {
        ty: ty.unwrap_or(TypeId::Error),
        target,
    }
}

pub(super) fn resolve_type(
    module: &Module,
    ty: TypeRefId,
    declarations: &Declarations,
    table: &mut TypeTable,
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
    table: &mut TypeTable,
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
    table: &mut TypeTable,
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
    let resolved = match &module.type_ref(ty).kind {
        TypeKind::Named(name) if name == "Self" && context.implementation.is_some() => module
            .impls
            .iter()
            .find(|item| Some(item.id) == context.implementation)
            .and_then(|item| item.for_type)
            .map(|receiver| resolve_type_argument(module, receiver, context, table, cancel, budget))
            .unwrap_or(TypeId::Error),
        TypeKind::Named(name) => {
            let reference = resolve_named_reference(name, context, false);
            target = reference.target;
            if reference.ty == TypeId::Error
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
            let reference = resolve_named_reference(name, context, false);
            target = reference.target;
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
            if reference.ty.is_unresolved()
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
                target = member_id.map(TypeTarget::AssociatedType);
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
    let resolved = if context.declarations.native_readonly_type(ty) {
        match resolved {
            TypeId::Map { key, value, .. } => TypeId::Map {
                key,
                value,
                access: CollectionAccess::ReadOnly,
            },
            TypeId::Set(item, _) => TypeId::Set(item, CollectionAccess::ReadOnly),
            _ => TypeId::Error,
        }
    } else {
        resolved
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
    table: &mut TypeTable,
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
    table: &mut TypeTable,
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
