use super::{ResolvedTypeRef, TypeTable, TypeTarget};
use crate::{builtin::surface, hir, types::TypeId};
use kagari_common::cancellation::CancellationToken;

#[derive(Debug, Clone, Copy)]
pub(super) struct TypeContext<'a> {
    pub declarations: &'a crate::declarations::Declarations,
    pub generics: &'a [hir::GenericParam],
    pub self_type: Option<hir::TraitId>,
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
                .definition(crate::resolver::ResolvedName::Trait(context.self_type?))
                .cloned()
                .map(TypeId::SelfType)
        } else if let Some(ty) = TypeId::from_name(name) {
            Some(ty)
        } else if let Some(binding) = context.declarations.names.lookup(name) {
            binding.target().and_then(|resolved| {
                use crate::resolver::ResolvedName;
                if let ResolvedName::HostType(id) = resolved {
                    target = Some(TypeTarget::Host(id));
                    return Some(TypeId::Host(
                        context.declarations.hosts.type_declaration(id)?.id.clone(),
                    ));
                }
                if let Some(imported) = context.declarations.imported_types().resolved(resolved) {
                    target = Some(TypeTarget::Source(imported.id));
                    return Some(imported.ty.clone());
                }
                let definition = context.declarations.definition(resolved)?.clone();
                let arguments = context
                    .declarations
                    .parameters_of(&definition)
                    .into_iter()
                    .map(TypeId::Generic)
                    .collect();
                Some(match resolved {
                    ResolvedName::Struct(id) => {
                        target = Some(TypeTarget::Struct(id));
                        TypeId::Struct(crate::types::NominalType {
                            declaration: definition,
                            arguments,
                        })
                    }
                    ResolvedName::Enum(id) => {
                        target = Some(TypeTarget::Enum(id));
                        TypeId::Enum(crate::types::NominalType {
                            declaration: definition,
                            arguments,
                        })
                    }
                    ResolvedName::Trait(id) => {
                        target = Some(TypeTarget::Trait(id));
                        TypeId::Trait(crate::types::NominalType {
                            declaration: definition,
                            arguments,
                        })
                    }
                    _ => return None,
                })
            })
        } else if let Some(imported) = context.declarations.imported_types().get(name) {
            target = Some(TypeTarget::Source(imported.id));
            Some(imported.ty.clone())
        } else if let Some(id) = context.declarations.host_type(name) {
            target = Some(TypeTarget::Host(id));
            Some(TypeId::Host(
                context.declarations.hosts.type_declaration(id)?.id.clone(),
            ))
        } else {
            None
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
    declarations: &crate::declarations::Declarations,
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
    if cancel.check().is_err() {
        return TypeId::Error;
    }
    let mut target = None;
    let resolved = match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => {
            let reference = resolve_named_type(name, context);
            target = reference.target;
            match &reference.ty {
                TypeId::Struct(ty) | TypeId::Enum(ty) | TypeId::Trait(ty)
                    if !ty.arguments.is_empty() =>
                {
                    TypeId::Error
                }
                _ => reference.ty,
            }
        }
        hir::TypeKind::Generic { name, args } => {
            let reference = resolve_named_type(name, context);
            target = reference.target;
            // An application has the same base binding as a named annotation.
            // A declaration, binder or unresolved import blocks prelude fallback.
            let prelude = reference.target.is_none()
                && reference.ty.is_unresolved()
                && context.declarations.names.lookup(name).is_none();
            // Visit every argument even if an earlier one cannot resolve.
            let args = args
                .iter()
                .map(|arg| resolve_type_in(module, *arg, context, table, cancel))
                .collect::<Vec<_>>();
            match reference.ty {
                TypeId::Struct(mut nominal)
                    if !nominal.arguments.is_empty() && nominal.arguments.len() == args.len() =>
                {
                    nominal.arguments = args;
                    TypeId::Struct(nominal)
                }
                TypeId::Enum(mut nominal)
                    if !nominal.arguments.is_empty() && nominal.arguments.len() == args.len() =>
                {
                    nominal.arguments = args;
                    TypeId::Enum(nominal)
                }
                TypeId::Trait(mut nominal)
                    if !nominal.arguments.is_empty() && nominal.arguments.len() == args.len() =>
                {
                    nominal.arguments = args;
                    TypeId::Trait(nominal)
                }
                _ if prelude => surface::standard_generic_type(name, args).unwrap_or(TypeId::Error),
                _ => TypeId::Error,
            }
        }
        hir::TypeKind::Tuple(elements) => {
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
        hir::TypeKind::Array(element) => TypeId::Array(Box::new(resolve_type_in(
            module, *element, context, table, cancel,
        ))),
    };
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
        hir::TypeKind::Named(name) => name.clone(),
        hir::TypeKind::Generic { name, args } => {
            let inner = args
                .iter()
                .map(|arg| display_type(module, *arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{inner}>")
        }
        hir::TypeKind::Tuple(elements) => {
            let inner = elements
                .iter()
                .map(|element| display_type(module, *element))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
        hir::TypeKind::Array(element) => format!("[{}]", display_type(module, *element)),
    }
}

pub(crate) fn display_type_id(ty: &TypeId) -> String {
    ty.display_name()
}
