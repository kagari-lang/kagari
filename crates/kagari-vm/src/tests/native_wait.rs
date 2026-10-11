//! Source-free native Future/await contracts; source async lowering belongs to AX03.
use crate::{
    error::VmError,
    executor::Executor,
    vm::{Vm, owned::DriveResult},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction as I, CallTarget, LocalSlot, NativeImportId, Register},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_contract::{
    ids::FunctionRef,
    native_import::NativeImport,
    representation::semantic_representation,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootedValue,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::{Completion, CompletionStatus},
        conversion::context::ConversionContext,
        future::NativeStart,
        registration::FunctionSpec,
        storage::NativeStorage,
        typed::NativeContext,
        types::Type,
    },
    session::ExecutionOptions,
    value::Value,
};
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};
use std::{
    num::NonZeroUsize,
    slice::from_ref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn function(
    id: usize,
    name: &str,
    parameter: Ty,
    result: Ty,
    instructions: Vec<I>,
    suspends: bool,
) -> BytecodeFunction {
    let params = vec![semantic_representation(&parameter)];
    let registers = vec![
        semantic_representation(&parameter),
        semantic_representation(&result),
    ];
    let mut metadata = FunctionMetadata {
        roots: RootSlotLayout::from_types(&params, &registers),
        params: params.clone(),
        locals: params,
        registers,
        return_type: semantic_representation(&result),
        ..Default::default()
    };
    metadata.effects.may_suspend = suspends;
    metadata.semantic.params.insert(0, parameter.clone());
    metadata.semantic.locals.insert(0, parameter.clone());
    metadata.semantic.registers.insert(0, parameter);
    metadata.semantic.registers.insert(1, result.clone());
    metadata.semantic.result = Some(result);
    BytecodeFunction {
        id: FunctionRef::new(id),
        identity: None,
        name: name.into(),
        parameter_count: 1,
        register_count: 2,
        local_count: 1,
        metadata,
        instructions,
    }
}

type PendingReplies = Vec<(i32, Completion<i32>)>;

struct Fixture {
    vm: Vm,
    module: LoadedModule,
    sent: Arc<Mutex<PendingReplies>>,
    starts: Arc<AtomicUsize>,
    cancels: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_functions(|_| {})
    }

    fn with_functions(edit: impl FnOnce(&mut Vec<BytecodeFunction>)) -> Self {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let starts = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));
        let mut vector = ModuleBuilder::new("kagari-alloc::vec", &DeclarationCatalog::default());
        let mut ty = vector.define_type("Vec");
        ty.type_parameter("T").unwrap();
        ty.native_storage(NativeStorage::sequence(0)).unwrap();
        ty.finish().unwrap();
        let vector = vector.finish().unwrap();
        let vector_declaration = vector.to_declaration().unwrap();
        let mut builder = ModuleBuilder::new("test::io", &vector.catalog());
        let mut future = builder.define_type("Future");
        future.type_parameter("T").unwrap();
        future.native_storage(NativeStorage::future()).unwrap();
        let future = future
            .finish()
            .unwrap()
            .apply([Type::i32()])
            .unwrap()
            .abi()
            .clone();
        let (sent_to, started, cancelled) = (sent.clone(), starts.clone(), cancels.clone());
        builder
            .add_async_function::<(Vec<i32>,), i32>(
                FunctionSpec::new("request").parameter_names(["values"]),
                move |(values,), completion| {
                    started.fetch_add(1, Ordering::Relaxed);
                    let input = values.iter().sum();
                    if input == 0 {
                        return Ok(NativeStart::Ready(7));
                    }
                    if input == -1 {
                        completion.complete(Ok(8));
                    }
                    if input == -2 {
                        return Err(RuntimeError::new(
                            RuntimeErrorKind::ScriptTrap,
                            "submission failed",
                        ));
                    }
                    sent_to.lock().unwrap().push((input, completion));
                    let cancelled = cancelled.clone();
                    Ok(NativeStart::cancellable(move || {
                        cancelled.fetch_add(1, Ordering::Relaxed);
                        assert_ne!(input, -4, "provider cleanup panic");
                    }))
                },
            )
            .unwrap();
        builder
            .add_function(
                FunctionSpec::new("read").parameter_names(["values", "index"]),
                |_: &mut NativeContext<'_>,
                 (values, index): (Vec<i32>, i32)|
                 -> NativeResult<i32> {
                    values.get(index as usize).copied().ok_or_else(|| {
                        RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds, "fixture index")
                    })
                },
            )
            .unwrap();
        let native = builder.finish().unwrap();
        let declaration = native.to_declaration().unwrap();
        let mut native_declarations = declaration.native_declarations();
        native_declarations.sort_by_key(|entry| entry.function.name != "request");
        let request = &native_declarations[0];
        let array = request.function.params[0].ty.clone();
        let mut functions = vec![
            function(
                0,
                "create",
                array.clone(),
                future.clone(),
                vec![
                    I::LoadLocal {
                        dst: Register::new(0),
                        local: LocalSlot::new(0),
                    },
                    I::Call {
                        dst: Some(Register::new(1)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![Register::new(0)],
                    },
                    I::Return(Some(Register::new(1))),
                ],
                false,
            ),
            function(
                1,
                "wait",
                future.clone(),
                Ty::Builtin(BuiltinType::I32),
                vec![
                    I::LoadLocal {
                        dst: Register::new(0),
                        local: LocalSlot::new(0),
                    },
                    I::Await {
                        dst: Register::new(1),
                        value: Register::new(0),
                        future: future.clone(),
                    },
                    I::Return(Some(Register::new(1))),
                ],
                true,
            ),
        ];
        edit(&mut functions);
        let module = BytecodeModule {
            identity: declaration.identity.clone(),
            dependencies: vec![ModuleRef::new(1)],
            native_imports: native_declarations
                .iter()
                .map(|entry| NativeImport {
                    instance: ConcreteFunctionIdentity {
                        declaration: entry.declaration.clone(),
                        arguments: vec![],
                    },
                    binding: entry.declaration.clone(),
                    signature: Signature {
                        params: entry.function.params.iter().map(|p| p.ty.clone()).collect(),
                        result: entry.function.return_type.clone(),
                    },
                    result_adapter: None,
                    generic: None,
                    requirements: vec![],
                    callables: vec![],
                    host: None,
                })
                .collect(),
            public_items: declaration
                .types
                .into_iter()
                .map(PublicItem::Type)
                .chain(declaration.functions.into_iter().map(PublicItem::Function))
                .collect(),
            native_declarations,
            types: vec![ValueType::Unit, ValueType::I32, ValueType::HeapObject],
            function_table: functions
                .iter()
                .map(|f| FunctionRecord {
                    id: f.id,
                    identity: None,
                    name: f.name.clone(),
                    params: f.metadata.params.clone(),
                    return_type: f.metadata.return_type,
                    effects: f.metadata.effects,
                })
                .collect(),
            functions,
            ..Default::default()
        };
        let artifact = KbcArtifact::from_program(
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![
                    module,
                    BytecodeModule {
                        identity: vector_declaration.identity,
                        public_items: vector_declaration
                            .types
                            .into_iter()
                            .map(PublicItem::Type)
                            .collect(),
                        ..Default::default()
                    },
                ],
            },
            Default::default(),
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        decoded.validate_for_loader(&Default::default()).unwrap();
        let mut config = RuntimeConfig::default();
        config.async_limits.max_pending_operations = NonZeroUsize::new(1).unwrap();
        let mut runtime = Runtime::new(config);
        vector.install(&mut runtime).unwrap();
        native.install(&mut runtime).unwrap();
        let module = runtime
            .load_program("native-wait", decoded.program)
            .unwrap();
        Self {
            vm: Vm::new(runtime),
            module,
            sent,
            starts,
            cancels,
        }
    }

    fn cold(&self, input: i32) -> RootedValue {
        let array = ConversionContext::new(self.vm.runtime(), &self.module)
            .unwrap()
            .encode(vec![input])
            .unwrap();
        let execution = self
            .vm
            .start(
                &self.module,
                "create",
                &[array.value(self.vm.runtime().gc()).unwrap()],
                ExecutionOptions::default(),
            )
            .unwrap();
        let DriveResult::Complete(result) = self.vm.drive(&execution, slice()).unwrap() else {
            panic!("cold creation completes synchronously");
        };
        result.unwrap()
    }
}

fn slice() -> NonZeroUsize {
    NonZeroUsize::new(100).unwrap()
}

#[test]
fn async_wait_live_storage_and_iteration_contract() {
    let f = Fixture::with_functions(|functions| {
        let array = functions[0].metadata.semantic.params[&0].clone();
        let mut function = functions[1].clone();
        function.id = FunctionRef::new(2);
        function.name = "wait_with_lease".into();
        function.parameter_count = 3;
        function.local_count = 3;
        function.register_count = 5;
        function.metadata.params.extend([ValueType::HeapObject; 2]);
        function.metadata.locals = function.metadata.params.clone();
        function.metadata.registers.extend([
            ValueType::HeapObject,
            ValueType::HeapObject,
            ValueType::I32,
        ]);
        for index in [1, 2] {
            function
                .metadata
                .semantic
                .params
                .insert(index, array.clone());
            function
                .metadata
                .semantic
                .locals
                .insert(index, array.clone());
        }
        for index in [2, 3] {
            function
                .metadata
                .semantic
                .registers
                .insert(index, array.clone());
        }
        function
            .metadata
            .semantic
            .registers
            .insert(4, Ty::Builtin(BuiltinType::I32));
        function.metadata.roots =
            RootSlotLayout::from_types(&function.metadata.locals, &function.metadata.registers);
        let await_instruction = function.instructions[1].clone();
        function.instructions = vec![
            I::LoadLocal {
                dst: Register::new(3),
                local: LocalSlot::new(2),
            },
            I::LoadLocal {
                dst: Register::new(2),
                local: LocalSlot::new(1),
            },
            I::BeginIteration {
                collection: Register::new(2),
            },
            I::LoadLocal {
                dst: Register::new(0),
                local: LocalSlot::new(0),
            },
            await_instruction,
            I::Call {
                dst: Some(Register::new(4)),
                callee: CallTarget::Native(NativeImportId::new(1)),
                args: vec![Register::new(2), Register::new(1)],
            },
            I::EndIteration,
            I::Return(Some(Register::new(4))),
        ];
        functions.push(function);
    });
    for exit in 0..3 {
        let future = f.cold(12);
        let allocate = |value| {
            ConversionContext::new(f.vm.runtime(), &f.module)
                .unwrap()
                .encode(vec![value])
                .unwrap()
        };
        let live_root = allocate(41i32);
        let dead_root = allocate(99i32);
        let Value::GcHandle(live) = live_root.value(f.vm.runtime().gc()).unwrap() else {
            panic!("live Vec fixture")
        };
        let Value::GcHandle(dead) = dead_root.value(f.vm.runtime().gc()).unwrap() else {
            panic!("dead Vec fixture")
        };
        let execution =
            f.vm.start(
                &f.module,
                "wait_with_lease",
                &[
                    future.value(f.vm.runtime().gc()).unwrap(),
                    Value::GcHandle(live),
                    Value::GcHandle(dead),
                ],
                ExecutionOptions::default(),
            )
            .unwrap();
        drop(dead_root);
        assert!(matches!(
            f.vm.drive(&execution, slice()).unwrap(),
            DriveResult::Waiting
        ));
        f.vm.runtime().collect_garbage().unwrap();
        assert!(
            f.vm.runtime().gc().object_kind(dead).is_none(),
            "dead slots must not keep objects alive"
        );
        assert!(
            f.vm.runtime()
                .gc()
                .sequence_push(live, Value::I32(3))
                .is_err(),
            "iteration lease survives an actual wait"
        );
        f.vm.runtime()
            .gc()
            .sequence_set(live, 0, Value::I32(42))
            .unwrap();
        let (_, completion) = f.sent.lock().unwrap().pop().unwrap();
        match exit {
            0 => {
                assert_eq!(completion.complete(Ok(0)), CompletionStatus::Accepted);
                let DriveResult::Complete(result) = f.vm.drive(&execution, slice()).unwrap() else {
                    panic!("resumed Vec read");
                };
                assert_eq!(
                    result.unwrap().value(f.vm.runtime().gc()),
                    Some(Value::I32(42)),
                    "live aliases survive physical slot reuse"
                );
            }
            1 => {
                execution.cancel();
                assert!(matches!(
                    f.vm.drive(&execution, slice()).unwrap(),
                    DriveResult::Complete(Err(_))
                ));
            }
            _ => {
                drop(execution);
                assert_eq!(f.vm.runtime().drain_retired_executions().unwrap(), 1);
            }
        }
        assert_eq!(completion.complete(Ok(9)), CompletionStatus::Stale);
        f.vm.runtime()
            .gc()
            .sequence_push(live, Value::I32(3))
            .unwrap();
        assert_eq!(
            live_root.value(f.vm.runtime().gc()),
            Some(Value::GcHandle(live))
        );
        assert_eq!(f.vm.runtime().resources().counters().current_call_depth, 0);
    }
}

#[test]
fn async_native_completion_contract() {
    let f = Fixture::new();
    let future = f.cold(41);
    assert_eq!(f.starts.load(Ordering::Relaxed), 0);
    f.vm.runtime().collect_garbage().unwrap();
    let value = future.value(f.vm.runtime().gc()).unwrap();
    let execution =
        f.vm.start(
            &f.module,
            "wait",
            from_ref(&value),
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert!(!execution.is_ready());
    assert_eq!(f.starts.load(Ordering::Relaxed), 1);
    f.vm.runtime().collect_garbage().unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.starts.load(Ordering::Relaxed),
        1,
        "poll does not resubmit"
    );
    let (input, completion) = f.sent.lock().unwrap().pop().unwrap();
    assert_eq!(input, 41, "cold captures survive GC");
    std::thread::spawn(move || {
        assert_eq!(
            completion.complete(Ok(input + 1)),
            CompletionStatus::Accepted
        );
    })
    .join()
    .unwrap();
    assert!(execution.is_ready());
    let DriveResult::Complete(result) = f.vm.drive(&execution, slice()).unwrap() else {
        panic!("completion resumes");
    };
    assert_eq!(
        result.unwrap().value(f.vm.runtime().gc()),
        Some(Value::I32(42))
    );
    assert_eq!(f.cancels.load(Ordering::Relaxed), 0);
    let duplicate =
        f.vm.start(&f.module, "wait", &[value], ExecutionOptions::default())
            .unwrap();
    assert!(matches!(
        f.vm.drive(&duplicate, slice()).unwrap(),
        DriveResult::Complete(Err(_))
    ));
    assert_eq!(f.starts.load(Ordering::Relaxed), 1);

    for (input, expected) in [(0, 7), (-1, 8)] {
        let future = f.cold(input);
        let execution =
            f.vm.start(
                &f.module,
                "wait",
                &[future.value(f.vm.runtime().gc()).unwrap()],
                ExecutionOptions::default(),
            )
            .unwrap();
        let DriveResult::Complete(result) = f.vm.drive(&execution, slice()).unwrap() else {
            panic!("immediate completion");
        };
        assert_eq!(
            result.unwrap().value(f.vm.runtime().gc()),
            Some(Value::I32(expected))
        );
    }
    let future = f.cold(5);
    let execution =
        f.vm.start(
            &f.module,
            "wait",
            &[future.value(f.vm.runtime().gc()).unwrap()],
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let (_, completion) = f.sent.lock().unwrap().pop().unwrap();
    completion.complete(Ok(6));
    execution.cancel();
    let DriveResult::Complete(Err(VmError::RuntimeError(error))) =
        f.vm.drive(&execution, slice()).unwrap()
    else {
        panic!("cancellation wins before result publication");
    };
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(f.cancels.load(Ordering::Relaxed), 1);
    assert_eq!(completion.complete(Ok(9)), CompletionStatus::Stale);
}

#[test]
fn async_native_completion_contract_retirement_and_admission() {
    let f = Fixture::new();
    drop(f.cold(10));
    f.vm.runtime().collect_garbage().unwrap();
    assert_eq!(f.starts.load(Ordering::Relaxed), 0);
    assert_eq!(f.cancels.load(Ordering::Relaxed), 0);
    let cold = f.cold(11);
    let value = cold.value(f.vm.runtime().gc()).unwrap();
    assert!(
        Executor::new(
            f.vm.runtime(),
            &f.module,
            FunctionRef::new(1),
            from_ref(&value)
        )
        .is_err()
    );
    assert_eq!(
        f.starts.load(Ordering::Relaxed),
        0,
        "synchronous entry rejects before submission"
    );
    let first =
        f.vm.start(
            &f.module,
            "wait",
            from_ref(&value),
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&first, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let alias =
        f.vm.start(&f.module, "wait", &[value], ExecutionOptions::default())
            .unwrap();
    assert!(matches!(
        f.vm.drive(&alias, slice()).unwrap(),
        DriveResult::Complete(Err(_))
    ));
    let other = f.cold(12);
    let full =
        f.vm.start(
            &f.module,
            "wait",
            &[other.value(f.vm.runtime().gc()).unwrap()],
            ExecutionOptions::default(),
        )
        .unwrap();
    let DriveResult::Complete(Err(VmError::RuntimeError(error))) =
        f.vm.drive(&full, slice()).unwrap()
    else {
        panic!("capacity rejection");
    };
    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    assert_eq!(
        f.starts.load(Ordering::Relaxed),
        1,
        "capacity is reserved before submission"
    );
    let (_, endpoint) = f.sent.lock().unwrap().pop().unwrap();
    drop(first);
    assert_eq!(f.vm.runtime().drain_retired_executions().unwrap(), 1);
    assert_eq!(f.cancels.load(Ordering::Relaxed), 1);
    assert_eq!(endpoint.complete(Ok(22)), CompletionStatus::Stale);

    let failed = f.cold(-2);
    let execution =
        f.vm.start(
            &f.module,
            "wait",
            &[failed.value(f.vm.runtime().gc()).unwrap()],
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Complete(Err(_))
    ));
    assert_eq!(f.starts.load(Ordering::Relaxed), 2);
    let failed = f.cold(14);
    let execution =
        f.vm.start(
            &f.module,
            "wait",
            &[failed.value(f.vm.runtime().gc()).unwrap()],
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let (_, endpoint) = f.sent.lock().unwrap().pop().unwrap();
    endpoint.complete(Err(RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "provider failure",
    )));
    let DriveResult::Complete(Err(VmError::RuntimeError(error))) =
        f.vm.drive(&execution, slice()).unwrap()
    else {
        panic!("provider failure terminates its waiter");
    };
    assert_eq!(error.message(), "provider failure");
    assert_eq!(
        f.cancels.load(Ordering::Relaxed),
        1,
        "accepted completion disarms cancellation"
    );
    let last = f.cold(13);
    let execution =
        f.vm.start(
            &f.module,
            "wait",
            &[last.value(f.vm.runtime().gc()).unwrap()],
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let (_, endpoint) = f.sent.lock().unwrap().pop().unwrap();
    drop(f.vm);
    assert!(!execution.is_ready());
    assert_eq!(f.cancels.load(Ordering::Relaxed), 2);
    assert_eq!(endpoint.complete(Ok(23)), CompletionStatus::Stale);
}

#[test]
fn async_native_completion_contract_cleanup_quarantine() {
    let f = Fixture::new();
    let future = f.cold(-4);
    let execution =
        f.vm.start(
            &f.module,
            "wait",
            &[future.value(f.vm.runtime().gc()).unwrap()],
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let (_, endpoint) = f.sent.lock().unwrap().pop().unwrap();
    execution.cancel();
    assert!(f.vm.drive(&execution, slice()).is_err());
    assert!(f.vm.runtime().is_quarantined());
    assert_eq!(f.cancels.load(Ordering::Relaxed), 1);
    assert_eq!(endpoint.complete(Ok(1)), CompletionStatus::Stale);
}
