use crate::{builtin::surface, hir, types::TypeId};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct TypeContext<'a> {
    pub generics: &'a [String],
}

pub(super) fn resolve_type(module: &hir::Module, ty: hir::TypeRefId) -> Option<TypeId> {
    resolve_type_in(module, ty, TypeContext::default())
}

pub(super) fn resolve_type_in(
    module: &hir::Module,
    ty: hir::TypeRefId,
    context: TypeContext<'_>,
) -> Option<TypeId> {
    match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => context
            .generics
            .iter()
            .find(|generic| *generic == name)
            .map(|_| TypeId::Generic(name.clone()))
            .or_else(|| TypeId::from_name(name))
            .or_else(|| {
                module
                    .structs
                    .iter()
                    .find(|item| item.name == *name)
                    .map(|_| TypeId::Struct(name.clone()))
            })
            .or_else(|| {
                module
                    .enums
                    .iter()
                    .find(|item| item.name == *name)
                    .map(|_| TypeId::Enum(name.clone()))
            })
            .or_else(|| {
                module
                    .traits
                    .iter()
                    .find(|item| item.name == *name)
                    .map(|_| TypeId::Trait(name.clone()))
            }),
        hir::TypeKind::Generic { name, args } => {
            let args = args
                .iter()
                .map(|arg| resolve_type_in(module, *arg, context))
                .collect::<Option<Vec<_>>>()?;
            surface::standard_enum_type(name, args)
        }
        hir::TypeKind::Tuple(elements) => elements
            .iter()
            .map(|element| resolve_type_in(module, *element, context))
            .collect::<Option<Vec<_>>>()
            .map(|elements| {
                if elements.is_empty() {
                    TypeId::from_name("()").expect("unit builtin should exist")
                } else {
                    TypeId::Tuple(elements)
                }
            }),
        hir::TypeKind::Array(element) => resolve_type_in(module, *element, context)
            .map(|element| TypeId::Array(Box::new(element))),
    }
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
