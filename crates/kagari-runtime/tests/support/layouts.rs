use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
};
use kagari_ir::{
    bytecode::{BytecodeModule, StructId},
    module::{StructFieldLayout, StructLayout, ValueType},
};
use kagari_runtime::{Runtime, module::StructLayoutRef};

pub fn layout(
    runtime: &mut Runtime,
    name: &str,
    fields: &[(&str, ValueType, bool)],
) -> StructLayoutRef {
    let declaration = DefinitionId {
        module: ModuleIdentity::single_file("layout-fixture.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Struct,
            name: name.into(),
            occurrence: 0,
        }],
    };
    let fields = fields
        .iter()
        .map(|(name, ty, mutable)| {
            let mut id = declaration.clone();
            id.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Field,
                name: (*name).into(),
                occurrence: 0,
            });
            StructFieldLayout {
                declaration: id,
                name: (*name).into(),
                ty: *ty,
                mutable: *mutable,
            }
        })
        .collect();
    let module = runtime
        .load_module(
            name,
            BytecodeModule {
                structures: vec![StructLayout {
                    declaration,
                    fields,
                }],
                ..Default::default()
            },
        )
        .unwrap();
    module.struct_layout(StructId::new(0)).unwrap()
}
