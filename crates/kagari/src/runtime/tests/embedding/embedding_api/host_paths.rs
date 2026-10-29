use crate::runtime::tests::embedding::native_program::ProgramFixture;
use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::KagariEngine,
    error::EmbeddingError,
    program::PreparedProgram,
    runtime::{LoadOptions, ReloadOptions, Runtime},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    instruction::{
        BytecodeInstruction, CallTarget, ConstantId, ConstantOperand, NativeImportId, PathId, Slot,
    },
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, PathRecord,
        RootSlotLayout,
    },
    program::ModuleRef,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    exports::ProgramExports,
    ids::FunctionRef,
    native_import::NativeImport,
    types::{
        ConcreteFunctionIdentity, PublicItem,
        named::{NamedContract, NamedDeclaration},
    },
};
use kagari_runtime::{
    host::{
        HostCallContext, HostError, HostObjectId, HostPathAdapter, HostPathDescriptor,
        HostPathDescriptorId, HostSchemaEpoch, PreparedHostPathWrite,
    },
    module::entry::ProgramEntry,
    native::{
        builder::ModuleBuilder, declarations::FunctionDecl, host::HostCallback,
        module::NativeModule,
    },
    value::Value,
};

use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{
        FieldDef, TypeDeclaration,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

fn embedding_host_path_module(path_access: PathAccess) -> NativeModule {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("game", &catalog).unwrap();
    let mut player = builder.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    player
        .field(
            DefinitionMetadata::checked(
                DefinitionContext::new().unwrap().snapshot(),
                FieldDef {
                    name: "hp".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    visibility: Visibility::Public,
                    mutable: true,
                    host_policy: Some(HostFieldPolicy {
                        readable: true,
                        path_access: PathAccess::ReadWrite,
                    }),
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let player = player.finish().unwrap();
    let hp = player
        .definitions()
        .lookup_child(player.id(), DefinitionKind::Field, "hp", 0)
        .unwrap();
    let other_access = if path_access == PathAccess::ReadWrite {
        PathAccess::ReadOnly
    } else {
        PathAccess::ReadWrite
    };
    for access in [path_access, other_access] {
        let path = builder
            .define_path(
                DefinitionMetadata::checked(
                    player.definitions().clone(),
                    HostPathDeclaration {
                        root: player.id(),
                        segments: vec![HostPathSegmentDeclaration::Field(hp)],
                        access,
                        schema_epoch: 0,
                    },
                    &Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
        let mut adapter = unsafe {
            // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
            HostPathAdapter::new().with_read_transport(|_, _| Ok(Value::I32(10)))
        };
        if access == PathAccess::ReadWrite {
            adapter = unsafe {
                // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
                adapter.with_prepare_write_transport(|_, _, record| {
                    if matches!(record.new_value, Value::I32(_)) {
                        Ok(PreparedHostPathWrite::new(|| {}))
                    } else {
                        Err(HostError::new("hp expects i32"))
                    }
                })
            };
        }
        builder.bind_path(&path, adapter).unwrap();
    }
    let make = builder
        .define_function(
            FunctionDecl::new("player")
                .returns(player.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    builder
        .bind(make, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(|cx: &HostCallContext<'_>, _: &[Value]| {
                let root = cx.runtime().host().root(HostObjectId(1)).unwrap();
                Ok(cx.runtime().gc().alloc_host_root_transport(root).unwrap())
            })
        })
        .unwrap();
    builder.finish().unwrap()
}

fn register_embedding_host_path_runtime(runtime: &mut Runtime) -> HostPathDescriptorId {
    let player = runtime
        .runtime()
        .types()
        .get_by_name("game::Player")
        .unwrap()
        .id;
    runtime
        .runtime_mut()
        .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = HostPathDescriptorId::new(0);
    assert!(
        runtime
            .runtime()
            .host()
            .path_descriptor(descriptor)
            .is_some()
    );
    descriptor
}

fn host_path_artifact(
    contract: &HostPathDescriptor,
    source_name: &str,
    path_debug_name: &str,
    instructions: Vec<BytecodeInstruction>,
    registers: Vec<ValueType>,
    return_type: ValueType,
    constants: Vec<ConstantOperand>,
) -> DefinitionMetadata<BytecodeArtifact> {
    let definitions = DefinitionContext::new().unwrap();
    let provider = embedding_host_path_module(contract.access)
        .to_declaration()
        .import_into(&definitions, &Default::default())
        .unwrap();
    let module = provider.records();
    let make = module
        .native_declarations(provider.definitions())
        .unwrap()
        .remove(0);
    let CallableImplementation::Native(binding) = make.function.implementation else {
        panic!("native fixture entry")
    };
    let import = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: ConcreteFunctionIdentity {
            declaration: make.declaration,
            arguments: vec![],
        },
        binding,
        signature: Signature {
            params: vec![],
            result: make.function.return_type.clone(),
        },
        requirements: vec![],
        callables: vec![],
    };
    let path = module.paths[0].clone();
    let schema = TypeDeclaration {
        declaration: path.root,
        definition: module.types[0].clone(),
    };
    assert_eq!(
        path.contract(std::slice::from_ref(&schema), provider.definitions())
            .unwrap()
            .fingerprint(provider.definitions())
            .unwrap(),
        contract.abi_fingerprint.0
    );
    let identity = ModuleIdentity::single_file(source_name);
    let root = definitions.intern_root(&identity).unwrap();
    let main = definitions
        .intern_child(root, DefinitionKind::Function, "main", 0)
        .unwrap();
    let instance = ConcreteFunctionIdentity {
        declaration: main,
        arguments: vec![],
    };
    let mut metadata = FunctionMetadata {
        roots: RootSlotLayout::from_types(&[registers.to_vec(), [].to_vec()].concat()),
        return_type,
        slots: registers,
        ..FunctionMetadata::default()
    };
    metadata.semantic.result = Some(match return_type {
        ValueType::Unit => Ty::Builtin(BuiltinType::Unit),
        ValueType::I32 => Ty::Builtin(BuiltinType::I32),
        _ => panic!("host path fixture returns only unit or i32"),
    });
    for (index, ty) in metadata.slots.iter().enumerate() {
        if *ty == ValueType::HostHandle {
            metadata
                .semantic
                .slots
                .insert(index, Ty::Host(schema.declaration));
        }
    }
    let artifact = KbcArtifact::from_program(
        ProgramFixture {
            exports: ProgramExports {
                entry: Some(main),
                ..Default::default()
            },
            root: ModuleRef::new(0),
            modules: vec![
                BytecodeModule {
                    dependencies: vec![ModuleRef::new(1)],
                    external_types: vec![schema.clone()],
                    path_declarations: vec![path],
                    native_imports: if instructions.iter().any(|instruction| {
                        matches!(
                            instruction,
                            BytecodeInstruction::Call {
                                callee: CallTarget::Native(_),
                                ..
                            }
                        )
                    }) {
                        vec![import]
                    } else {
                        vec![]
                    },
                    identity,
                    source_name: source_name.to_owned(),
                    module_slots: vec![],
                    constants,
                    types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
                    paths: vec![PathRecord {
                        contract_fingerprint: contract.abi_fingerprint.0,
                        id: PathId::new(0),
                        root_ty: ValueType::HostHandle,
                        result_ty: ValueType::I32,
                        read_only: contract.access == PathAccess::ReadOnly,
                        debug_name: path_debug_name.to_owned(),
                    }],
                    function_table: vec![FunctionRecord {
                        id: FunctionRef::new(0),
                        identity: Some(instance.clone()),
                        name: "main".to_owned(),
                        params: metadata.params.clone(),
                        return_type: metadata.return_type,
                        effects: metadata.effects,
                    }],
                    functions: vec![{
                        let fixture_instructions = instructions;
                        let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata =
                            metadata;

                        if fixture_metadata.origins.is_empty() {
                            fixture_metadata.origins = (0..fixture_instructions.len())
                                .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                                    span: Default::default(),
                                    line: None,
                                    column: None,
                                    instruction,
                                })
                                .collect();
                        }
                        BytecodeFunction {
                            id: FunctionRef::new(0),
                            identity: Some(instance.clone()),
                            name: "main".to_owned(),
                            parameter_count: 0,
                            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                            metadata: fixture_metadata,
                            instructions: fixture_instructions,
                        }
                    }],
                    ..BytecodeModule::default()
                },
                BytecodeModule {
                    identity: module.identity.clone(),
                    source_name: "native:game".into(),
                    external_types: vec![schema],
                    named_declarations: vec![NamedDeclaration {
                        declaration: make.declaration,
                        visibility: Visibility::Public,
                        ambiguous_local_body: None,
                        contract: NamedContract::Function(make.function.clone()),
                    }],
                    native_declarations: vec![make.clone()],
                    public_items: vec![
                        PublicItem::Type(module.types[0].clone()),
                        PublicItem::Function(make.function),
                    ],
                    ..Default::default()
                },
            ],
        }
        .build(&definitions),
        ArtifactBuildOptions::default(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    DefinitionMetadata::checked(definitions.snapshot(), artifact, &Default::default()).unwrap()
}

#[test]
fn reload_rejects_typed_path_fingerprint_changes_without_publishing_epoch() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![embedding_host_path_module(PathAccess::ReadWrite)],
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(&mut runtime);
    let contract = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let first = host_path_artifact(
        &contract,
        "reload_paths.kgr",
        "game.Player.hp",
        vec![BytecodeInstruction::Return(None)],
        vec![],
        ValueType::Unit,
        vec![],
    );
    let changed = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(1))
        .unwrap()
        .clone();
    assert_ne!(changed.abi_fingerprint, contract.abi_fingerprint);
    let candidate = host_path_artifact(
        &changed,
        "reload_paths.kgr",
        "game.Player.hp.readonly",
        vec![BytecodeInstruction::Return(None)],
        vec![],
        ValueType::Unit,
        vec![],
    );

    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(first, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                program_name: Some("reload_paths".to_owned()),
            },
        )
        .expect("module should load");
    let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
    let before_count = runtime.runtime().modules().loaded_count();
    let before_epoch = runtime.runtime().select_program(&loaded).unwrap().epoch;

    let error = runtime
        .reload_program(
            &loaded,
            &PreparedProgram::from_artifact(candidate, &Default::default(), &Default::default())
                .unwrap(),
            ReloadOptions {
                program_name: Some("reload_paths".to_owned()),
            },
        )
        .expect_err("changed typed path fingerprints should reject reload");

    let EmbeddingError::ReloadValidation { code, message } = error else {
        panic!("expected reload validation error");
    };
    assert_eq!(code, "KG_RELOAD_PATH_FINGERPRINT_MISMATCH");
    assert!(message.contains("typed path fingerprints"));
    assert_eq!(runtime.runtime().modules().loaded_count(), before_count);
    assert_eq!(
        runtime.runtime().select_program(&loaded).unwrap().epoch,
        before_epoch
    );
}

#[test]
fn installed_host_path_mutation_uses_declared_access() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![embedding_host_path_module(PathAccess::ReadWrite)],
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(&mut runtime);
    let contract = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let artifact = host_path_artifact(
        &contract,
        "set_path.kgr",
        "game.Player.hp",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Slot::new(0)),
                callee: CallTarget::Native(NativeImportId::new(0)),
                args: vec![],
            },
            BytecodeInstruction::LoadConst {
                dst: Slot::new(1),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::SetPath {
                root_or_view: Slot::new(0),
                path: PathId::new(0),
                dynamic_args: vec![],
                value: Slot::new(1),
            },
            BytecodeInstruction::Return(None),
        ],
        vec![ValueType::HostHandle, ValueType::I32],
        ValueType::Unit,
        vec![ConstantOperand::I32(5)],
    );
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                program_name: Some("set_path".to_owned()),
            },
        )
        .expect("module should load");

    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| Ok(view.is_unit()))
            .expect("retained execution result")
    );
}

#[test]
fn installed_host_path_reads_need_no_permission_flags() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![embedding_host_path_module(PathAccess::ReadOnly)],
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    register_embedding_host_path_runtime(&mut runtime);
    let contract = runtime
        .runtime()
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let artifact = host_path_artifact(
        &contract,
        "read_secure_path.kgr",
        "game.Player.secure_hp",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Slot::new(0)),
                callee: CallTarget::Native(NativeImportId::new(0)),
                args: vec![],
            },
            BytecodeInstruction::ReadPath {
                dst: Slot::new(1),
                root_or_view: Slot::new(0),
                path: PathId::new(0),
                dynamic_args: vec![],
            },
            BytecodeInstruction::Return(Some(Slot::new(1))),
        ],
        vec![ValueType::HostHandle, ValueType::I32],
        ValueType::I32,
        vec![],
    );
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                program_name: Some("read_secure_path".to_owned()),
            },
        )
        .expect("module should load");

    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(10)
    );
}
