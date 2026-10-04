use kagari_bytecode::{
    instruction::StructId,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
};
use kagari_contract::layout::{StructFieldLayout, StructLayout};
use kagari_runtime::{
    Runtime,
    module::{LoadedModule, StructLayoutRef},
    value::Value,
};
use kagari_types::ty::Ty;

#[allow(dead_code)] // Shared support module is also compiled by integration tests.
pub fn interface_value(runtime: &mut Runtime) -> Value {
    interface_value_with(
        runtime,
        Ty::Builtin(kagari_types::scalar::BuiltinType::I32),
        Value::I32(7),
    )
}

#[allow(dead_code)] // Shared support module is also compiled by integration tests.
pub fn interface_value_with(runtime: &mut Runtime, concrete_type: Ty, data: Value) -> Value {
    use kagari_bytecode::{
        module::InterfaceTableRecord,
        program::{BytecodeProgram, ModuleRef},
    };
    use {
        kagari_contract::types::{InterfaceTable, PublicItem},
        kagari_types::{declaration::TraitDef, ty::NominalTy},
    };
    let identity = ModuleIdentity::single_file("interface-fixture.kgr");
    let declaration = |kind, name: &str| DefinitionPath {
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
                        PublicItem::Trait(TraitDef {
                            conversion_adapter: None,
                            storage_access: None,
                            associated_consts: Vec::new(),
                            supertraits: Vec::new(),
                            associated_types: Vec::new(),
                            name: "Tag".into(),
                            generic_params: vec![],
                            bounds: vec![],
                            methods: vec![],
                        }),
                        PublicItem::InterfaceTable(Box::new(InterfaceTable {
                            associated_type_families: Vec::new(),
                            associated_consts: Vec::new(),
                            host_bridge: false,
                            declaration: impl_id.clone(),
                            name: String::new(),
                            generic_params: vec![],
                            bounds: vec![],
                            trait_type: Ty::Trait(NominalTy {
                                associated_types: Default::default(),
                                declaration: trait_id,
                                arguments: vec![],
                            }),
                            for_type: concrete_type,
                            methods: vec![],
                        })),
                    ],
                    interface_tables: vec![InterfaceTableRecord {
                        parents: vec![],
                        view: None,
                        arguments: Vec::new(),
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

#[allow(dead_code)] // Shared helper has consumers in other integration targets.
pub fn layout(runtime: &mut Runtime, name: &str, fields: &[(&str, Ty, bool)]) -> StructLayoutRef {
    let declaration = DefinitionPath {
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
            BytecodeProgram {
                root: ModuleRef::new(0),
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

#[allow(dead_code)] // Shared fixture is compiled by tests that only need nominal layouts.
pub fn allocation_owner(runtime: &mut Runtime) -> LoadedModule {
    runtime
        .load_program(
            "allocation-owner",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap()
}

/// Install private ordinary enum layouts for scalar/array semantic boundary tests.
#[allow(dead_code)]
pub fn enum_owner(runtime: &mut Runtime, payloads: Vec<Ty>) -> LoadedModule {
    use kagari_contract::layout::{EnumLayout, EnumVariantLayout};
    use kagari_types::declaration::module::ModuleDecl;
    let identity = ModuleIdentity::single_file("enum-fixture.kgr");
    let enumerations = payloads
        .into_iter()
        .enumerate()
        .map(|(slot, payload)| {
            let declaration = DefinitionPath {
                module: identity.clone(),
                path: vec![DefinitionPathSegment {
                    kind: DefinitionKind::Enum,
                    name: format!("Item{slot}"),
                    occurrence: 0,
                }],
            };
            EnumLayout {
                declaration: declaration.clone(),
                arguments: vec![],
                variants: vec![EnumVariantLayout {
                    reports_failure: false,
                    declaration: ModuleDecl::variant_id(&declaration, "Data"),
                    payload: vec![payload],
                }],
            }
        })
        .collect();
    runtime
        .load_program(
            "enum-fixture",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    identity,
                    enumerations,
                    ..Default::default()
                }],
            },
        )
        .unwrap()
}
