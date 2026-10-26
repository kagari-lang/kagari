//! Lower nominal declarations once, independently of public export authority.

use crate::source::{
    lower::abi::{abi_type, checked_bounds, generic_param_abi, parameter_bounds},
    types::lower_native_kind,
};
use kagari_hir::{
    AnalyzedModule, declarations::DeclarationId, hir::item::Item, native::NativeTypeKind,
    resolver::resolved::ResolvedName,
};
use kagari_types::{
    declaration::{FieldDef, TypeDeclaration, TypeDef, TypeDefKind, VariantDef},
    visibility::Visibility,
};

pub(super) fn lower(module: &AnalyzedModule, item: Item) -> (Visibility, TypeDeclaration) {
    let hir = &module.lowered.module;
    let (name, visibility, definition) = match item {
        Item::Struct(id) => {
            let item = hir
                .structs
                .iter()
                .find(|item| item.id == id)
                .expect("checked struct declaration");
            (
                ResolvedName::Struct(id),
                item.visibility,
                TypeDef {
                    name: item.name.clone(),
                    kind: TypeDefKind::Struct,
                    generic_params: generic_param_abi(module, &item.generic_params),
                    bounds: parameter_bounds(module, &item.generic_params),
                    fields: item
                        .fields
                        .iter()
                        .map(|field| FieldDef {
                            host_policy: None,
                            visibility: field.visibility,
                            name: field.name.clone(),
                            ty: abi_type(
                                module,
                                &module
                                    .typed
                                    .type_table
                                    .field_type(field.id)
                                    .expect("checked field type must exist"),
                            ),
                            mutable: field.writeability.is_var(),
                        })
                        .collect(),
                    variants: Vec::new(),
                },
            )
        }
        Item::Enum(id) => {
            let item = hir
                .enums
                .iter()
                .find(|item| item.id == id)
                .expect("checked enum declaration");
            (
                ResolvedName::Enum(id),
                item.visibility,
                TypeDef {
                    name: item.name.clone(),
                    kind: TypeDefKind::Enum,
                    generic_params: generic_param_abi(module, &item.generic_params),
                    bounds: parameter_bounds(module, &item.generic_params),
                    fields: Vec::new(),
                    variants: item
                        .variants
                        .iter()
                        .map(|variant| VariantDef {
                            reports_failure: module.lowered.registered_enum_failure(variant.id),
                            name: variant.name.clone(),
                            payload: variant
                                .payload
                                .iter()
                                .map(|ty| {
                                    abi_type(
                                        module,
                                        &module
                                            .typed
                                            .type_table
                                            .type_ref(*ty)
                                            .expect("checked enum payload type must exist")
                                            .ty,
                                    )
                                })
                                .collect(),
                        })
                        .collect(),
                },
            )
        }
        Item::OpaqueType(id) => {
            let item = hir
                .opaque_types
                .iter()
                .find(|item| item.id == id)
                .expect("checked native type declaration");
            let representation = module
                .declarations
                .native_type(id)
                .expect("checked native type representation");
            let definition = if let NativeTypeKind::Host { definition, .. } = representation {
                // Registered host fields and policies belong to the installed contract,
                // not to the opaque source facade or a reconstructed Rust layout.
                definition.as_ref().clone()
            } else {
                let DeclarationId::Definition(declaration) = module
                    .declarations
                    .target(ResolvedName::OpaqueType(id))
                    .expect("checked native type identity")
                    .id
                else {
                    unreachable!("checked native nominal identity")
                };
                let signature = module
                    .aggregates
                    .native_type(&declaration)
                    .expect("checked native type contract");
                TypeDef {
                    name: item.name.clone(),
                    kind: lower_native_kind(representation),
                    generic_params: generic_param_abi(module, &item.generic_params),
                    bounds: checked_bounds(
                        &module.aggregates,
                        module.declarations.definitions(),
                        &signature.bounds,
                    ),
                    fields: Vec::new(),
                    variants: Vec::new(),
                }
            };
            (ResolvedName::OpaqueType(id), item.visibility, definition)
        }
        _ => unreachable!("nominal type declaration"),
    };
    let DeclarationId::Definition(declaration) = module
        .declarations
        .target(name)
        .expect("checked nominal declaration")
        .id
    else {
        unreachable!("checked nominal identity")
    };
    (
        visibility,
        TypeDeclaration {
            declaration,
            definition,
        },
    )
}
