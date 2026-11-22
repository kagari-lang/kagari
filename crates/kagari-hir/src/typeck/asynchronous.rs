//! Language-owned async declaration identities, independent of installed storage.

use crate::{
    declarations::Declarations,
    types::{NominalType, TypeId},
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, table::DefinitionId};
use kagari_types::language::binding;

fn task_declaration(declarations: &Declarations) -> Option<DefinitionId> {
    let identity = ModuleIdentity {
        package: PackageId("kagari-core".into()),
        path: vec!["task".into()],
    };
    let definitions = declarations.definitions();
    definitions
        .lookup_root(&identity)
        .and_then(|root| definitions.lookup_child(root, DefinitionKind::AssociatedType, "Task", 0))
}

pub(super) fn future_type(declarations: &Declarations, output: TypeId) -> Option<TypeId> {
    Some(TypeId::NativeObject(NominalType {
        declaration: declarations
            .definitions()
            .lookup(&binding::future_declaration())?,
        arguments: vec![output],
        associated_types: Default::default(),
    }))
}

pub(super) fn future_output<'a>(declarations: &Declarations, ty: &'a TypeId) -> Option<&'a TypeId> {
    storage_output(ty, || {
        declarations
            .definitions()
            .lookup(&binding::future_declaration())
    })
}

pub(super) fn await_output<'a>(declarations: &Declarations, ty: &'a TypeId) -> Option<&'a TypeId> {
    future_output(declarations, ty)
        .or_else(|| storage_output(ty, || task_declaration(declarations)))
}

fn storage_output(
    ty: &TypeId,
    declaration: impl FnOnce() -> Option<DefinitionId>,
) -> Option<&TypeId> {
    let TypeId::NativeObject(nominal) = ty else {
        return None;
    };
    (nominal.arguments.len() == 1
        && nominal.associated_types.is_empty()
        && nominal.declaration == declaration()?)
    .then(|| &nominal.arguments[0])
}
