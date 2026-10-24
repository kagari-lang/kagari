//! Shared native-wait fixture for unit and standalone consumer contracts.
use super::{EngineConfig, KagariEngine, KagariEngineBuilder, PreparedProgram, Runtime};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{
        BytecodeInstruction as I, CallTarget, ConstantId, ConstantOperand, NativeImportId, Slot,
    },
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::ModuleRef,
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    exports::{ExportNamespace, ProgramExports},
    ids::FunctionRef,
    native_import::NativeImport,
    representation::semantic_representation,
    types::{
        ConcreteFunctionIdentity, PublicItem,
        named::{NamedContract, NamedDeclaration},
    },
};
use kagari_runtime::{
    module::Program,
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
    visibility::Visibility,
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::native_program::ProgramFixture;

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
        return_type: output_repr,
        roots: RootSlotLayout::from_types(
            &[[input_repr, output_repr].to_vec(), [input_repr].to_vec()].concat(),
        ),
        slots: [vec![input_repr, output_repr], vec![input_repr]].concat(),
        ..Default::default()
    };
    metadata.effects.may_suspend = suspends;
    metadata.semantic.params.insert(0, input.clone());
    metadata.semantic.slots.insert(2, input.clone());
    metadata.semantic.slots.insert(0, input);
    metadata.semantic.slots.insert(1, output.clone());
    metadata.semantic.result = Some(output);
    {
        let parameter_base: usize = 2;
        let fixture_instructions = instructions;
        let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata = metadata;
        if fixture_metadata.parameter_slots.is_empty() {
            fixture_metadata.parameter_slots =
                (0..1).map(|i| Slot::new(parameter_base + i)).collect();
        }
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
            id: FunctionRef::new(id),
            identity: None,
            name: name.into(),
            parameter_count: 1,
            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
            metadata: fixture_metadata,
            instructions: fixture_instructions,
        }
    }
}

pub(super) struct Fixture {
    // The source suite uses the engine; the external consumer only uses the runtime.
    #[allow(dead_code)]
    #[cfg(feature = "source")]
    pub(super) engine: KagariEngine,
    pub(super) runtime: Runtime,
    pub(super) module: Program,
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
        let load = I::Move {
            dst: Slot::new(0),
            src: Slot::new(2),
        };
        let result = I::Return(Some(Slot::new(1)));
        let mut functions = vec![
            function(
                0,
                "create",
                scalar.clone(),
                future.clone(),
                vec![
                    load.clone(),
                    I::Call {
                        dst: Some(Slot::new(1)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![Slot::new(0)],
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
                        dst: Slot::new(1),
                        value: Slot::new(0),
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
                    dst: Slot::new(1),
                    constant: ConstantId::new(0),
                },
                I::Call {
                    dst: Some(Slot::new(0)),
                    callee: CallTarget::Native(NativeImportId::new(0)),
                    args: vec![Slot::new(1)],
                },
                I::Await {
                    dst: Slot::new(1),
                    value: Slot::new(0),
                    future: future.clone(),
                },
                I::Return(Some(Slot::new(1))),
            ],
            true,
        );
        run.parameter_count = 0;
        run.slot_count = 2;
        run.metadata.parameter_slots.clear();
        run.metadata.params.clear();
        run.metadata.slots.truncate(2);
        run.metadata.semantic.params.clear();
        run.metadata.semantic.slots.remove(&2);
        run.metadata.roots = RootSlotLayout::from_types(&run.metadata.slots);
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
            named_declarations: request
                .iter()
                .map(|entry| NamedDeclaration {
                    declaration: entry.declaration,
                    visibility: Visibility::Public,
                    ambiguous_local_body: None,
                    contract: NamedContract::Function(entry.function.clone()),
                })
                .collect(),
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
            ProgramFixture {
                exports,
                root: ModuleRef::new(0),
                modules: vec![application, foundation],
            }
            .build(&definitions),
            Default::default(),
            &definitions,
            &Default::default(),
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
