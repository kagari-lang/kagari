//! Nominal Vec identity and allocation for the Rust collection adapter.
use crate::{
    Runtime, error::RuntimeError, frame::types::arguments::TypeArgument, module::LoadedModule,
    native::binding::NativeResult,
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
    reference::DefinitionReference,
};
use kagari_types::ty::{NominalTy, Ty};

pub(crate) fn declaration() -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity {
            package: PackageId("kagari-alloc".into()),
            path: vec!["vec".into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::AssociatedType,
            name: "Vec".into(),
            occurrence: 0,
        }],
    }
}

impl Runtime {
    pub(crate) fn vec_type(
        &self,
        owner: &LoadedModule,
        element: &TypeArgument,
    ) -> NativeResult<TypeArgument> {
        let id = declaration()
            .resolve(&self.definition_context().snapshot())
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        element.derive(self, owner, |item| {
            Some(Ty::NativeObject(NominalTy {
                declaration: id,
                arguments: vec![item.clone()],
                associated_types: Default::default(),
            }))
        })
    }
}
