use super::{ResolvedTypeRef, TypeTable, TypeTarget, associated};
use crate::{
    declarations::Declarations,
    hir,
    native::NativeTypeKind,
    resolver::ResolvedName,
    types::{NominalType, TypeId},
};
use hir::{BodyOwner, HirOwner, TypeKind};
use kagari_abi::{scalar::BuiltinType, standard::traits::StandardTrait};
use kagari_common::{cancellation::CancellationToken, identity};

#[derive(Debug, Clone, Copy)]
pub(super) struct TypeContext<'a> {
    pub declarations: &'a Declarations,
    pub generics: &'a [hir::GenericParam],
    pub self_type: Option<hir::TraitId>,
    pub implementation: Option<hir::ImplId>,
}

pub(super) fn native_type(
    target: Option<TypeTarget>,
    declarations: &Declarations,
) -> Option<NativeTypeKind> {
    match target? {
        TypeTarget::OpaqueType(id) => declarations.native_type(id),
        TypeTarget::Enum(id) => declarations.native_enum(id),
        TypeTarget::Source(id) => declarations.imported_types().target(id)?.native_type,
        _ => None,
    }
}

pub(super) fn resolve_named_type(name: &str, context: TypeContext<'_>) -> ResolvedTypeRef {
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
                .resolve_name(name)
                .and_then(|resolved| {
                    if let ResolvedName::HostType(id) = resolved {
                        target = Some(TypeTarget::Host(id));
                        return Some(TypeId::Host(
                            context.declarations.hosts.type_declaration(id)?.id.clone(),
                        ));
                    }
                    if let Some(imported) = context.declarations.imported_types().resolved(resolved)
                    {
                        target = Some(TypeTarget::Source(imported.id));
                        return Some(imported.ty.clone());
                    }
                    let definition = context.declarations.definition(resolved)?.clone();
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
                            if let Some(kind) = context.declarations.native_enum(id) {
                                return kind.apply(&arguments);
                            }
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
    module: &hir::Module,
    ty: hir::TypeRefId,
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

pub(super) fn resolve_type_in(
    module: &hir::Module,
    ty: hir::TypeRefId,
    context: TypeContext<'_>,
    table: &mut TypeTable,
    cancel: &CancellationToken,
) -> TypeId {
    if let Some(inferred) = table.inference_holes.get(&ty).cloned() {
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
            .map(|receiver| resolve_type_in(module, receiver, context, table, cancel))
            .unwrap_or(TypeId::Error),
        TypeKind::Named(name) => {
            let reference = resolve_named_type(name, context);
            target = reference.target;
            if reference.ty == TypeId::Error && name.contains("::") {
                let resolved = associated::resolve_projection_name(
                    module,
                    name,
                    Vec::new(),
                    context,
                    table,
                    cancel,
                );
                table.resolving_types.remove(&ty);
                table.insert_type_ref(
                    ty,
                    ResolvedTypeRef {
                        ty: resolved.clone(),
                        target: None,
                    },
                );
                return resolved;
            }
            match &reference.ty {
                _ if native_type(target, context.declarations)
                    .is_some_and(|kind| kind.arity() != 0) =>
                {
                    TypeId::Error
                }
                TypeId::Struct(ty) | TypeId::Enum(ty) | TypeId::Trait(ty)
                    if !ty.arguments.is_empty() =>
                {
                    TypeId::Error
                }
                _ => reference.ty,
            }
        }
        TypeKind::Generic {
            name,
            args,
            bindings,
            positional_after_binding,
            callable_syntax,
        } => {
            let reference = resolve_named_type(name, context);
            target = reference.target;
            // Visit every argument even if an earlier one cannot resolve.
            let args = args
                .iter()
                .map(|arg| resolve_type_in(module, *arg, context, table, cancel))
                .collect::<Vec<_>>();
            let resolved_bindings = bindings
                .iter()
                .map(|(name, ty)| {
                    (
                        name.clone(),
                        resolve_type_in(module, *ty, context, table, cancel),
                    )
                })
                .collect::<Vec<_>>();
            if reference.ty.is_unresolved() && name.contains("::") && bindings.is_empty() {
                let resolved =
                    associated::resolve_projection_name(module, name, args, context, table, cancel);
                table.resolving_types.remove(&ty);
                table.insert_type_ref(
                    ty,
                    ResolvedTypeRef {
                        ty: resolved.clone(),
                        target: None,
                    },
                );
                return resolved;
            }
            match native_type(target, context.declarations) {
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
                                || StandardTrait::from_id(&nominal.declaration)
                                    == Some(StandardTrait::Fn)) =>
                    {
                        nominal.arguments = args;
                        let members =
                            associated::members(module, context.declarations, &nominal.declaration);
                        let mut valid = true;
                        for (name, value) in resolved_bindings {
                            let id = identity::associated_type_id(&nominal.declaration, &name);
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
        TypeKind::Projection {
            receiver,
            trait_ref,
            member,
            arguments,
        } => {
            let receiver = resolve_type_in(module, *receiver, context, table, cancel);
            let interface = resolve_type_in(module, *trait_ref, context, table, cancel);
            let arguments = arguments
                .iter()
                .map(|arg| resolve_type_in(module, *arg, context, table, cancel))
                .collect();
            associated::qualified_projection(
                module,
                receiver,
                interface,
                (member, arguments),
                context,
                table,
                cancel,
            )
        }
        TypeKind::Tuple(elements) => {
            let elements = elements
                .iter()
                .map(|element| resolve_type_in(module, *element, context, table, cancel))
                .collect::<Vec<_>>();
            if elements.is_empty() {
                TypeId::from_name("()").expect("unit builtin")
            } else {
                TypeId::Tuple(elements)
            }
        }
        TypeKind::Array(element) => {
            let mut interface = StandardTrait::List.nominal();
            interface
                .arguments
                .push(resolve_type_in(module, *element, context, table, cancel));
            TypeId::Trait(interface)
        }
        TypeKind::Function { params, result } => TypeId::Function {
            params: params
                .iter()
                .map(|param| resolve_type_in(module, *param, context, table, cancel))
                .collect(),
            result: Box::new(resolve_type_in(module, *result, context, table, cancel)),
        },
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
pub(super) fn display_type(module: &hir::Module, ty: hir::TypeRefId) -> String {
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

pub(crate) fn display_type_id(ty: &TypeId) -> String {
    ty.display_name()
}
