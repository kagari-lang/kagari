//! Language-owned async declaration identities, independent of installed storage.

use crate::{
    declarations::Declarations,
    types::{NominalType, TypeId},
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, table::DefinitionId};

fn storage_declaration(declarations: &Declarations, name: &str) -> Option<DefinitionId> {
    let module = if name == "Future" { "future" } else { "task" };
    let identity = ModuleIdentity {
        package: PackageId("kagari-core".into()),
        path: vec![module.into()],
    };
    let definitions = declarations.definitions();
    definitions
        .lookup_root(&identity)
        .and_then(|root| definitions.lookup_child(root, DefinitionKind::AssociatedType, name, 0))
}

pub(super) fn future_type(declarations: &Declarations, output: TypeId) -> Option<TypeId> {
    Some(TypeId::NativeObject(NominalType {
        declaration: storage_declaration(declarations, "Future")?,
        arguments: vec![output],
        associated_types: Default::default(),
    }))
}

pub(super) fn future_output<'a>(declarations: &Declarations, ty: &'a TypeId) -> Option<&'a TypeId> {
    storage_output(declarations, ty, "Future")
}

pub(super) fn await_output<'a>(declarations: &Declarations, ty: &'a TypeId) -> Option<&'a TypeId> {
    future_output(declarations, ty).or_else(|| storage_output(declarations, ty, "Task"))
}

fn storage_output<'a>(
    declarations: &Declarations,
    ty: &'a TypeId,
    role: &str,
) -> Option<&'a TypeId> {
    let TypeId::NativeObject(nominal) = ty else {
        return None;
    };
    (Some(nominal.declaration) == storage_declaration(declarations, role)
        && nominal.arguments.len() == 1
        && nominal.associated_types.is_empty())
    .then(|| &nominal.arguments[0])
}
