//! Nominal Vec identity and allocation for the Rust collection adapter.
use crate::{
    Runtime, error::RuntimeError, frame::types::arguments::TypeArgument, module::ExecutableModule,
    native::binding::NativeResult,
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
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
        owner: &ExecutableModule,
        element: &TypeArgument,
    ) -> NativeResult<TypeArgument> {
        let id = self
            .definition_context()
            .snapshot()
            .lookup(&declaration())
            .ok_or_else(|| RuntimeError::module_validation("Vec declaration unavailable"))?;
        element.derive(self, owner, |item| {
            Some(Ty::NativeObject(NominalTy {
                declaration: id,
                arguments: vec![item.clone()],
                associated_types: Default::default(),
            }))
        })
    }
}
