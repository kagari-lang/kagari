//! Shared native-wait fixture for unit and standalone consumer contracts.
use super::{EngineConfig, KagariEngine, KagariEngineBuilder, KagariRuntime, PreparedProgram};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{
        BytecodeInstruction as I, CallTarget, ConstantId, ConstantOperand, LocalSlot,
        NativeImportId, Register,
    },
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    exports::{ExportNamespace, ProgramExports},
    ids::FunctionRef,
    native_import::NativeImport,
    representation::semantic_representation,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_runtime::{
    module::LoadedModule,
    native::{
        adapter::Async,
        builder::ModuleBuilder,
        completion::{Completion, CompletionStatus},
        declarations::FunctionDecl,
        future::NativeStart,
        types::Type,
    },
};
use kagari_stdlib::catalog;
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{FnDecl, Param},
    scalar::BuiltinType,
    ty::Ty,
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn function(
    id: usize,
    name: &str,
    input: Ty,
    output: Ty,
    instructions: Vec<I>,
    suspends: bool,
) -> BytecodeFunction {
    let input_repr = semantic_representation(&input);
    let output_repr = semantic_representation(&output);
    let mut metadata = FunctionMetadata {
        params: vec![input_repr],
        locals: vec![input_repr],
        registers: vec![input_repr, output_repr],
        return_type: output_repr,
        roots: RootSlotLayout::from_types(&[input_repr], &[input_repr, output_repr]),
        ..Default::default()
    };
    metadata.effects.may_suspend = suspends;
    metadata.semantic.params.insert(0, input.clone());
    metadata.semantic.locals.insert(0, input.clone());
    metadata.semantic.registers.insert(0, input);
    metadata.semantic.registers.insert(1, output.clone());
    metadata.semantic.result = Some(output);
    BytecodeFunction {
        id: FunctionRef::new(id),
        identity: None,
        name: name.into(),
        parameter_count: 1,
        local_count: 1,
        register_count: 2,
        metadata,
        instructions,
    }
}

pub(super) struct Fixture {
    // The source suite uses the engine; the external consumer only uses the runtime.
    #[allow(dead_code)]
    #[cfg(feature = "source")]
    pub(super) engine: KagariEngine,
    pub(super) runtime: KagariRuntime,
    pub(super) module: LoadedModule,
    pub(super) program: PreparedProgram,
    pub(super) starts: Arc<AtomicUsize>,
    pub(super) cancels: Arc<AtomicUsize>,
    pub(super) sent: Arc<Mutex<Vec<Completion<i32>>>>,
    pub(super) inputs: Arc<Mutex<Vec<i32>>>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        Self::configured(Default::default())
    }

    pub(super) fn configured(config: EngineConfig) -> Self {
        Self::with_provider(config, |_| {})
    }

    pub(super) fn with_provider(
        config: EngineConfig,
        install: impl FnOnce(&mut KagariEngineBuilder),
    ) -> Self {
        let mut engine = KagariEngine::builder().unwrap();
        engine.config(config);
        let definitions = DefinitionContext::new().unwrap();
        let future = engine.declarations().future_type().unwrap();
        let owner = future
            .definitions()
            .resolve(future.id())
            .unwrap()
            .module()
            .clone();
        let future = future.apply([Type::i32()]).unwrap();
        let future = DefinitionMetadata::checked(
            future.definitions().unwrap().clone(),
            future.abi().clone(),
            &Default::default(),
        )
        .unwrap()
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
        let starts = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let inputs = Arc::new(Mutex::new(Vec::new()));
        let recorded = inputs.clone();
        let (started, cancelled, send) = (starts.clone(), cancels.clone(), sent.clone());
        let mut native = ModuleBuilder::new("test::async_sdk", engine.declarations()).unwrap();
        {
            let registered_function = native
                .define_function(
                    FunctionDecl::new("request")
                        .parameter("input", Type::i32())
                        .returns(
                            native
                                .declarations()
                                .future_type()
                                .unwrap()
                                .apply([Type::i32()])
                                .unwrap(),
                        ),
                )
                .unwrap();
            native.bind::<Async<(i32,)>, i32>(
                registered_function,
                move |(input,): (i32,), completion: Completion<i32>| {
                    started.fetch_add(1, Ordering::SeqCst);
                    recorded.lock().unwrap().push(input);
                    match input {
                        0 => Ok(NativeStart::Ready(7)),
                        1 => {
                            assert_eq!(completion.complete(Ok(8)), CompletionStatus::Accepted);
                            Ok(NativeStart::pending())
                        }
                        _ => {
                            send.lock().unwrap().push(completion);
                            let cancelled = cancelled.clone();
                            Ok(NativeStart::cancellable(move || {
                                cancelled.fetch_add(1, Ordering::SeqCst);
                            }))
                        }
                    }
                },
            )
        }
        .unwrap();
        let native = native.finish().unwrap();
        let declaration = native
            .to_declaration()
            .import_into(&definitions, &Default::default())
            .unwrap();
        engine.install(native).unwrap();
        install(&mut engine);
        let engine = engine.build().unwrap();
        let request = declaration
            .records()
            .native_declarations(declaration.definitions())
            .unwrap();
        let declaration = declaration.into_records();
        let scalar = Ty::Builtin(BuiltinType::I32);
        let load = I::LoadLocal {
            dst: Register::new(0),
            local: LocalSlot::new(0),
        };
        let result = I::Return(Some(Register::new(1)));
        let mut functions = vec![
            function(
                0,
                "create",
                scalar.clone(),
                future.clone(),
                vec![
                    load.clone(),
                    I::Call {
                        dst: Some(Register::new(1)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![Register::new(0)],
                    },
                    result.clone(),
                ],
                false,
            ),
            function(
                1,
                "wait",
                future.clone(),
                scalar.clone(),
                vec![
                    load,
                    I::Await {
                        dst: Register::new(1),
                        value: Register::new(0),
                        future: future.clone(),
                    },
                    result,
                ],
                true,
            ),
        ];
        let mut run = function(
            2,
            "run",
            future.clone(),
            scalar.clone(),
            vec![
                I::LoadConst {
                    dst: Register::new(1),
                    constant: ConstantId::new(0),
                },
                I::Call {
                    dst: Some(Register::new(0)),
                    callee: CallTarget::Native(NativeImportId::new(0)),
                    args: vec![Register::new(1)],
                },
                I::Await {
                    dst: Register::new(1),
                    value: Register::new(0),
                    future: future.clone(),
                },
                I::Return(Some(Register::new(1))),
            ],
            true,
        );
        run.parameter_count = 0;
        run.local_count = 0;
        run.metadata.params.clear();
        run.metadata.locals.clear();
        run.metadata.semantic.params.clear();
        run.metadata.semantic.locals.clear();
        run.metadata.roots = RootSlotLayout::from_types(&[], &run.metadata.registers);
        functions.push(run);
        // These SDK fixtures test public host entry, so author the full callable
        // contract from their explicit semantic signatures before serialization.
        let root = definitions.intern_root(&declaration.identity).unwrap();
        let mut namespace = ExportNamespace::default();
        let mut public_items = declaration
            .functions
            .into_iter()
            .map(PublicItem::Function)
            .collect::<Vec<_>>();
        for function in &mut functions {
            let id = definitions
                .intern_child(root, DefinitionKind::Function, &function.name, 0)
                .unwrap();
            function.identity = Some(ConcreteFunctionIdentity {
                declaration: id,
                arguments: vec![],
            });
            namespace.functions.insert(function.name.clone(), id);
            public_items.push(PublicItem::Function(FnDecl {
                host_policy: None,
                method_policy: Default::default(),
                name: function.name.clone(),
                implementation: CallableImplementation::Script,
                generic_params: vec![],
                bounds: vec![],
                params: function
                    .metadata
                    .semantic
                    .params
                    .iter()
                    .map(|(slot, ty)| Param {
                        name: format!("argument{slot}"),
                        ty: ty.clone(),
                        mutable: false,
                    })
                    .collect(),
                return_type: function.metadata.semantic.result.clone().unwrap(),
            }));
        }
        let exports = ProgramExports {
            namespaces: [(declaration.identity.clone(), namespace)].into(),
            entry: None,
        };
        let application = BytecodeModule {
            identity: declaration.identity,
            dependencies: vec![ModuleRef::new(1)],
            constants: vec![ConstantOperand::I32(13)],
            native_imports: vec![NativeImport {
                receiver_access: request[0].function.method_policy.receiver_access,
                instance: ConcreteFunctionIdentity {
                    declaration: request[0].declaration,
                    arguments: vec![],
                },
                binding: request[0].declaration,
                signature: Signature {
                    params: vec![scalar],
                    result: future,
                },
                result_adapter: None,
                generic: None,
                requirements: vec![],
                callables: vec![],
            }],
            public_items,
            native_declarations: request,
            types: vec![ValueType::Unit, ValueType::I32, ValueType::HeapObject],
            function_table: functions
                .iter()
                .map(|function| FunctionRecord {
                    id: function.id,
                    identity: function.identity.clone(),
                    name: function.name.clone(),
                    params: function.metadata.params.clone(),
                    return_type: function.metadata.return_type,
                    effects: function.metadata.effects,
                })
                .collect(),
            functions,
            ..Default::default()
        };
        let foundation = catalog::shared()
            .unwrap()
            .into_iter()
            .find(|declaration| declaration.records().identity == owner)
            .unwrap();
        let foundation = foundation
            .import_into(&definitions, &Default::default())
            .unwrap()
            .into_records();
        let foundation = BytecodeModule {
            identity: owner,
            public_items: foundation
                .types
                .iter()
                .cloned()
                .map(PublicItem::Type)
                .collect(),
            types: vec![ValueType::Unit],
            ..Default::default()
        };
        let artifact = KbcArtifact::from_program(
            BytecodeProgram {
                exports,
                root: ModuleRef::new(0),
                modules: vec![application, foundation],
            },
            Default::default(),
            &definitions,
        )
        .unwrap();
        let decoded =
            KbcArtifact::from_bytes(&artifact.to_bytes(&definitions.snapshot()).unwrap()).unwrap();
        let program =
            PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap();
        let mut runtime = engine.runtime(Default::default());
        let module = runtime.load_program(&program, Default::default()).unwrap();
        Self {
            #[cfg(feature = "source")]
            engine,
            runtime,
            module,
            program,
            starts,
            cancels,
            sent,
            inputs,
        }
    }
}

pub(super) fn slice() -> NonZeroUsize {
    NonZeroUsize::new(100).unwrap()
}
