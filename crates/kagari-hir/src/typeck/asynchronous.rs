//! The installed Future storage role, independent of source names and aliases.

use crate::{
    declarations::Declarations,
    native::NativeTypeKind,
    types::{NominalType, TypeId},
};
use kagari_common::identity::DefinitionPath;
use kagari_types::declaration::native::NativeStorageLayout;

fn future_declaration(declarations: &Declarations) -> Option<&DefinitionPath> {
    let mut found = None;
    for representation in declarations.native_types() {
        if let NativeTypeKind::Storage {
            declaration,
            arity: 1,
            layout: NativeStorageLayout::Future,
        } = representation
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
        declaration: future_declaration(declarations)?.clone(),
        arguments: vec![output],
        associated_types: Default::default(),
    }))
}

pub(super) fn future_output<'a>(declarations: &Declarations, ty: &'a TypeId) -> Option<&'a TypeId> {
    let TypeId::NativeObject(nominal) = ty else {
        return None;
    };
    (Some(&nominal.declaration) == future_declaration(declarations)
        && nominal.arguments.len() == 1
        && nominal.associated_types.is_empty())
    .then(|| &nominal.arguments[0])
}
