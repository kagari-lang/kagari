//! The cold single-drive Future contract; runtime storage is installed separately.
use crate::catalog::identities;
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::{
        TypeDef, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeStorageLayout,
    },
    ty::GenericParam,
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    module.types.push(TypeDef {
        name: "Future".into(),
        kind: TypeDefKind::NativeStorage(NativeStorageLayout::Future),
        generic_params: vec![GenericParam {
            owner: identities::definition(ids, DefinitionKind::AssociatedType, "Future")?,
            position: 0,
        }],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });

    Ok(())
}
