use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
};
use kagari_ir::{
    bytecode::{BytecodeModule, StructId},
    module::{StructFieldLayout, StructLayout, abi::AbiType},
};
use kagari_runtime::{Runtime, module::StructLayoutRef, value::Value};

#[allow(dead_code)] // Shared support module is also compiled by integration tests.
pub fn interface_value(runtime: &mut Runtime) -> Value {
    interface_value_with(
        runtime,
        AbiType::Builtin(kagari_ir::module::abi::BuiltinType::I32),
        Value::I32(7),
    )
}

#[allow(dead_code)] // Shared support module is also compiled by integration tests.
pub fn interface_value_with(runtime: &mut Runtime, concrete_type: AbiType, data: Value) -> Value {
    use kagari_ir::{
        bytecode::{BytecodeProgram, InterfaceTableRecord, ModuleRef},
        module::{InterfaceTableAbi, PublicAbiItem, TraitAbi, abi::NominalAbiType},
    };
    let identity = ModuleIdentity::single_file("interface-fixture.kgr");
    let declaration = |kind, name: &str| DefinitionId {
        module: identity.clone(),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    };
    let trait_id = declaration(DefinitionKind::Trait, "Tag");
    let impl_id = declaration(DefinitionKind::Impl, "");
    let module = runtime
        .load_program(
            "interface-fixture",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    identity,
                    public_items: vec![
                        PublicAbiItem::Trait(TraitAbi {
                            associated_types: Vec::new(),
                            name: "Tag".into(),
                            generic_params: vec![],
                            bounds: vec![],
                            methods: vec![],
                        }),
                        PublicAbiItem::InterfaceTable(InterfaceTableAbi {
                            declaration: impl_id.clone(),
                            name: String::new(),
                            generic_params: vec![],
                            bounds: vec![],
                            trait_type: AbiType::Trait(NominalAbiType {
                                associated_types: Default::default(),
                                declaration: trait_id,
                                arguments: vec![],
                            }),
                            for_type: concrete_type,
                            methods: vec![],
                        }),
                    ],
                    interface_tables: vec![InterfaceTableRecord {
                        declaration: impl_id,
                        methods: vec![],
                    }],
                    ..Default::default()
                }],
            },
        )
        .unwrap();
    runtime.make_interface(&module, 0, data).unwrap()
}

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
