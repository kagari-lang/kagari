//! Scope and Task storage roles are sealed runtime-owned capabilities.
use kagari_common::identity::DefinitionKind;
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout},
    ty::GenericParam,
};

pub(super) fn declare(module: &mut ModuleDecl) {
    for (name, layout) in [
        ("Task", NativeStorageLayout::Task),
        ("TaskScope", NativeStorageLayout::TaskScope),
    ] {
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::NativeStorage(layout),
            generic_params: if layout == NativeStorageLayout::Task {
                vec![GenericParam {
                    owner: module.definition(DefinitionKind::AssociatedType, name),
                    position: 0,
                }]
            } else {
                vec![]
            },
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
    }
}
