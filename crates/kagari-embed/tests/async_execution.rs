//! The SDK native-wait contract also runs without source/native compilation features.
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{
        BytecodeInstruction as I, CallTarget, ConstantOperand, LocalSlot, NativeImportId, Register,
    },
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_contract::{
    ids::FunctionRef,
    native_import::NativeImport,
    representation::semantic_representation,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
    runtime::{KagariRuntime, owned::DriveResult},
};
use kagari_runtime::{
    gc::roots::RootedValue,
    module::LoadedModule,
    native::{
        builder::ModuleBuilder,
        completion::{Completion, CompletionStatus},
        future::NativeStart,
        registration::FunctionSpec,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::catalog;
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Wake, Waker},
};

#[cfg(feature = "source")]
#[path = "async_execution/source.rs"]
mod source;

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

struct Fixture {
    #[cfg(feature = "source")]
    engine: KagariEngine,
    runtime: KagariRuntime,
    module: LoadedModule,
    program: PreparedProgram,
    starts: Arc<AtomicUsize>,
    cancels: Arc<AtomicUsize>,
    sent: Arc<Mutex<Vec<Completion<i32>>>>,
    inputs: Arc<Mutex<Vec<i32>>>,
}

impl Fixture {
    fn new() -> Self {
        Self::configured(Default::default())
    }

    fn configured(config: EngineConfig) -> Self {
        let mut engine = KagariEngine::builder().unwrap();
        engine.config(config);
        let future = engine.declarations().future_type().unwrap();
        let owner = future.id().module.clone();
        let future = future.apply([Type::i32()]).unwrap().abi().clone();
        let starts = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let inputs = Arc::new(Mutex::new(Vec::new()));
        let recorded = inputs.clone();
        let (started, cancelled, send) = (starts.clone(), cancels.clone(), sent.clone());
        let mut native = ModuleBuilder::new("test::async_sdk", engine.declarations());
        native
            .add_async_function::<(i32,), i32>(
                FunctionSpec::new("request").parameter_names(["input"]),
                move |(input,), completion| {
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
            .unwrap();
        let native = native.finish().unwrap();
        let declaration = native.to_declaration().unwrap();
        engine.install(native).unwrap();
        let engine = engine.build().unwrap();
        let request = declaration.native_declarations();
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
                    constant: ConstantOperand::I32(13),
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
        let application = BytecodeModule {
            identity: declaration.identity,
            dependencies: vec![ModuleRef::new(1)],
            constants: vec![ConstantOperand::I32(13)],
            native_imports: vec![NativeImport {
                instance: ConcreteFunctionIdentity {
                    declaration: request[0].declaration.clone(),
                    arguments: vec![],
                },
                binding: request[0].declaration.clone(),
                signature: Signature {
                    params: vec![scalar],
                    result: future,
                },
                result_adapter: None,
                generic: None,
                requirements: vec![],
                callables: vec![],
                host: None,
            }],
            public_items: declaration
                .functions
                .into_iter()
                .map(PublicItem::Function)
                .collect(),
            native_declarations: request,
            types: vec![ValueType::Unit, ValueType::I32, ValueType::HeapObject],
            function_table: functions
                .iter()
                .map(|function| FunctionRecord {
                    id: function.id,
                    identity: None,
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
            .into_iter()
            .find(|declaration| declaration.identity == owner)
            .unwrap();
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
                root: ModuleRef::new(0),
                modules: vec![application, foundation],
            },
            Default::default(),
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
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

    fn cold(&self, input: i32) -> RootedValue {
        let execution = self
            .runtime
            .start(
                &self.module,
                "create",
                &[Value::I32(input)],
                &Default::default(),
            )
            .unwrap();
        let DriveResult::Complete(result) = self.runtime.drive(&execution, slice()).unwrap() else {
            panic!("cold creation");
        };
        result.unwrap()
    }
}

#[derive(Default)]
struct Notice(AtomicUsize);

impl Wake for Notice {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn slice() -> NonZeroUsize {
    NonZeroUsize::new(100).unwrap()
}

#[test]
fn sdk_owned_native_wait_contract() {
    let f = Fixture::new();
    assert!(f.program.bytecode().modules().len() > 1);
    assert!(
        f.runtime
            .execute(&f.module, "run", &[], &Default::default())
            .is_err()
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "synchronous entry cannot submit work"
    );
    let future = f.cold(21);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    let execution = f
        .runtime
        .start(
            &f.module,
            "wait",
            &[future.value(f.runtime.runtime().gc()).unwrap()],
            &Default::default(),
        )
        .unwrap();
    let notice = Arc::new(Notice::default());
    execution.set_waker(&Waker::from(notice.clone()));
    assert_eq!(
        notice.0.load(Ordering::SeqCst),
        1,
        "registration observes queued readiness"
    );
    assert!(matches!(
        f.runtime.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert!(!execution.is_ready());
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    assert_eq!(*f.inputs.lock().unwrap(), [21]);
    drop(future);
    f.runtime.runtime().collect_garbage().unwrap();
    let completion = f.sent.lock().unwrap().pop().unwrap();
    std::thread::spawn(move || {
        assert_eq!(completion.complete(Ok(42)), CompletionStatus::Accepted);
        assert_eq!(completion.complete(Ok(99)), CompletionStatus::Duplicate);
    })
    .join()
    .unwrap();
    assert_eq!(notice.0.load(Ordering::SeqCst), 2);
    assert!(execution.is_ready());
    drop(f.cold(30));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "readiness and another root never drive this continuation"
    );
    let DriveResult::Complete(result) = f.runtime.drive(&execution, slice()).unwrap() else {
        panic!("ready continuation");
    };
    assert_eq!(
        result.unwrap().value(f.runtime.runtime().gc()),
        Some(Value::I32(42))
    );
    assert!(!execution.is_ready());
    assert!(
        f.runtime.drive(&execution, slice()).is_err(),
        "retired owner cannot resume twice"
    );
    for (input, output) in [(0, 7), (1, 8)] {
        let future = f.cold(input);
        let execution = f
            .runtime
            .start_future(
                &future.value(f.runtime.runtime().gc()).unwrap(),
                &Default::default(),
            )
            .unwrap();
        let DriveResult::Complete(result) = f.runtime.drive(&execution, slice()).unwrap() else {
            panic!("immediate publication");
        };
        assert_eq!(
            result.unwrap().value(f.runtime.runtime().gc()),
            Some(Value::I32(output))
        );
    }
    let shared = ExecutionContext::default();
    let left_future = f.cold(15);
    let right_future = f.cold(16);
    let left = f
        .runtime
        .start(
            &f.module,
            "wait",
            &[left_future.value(f.runtime.runtime().gc()).unwrap()],
            &shared,
        )
        .unwrap();
    let right = f
        .runtime
        .start(
            &f.module,
            "wait",
            &[right_future.value(f.runtime.runtime().gc()).unwrap()],
            &shared,
        )
        .unwrap();
    for owner in [&left, &right] {
        assert!(matches!(
            f.runtime.drive(owner, slice()).unwrap(),
            DriveResult::Waiting
        ));
    }
    let right_completion = f.sent.lock().unwrap().pop().unwrap();
    let left_completion = f.sent.lock().unwrap().pop().unwrap();
    left.cancel();
    assert!(
        shared.cancellation.check().is_ok(),
        "owner cancellation must not cancel the shared host context"
    );
    assert!(!right.is_ready(), "independent waiter remains pending");
    assert!(matches!(
        f.runtime.drive(&left, slice()).unwrap(),
        DriveResult::Complete(Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::Cancelled,
            ..
        }))
    ));
    assert_eq!(left_completion.complete(Ok(99)), CompletionStatus::Stale);
    assert_eq!(
        right_completion.complete(Ok(43)),
        CompletionStatus::Accepted
    );
    let DriveResult::Complete(result) = f.runtime.drive(&right, slice()).unwrap() else {
        panic!("uncancelled sibling execution");
    };
    assert_eq!(
        result.unwrap().value(f.runtime.runtime().gc()),
        Some(Value::I32(43))
    );
    for abandon in [false, true] {
        let future = f.cold(10);
        let context = ExecutionContext::default();
        let execution = f
            .runtime
            .start_future(&future.value(f.runtime.runtime().gc()).unwrap(), &context)
            .unwrap();
        assert!(matches!(
            f.runtime.drive(&execution, slice()).unwrap(),
            DriveResult::Waiting
        ));
        let completion = f.sent.lock().unwrap().pop().unwrap();
        if abandon {
            drop(execution);
            assert_eq!(f.runtime.drain_retired_executions().unwrap(), 1);
        } else {
            let wake = Arc::new(Notice::default());
            execution.set_waker(&Waker::from(wake.clone()));
            context.cancellation.cancel();
            assert_eq!(wake.0.load(Ordering::SeqCst), 1);
            let DriveResult::Complete(Err(EmbeddingError::Runtime { kind, .. })) =
                f.runtime.drive(&execution, slice()).unwrap()
            else {
                panic!("terminal cancellation");
            };
            assert_eq!(kind, RuntimeFailureKind::Cancelled);
        }
        assert_eq!(completion.complete(Ok(9)), CompletionStatus::Stale);
    }
    assert_eq!(f.cancels.load(Ordering::SeqCst), 3);
    assert_eq!(
        f.runtime
            .runtime()
            .resources()
            .counters()
            .current_call_depth,
        0
    );
}

#[cfg(feature = "native")]
mod native {
    use super::{Fixture, slice};
    use kagari_abi::native::{BackendId, BackendTarget};
    use kagari_codegen::{
        BackendConfiguration, BackendFunctionInput, CodegenBackend, diagnostic::BackendCompileError,
    };
    use kagari_contract::native::NativeCompilationProduct;
    use kagari_embed::runtime::owned::DriveResult;
    use kagari_runtime::{native::completion::CompletionStatus, value::Value};
    use kagari_vm::vm::native::PreparedNativeEntry;
    use std::sync::atomic::Ordering;

    struct Backend {
        calls: usize,
    }

    // SAFETY: this backend only reports unsupported input and never publishes code.
    unsafe impl CodegenBackend for Backend {
        fn configuration(&self) -> BackendConfiguration {
            BackendConfiguration {
                backend: BackendId::new("async-decline"),
                target: BackendTarget::new("host", usize::BITS as u8),
                options: vec![],
            }
        }

        fn compile_function(
            &mut self,
            _: BackendFunctionInput<'_>,
        ) -> Result<NativeCompilationProduct, BackendCompileError> {
            self.calls += 1;
            Err(BackendCompileError::unsupported(
                "fixture must not compile a resume body",
            ))
        }
    }

    #[test]
    fn sdk_native_preparation_declines_resume_before_entry() {
        let f = Fixture::new();
        let mut backend = Backend { calls: 0 };
        let prepared = f
            .runtime
            .prepare_native(
                &f.program,
                &f.module,
                "run",
                &mut backend,
                &Default::default(),
            )
            .unwrap();
        let PreparedNativeEntry::Unsupported { diagnostics, .. } = &prepared else {
            panic!("resume body requires interpreter");
        };
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("owned interpreter driver"))
        );
        assert_eq!(backend.calls, 0);
        assert!(
            f.runtime
                .execute_prepared(&f.module, "run", &[], &Default::default(), &prepared)
                .is_err()
        );
        assert_eq!(f.starts.load(Ordering::SeqCst), 0);
        let execution = f
            .runtime
            .start(&f.module, "run", &[], &Default::default())
            .unwrap();
        assert!(matches!(
            f.runtime.drive(&execution, slice()).unwrap(),
            DriveResult::Waiting
        ));
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(42)),
            CompletionStatus::Accepted
        );
        let DriveResult::Complete(result) = f.runtime.drive(&execution, slice()).unwrap() else {
            panic!("interpreter continuation");
        };
        assert_eq!(
            result.unwrap().value(f.runtime.runtime().gc()),
            Some(Value::I32(42))
        );
        assert_eq!(backend.calls, 0);
    }
}
