//! The cold single-drive Future contract; runtime storage is installed separately.
use kagari_common::identity::DefinitionKind;
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout},
    ty::GenericParam,
};

pub(super) fn declare(module: &mut ModuleDecl) {
    module.types.push(TypeDef {
        name: "Future".into(),
        kind: TypeDefKind::NativeStorage(NativeStorageLayout::Future),
        generic_params: vec![GenericParam {
            owner: module.definition(DefinitionKind::AssociatedType, "Future"),
            position: 0,
        }],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
}
