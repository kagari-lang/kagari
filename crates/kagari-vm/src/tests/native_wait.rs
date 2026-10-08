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
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::{Completion, CompletionStatus},
        future::NativeStart,
        registration::FunctionSpec,
        storage::NativeStorage,
        types::Type,
    },
    session::ExecutionOptions,
    value::Value,
};
use kagari_types::{
    callable::Signature, collection::CollectionAccess, scalar::BuiltinType, ty::Ty,
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

struct Fixture {
    vm: Vm,
    module: LoadedModule,
    sent: Arc<Mutex<Vec<(i32, Completion<i32>)>>>,
    starts: Arc<AtomicUsize>,
    cancels: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let starts = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));
        let mut builder = ModuleBuilder::new("test::io", &DeclarationCatalog::default());
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
        let native = builder.finish().unwrap();
        let declaration = native.to_declaration().unwrap();
        let native_declarations = declaration.native_declarations();
        let request = &native_declarations[0];
        let array = Ty::Array(
            Box::new(Ty::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable,
        );
        let functions = vec![
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
        let module = BytecodeModule {
            identity: declaration.identity.clone(),
            native_imports: vec![NativeImport {
                instance: ConcreteFunctionIdentity {
                    declaration: request.declaration.clone(),
                    arguments: vec![],
                },
                binding: request.declaration.clone(),
                signature: Signature {
                    params: vec![array],
                    result: future,
                },
                result_adapter: None,
                generic: None,
                requirements: vec![],
                callables: vec![],
                host: None,
            }],
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
                modules: vec![module],
            },
            Default::default(),
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        decoded.validate_for_loader(&Default::default()).unwrap();
        let mut config = RuntimeConfig::default();
        config.async_limits.max_pending_operations = NonZeroUsize::new(1).unwrap();
        let mut runtime = Runtime::new(config);
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
        let array = self
            .vm
            .runtime()
            .alloc_array(
                &self.module,
                Ty::Builtin(BuiltinType::I32),
                vec![Value::I32(input)],
            )
            .unwrap();
        let execution = self
            .vm
            .start(
                &self.module,
                "create",
                &[Value::Array(array)],
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
            &[value.clone()],
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
            &[value.clone()]
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
            &[value.clone()],
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
