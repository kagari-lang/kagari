use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
};
use kagari_ir::{
    bytecode::{BytecodeModule, StructId},
    module::{StructFieldLayout, StructLayout, abi::AbiType},
};
use kagari_runtime::{Runtime, module::StructLayoutRef};

pub fn layout(
    runtime: &mut Runtime,
    name: &str,
    fields: &[(&str, AbiType, bool)],
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
                ty: ty.clone(),
                mutable: *mutable,
            }
        })
        .collect();
    let module = runtime
        .load_program(
            name,
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    structures: vec![StructLayout {
                        arguments: Vec::new(),
                        declaration,
                        fields,
                    }],
                    ..Default::default()
                }],
            },
        )
        .unwrap();
    module.struct_layout(StructId::new(0)).unwrap()
}
