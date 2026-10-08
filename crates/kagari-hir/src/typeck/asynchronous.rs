//! Installed async storage roles, independent of source names and aliases.

use crate::{
    declarations::Declarations,
    native::NativeTypeKind,
    types::{NominalType, TypeId},
};
use kagari_common::identity::DefinitionPath;
use kagari_types::declaration::native::NativeStorageLayout;

fn storage_declaration(
    declarations: &Declarations,
    role: NativeStorageLayout,
) -> Option<&DefinitionPath> {
    let mut found = None;
    for representation in declarations.native_types() {
        if let NativeTypeKind::Storage {
            declaration,
            arity: 1,
            layout,
        } = representation
            && *layout == role
        {
            if found.is_some_and(|previous| previous != declaration) {
                return None;
            }
            found = Some(declaration);
        }
    }
    found
}

pub(super) fn future_type(declarations: &Declarations, output: TypeId) -> Option<TypeId> {
    Some(TypeId::NativeObject(NominalType {
        declaration: storage_declaration(declarations, NativeStorageLayout::Future)?.clone(),
        arguments: vec![output],
        associated_types: Default::default(),
    }))
}

pub(super) fn future_output<'a>(declarations: &Declarations, ty: &'a TypeId) -> Option<&'a TypeId> {
    storage_output(declarations, ty, NativeStorageLayout::Future)
}

pub(super) fn await_output<'a>(declarations: &Declarations, ty: &'a TypeId) -> Option<&'a TypeId> {
    future_output(declarations, ty)
        .or_else(|| storage_output(declarations, ty, NativeStorageLayout::Task))
}

fn storage_output<'a>(
    declarations: &Declarations,
    ty: &'a TypeId,
    role: NativeStorageLayout,
) -> Option<&'a TypeId> {
    let TypeId::NativeObject(nominal) = ty else {
        return None;
    };
    (Some(&nominal.declaration) == storage_declaration(declarations, role)
        && nominal.arguments.len() == 1
        && nominal.associated_types.is_empty())
    .then(|| &nominal.arguments[0])
}
