//! Lower nominal declarations once, independently of public export authority.

use crate::source::{
    lower::abi::{abi_parameters, abi_type, checked_bounds},
    types::lower_native_kind,
};
use kagari_hir::{
    compile_input::program::CheckedModule, hir::item::Item, native::NativeTypeKind,
    resolver::resolved::ResolvedName,
};
use kagari_types::{
    declaration::{FieldDef, TypeDeclaration, TypeDef, TypeDefKind, VariantDef},
    visibility::Visibility,
};

pub(super) fn lower(module: &CheckedModule<'_>, item: Item) -> (Visibility, TypeDeclaration) {
    let headers = module.headers();
    let environment = module.environment();
    let definitions = environment.definitions();
    let catalog = environment.aggregates();
    let identity = |name| {
        environment
            .definition(name)
            .expect("admitted nominal identity")
    };
    let (declaration, visibility, definition) = match item {
        Item::Struct(id) => {
            let item = &headers.structs[id.index()];
            let declaration = identity(ResolvedName::Struct(id));
            let signature = catalog
                .structure(&declaration)
                .expect("admitted struct contract");
            (
                declaration,
                item.visibility,
                TypeDef {
                    name: item.name.clone(),
                    kind: TypeDefKind::Struct,
                    generic_params: abi_parameters(&signature.generic_params),
                    bounds: checked_bounds(catalog, definitions, &signature.bounds),
                    fields: signature
                        .fields
                        .iter()
                        .map(|field| FieldDef {
                            host_policy: None,
                            visibility: field.visibility,
                            name: field.name.clone(),
                            ty: abi_type(module, &field.ty),
                            mutable: field.writeability.is_var(),
                        })
                        .collect(),
                    variants: Vec::new(),
                },
            )
        }
        Item::Enum(id) => {
            let item = &headers.enums[id.index()];
            let declaration = identity(ResolvedName::Enum(id));
            let signature = catalog
                .enumeration(&declaration)
                .expect("admitted enum contract");
            (
                declaration,
                item.visibility,
                TypeDef {
                    name: item.name.clone(),
                    kind: TypeDefKind::Enum,
                    generic_params: abi_parameters(&signature.generic_params),
                    bounds: checked_bounds(catalog, definitions, &signature.bounds),
                    fields: Vec::new(),
                    variants: signature
                        .variants
                        .iter()
                        .map(|variant| VariantDef {
                            reports_failure: variant.reports_failure,
                            name: variant.name.clone(),
                            payload: variant
                                .payload
                                .iter()
                                .map(|ty| abi_type(module, ty))
                                .collect(),
                        })
                        .collect(),
                },
            )
        }
        Item::OpaqueType(id) => {
            let item = &headers.opaque_types[id.index()];
            let declaration = identity(ResolvedName::OpaqueType(id));
            let signature = catalog
                .native_type(&declaration)
                .expect("admitted native type contract");
            let definition =
                if let NativeTypeKind::Host { definition, .. } = &signature.representation {
                    // Installed host fields and policies belong to the registration contract.
                    definition.as_ref().clone()
                } else {
                    TypeDef {
                        name: item.name.clone(),
                        kind: lower_native_kind(signature.representation.clone()),
                        generic_params: abi_parameters(&signature.generic_params),
                        bounds: checked_bounds(catalog, definitions, &signature.bounds),
                        fields: Vec::new(),
                        variants: Vec::new(),
                    }
                };
            (declaration, item.visibility, definition)
        }
        _ => unreachable!("nominal type declaration"),
    };
    (
        visibility,
        TypeDeclaration {
            declaration,
            definition,
        },
    )
}
