use kagari_bytecode::{
    instruction::StructId,
    module::{BytecodeModule, InterfaceTableRecord},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout},
    types::{InterfaceTable, PublicItem},
};
use kagari_runtime::{
    Runtime,
    module::{LoadedModule, StructLayoutRef},
    native::{builder::ModuleBuilder, storage::NativeStorage},
    value::Value,
};
use kagari_types::{
    declaration::TraitDef,
    ty::{NominalTy, Ty},
    visibility::Visibility,
};

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
    let identity = ModuleIdentity::single_file("interface-fixture.kgr");
    let (context, types) = type_scope(runtime, vec![concrete_type]);
    let concrete_type = types.into_iter().next().unwrap();
    let root = context.intern_root(&identity).unwrap();
    let declaration = |kind, name: &str| context.intern_child(root, kind, name, 0).unwrap();
    let trait_id = declaration(DefinitionKind::Trait, "Tag");
    let impl_id = declaration(DefinitionKind::Impl, "");
    let module = runtime
        .load_program(
            "interface-fixture",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule {
                        identity,
                        public_items: vec![
                            PublicItem::Trait(TraitDef {
                                conversion_adapter: None,
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
                                bridge: None,
                                declaration: impl_id,
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
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    runtime.make_interface(&module, 0, data).unwrap()
}

#[allow(dead_code)] // Shared helper has consumers in other integration targets.
pub fn layout(runtime: &mut Runtime, name: &str, fields: &[(&str, Ty, bool)]) -> StructLayoutRef {
    let (context, types) = type_scope(
        runtime,
        fields.iter().map(|(_, ty, _)| ty.clone()).collect(),
    );
    let identity = ModuleIdentity::single_file("layout-fixture.kgr");
    let root = context.intern_root(&identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Struct, name, 0)
        .unwrap();
    let fields = fields
        .iter()
        .zip(types)
        .map(|((name, _, mutable), ty)| {
            let id = context
                .intern_child(declaration, DefinitionKind::Field, name, 0)
                .unwrap();
            StructFieldLayout {
                visibility: Visibility::Public,
                declaration: id,
                name: (*name).into(),
                ty,
                mutable: *mutable,
            }
        })
        .collect();
    let module = runtime
        .load_program(
            name,
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
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
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    module.struct_layout(StructId::new(0)).unwrap()
}

#[allow(dead_code)] // Shared fixture is compiled by tests that only need nominal layouts.
pub fn allocation_owner(runtime: &mut Runtime) -> LoadedModule {
    let context = DefinitionContext::new().unwrap();
    runtime
        .load_program(
            "allocation-owner",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap()
}

/// Install private ordinary enum layouts for scalar/array semantic boundary tests.
#[allow(dead_code)]
pub fn enum_owner(runtime: &mut Runtime, payloads: Vec<Ty>) -> LoadedModule {
    let (context, payloads) = type_scope(runtime, payloads);
    let identity = ModuleIdentity::single_file("enum-fixture.kgr");
    let root = context.intern_root(&identity).unwrap();
    let enumerations = payloads
        .into_iter()
        .enumerate()
        .map(|(slot, payload)| {
            let declaration = context
                .intern_child(root, DefinitionKind::Enum, &format!("Item{slot}"), 0)
                .unwrap();
            EnumLayout {
                declaration,
                arguments: vec![],
                variants: vec![EnumVariantLayout {
                    reports_failure: false,
                    declaration: context
                        .intern_child(declaration, DefinitionKind::Variant, "Data", 0)
                        .unwrap(),
                    payload: vec![payload],
                }],
            }
        })
        .collect();
    runtime
        .load_program(
            "enum-fixture",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule {
                        identity,
                        enumerations,
                        ..Default::default()
                    }],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap()
}

/// Load registered sequence contracts for tests of growth and detached leases.
#[allow(dead_code)]
pub fn sequence_owner(runtime: &mut Runtime) -> LoadedModule {
    let mut builder = ModuleBuilder::new("kagari-alloc::vec", &Default::default()).unwrap();
    let mut vector = builder.define_type("Vec").unwrap();
    vector.type_parameter("T").unwrap();
    vector.native_storage(NativeStorage::sequence(0)).unwrap();
    vector.finish().unwrap();
    let module = builder.finish().unwrap();
    let context = DefinitionContext::new().unwrap();
    let declaration = module
        .to_declaration()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    module.install(runtime).unwrap();
    runtime
        .load_program(
            "sequence-fixture",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule {
                        identity: declaration.identity.clone(),
                        public_items: declaration
                            .types
                            .iter()
                            .cloned()
                            .map(PublicItem::Type)
                            .collect(),
                        ..Default::default()
                    }],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap()
}

/// Adopt runtime-owned nominal inputs into an independent authoring scope.
fn type_scope(runtime: &Runtime, types: Vec<Ty>) -> (DefinitionContext, Vec<Ty>) {
    let context = DefinitionContext::new().unwrap();
    let types =
        DefinitionMetadata::checked(runtime.host().definitions(), types, &Default::default())
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
    (context, types)
}
