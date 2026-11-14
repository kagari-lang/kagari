//! Source-free native Future/await contracts; source async lowering belongs to AX03.
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime, RuntimeConfig,
    error::{
        RuntimeErrorKind,
        reason::{ErrorReason, IndexError},
        trap::TrapReason,
    },
    module::ExecutableModule,
    native::{
        adapter::Async,
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::{Completion, CompletionStatus},
        context::NativeCall,
        conversion::{KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        future::NativeStart,
        storage::NativeStorage,
        types::Type,
        views::ValueHandle,
    },
    session::ExecutionOptions,
    value::{carriers::RootedValue, view::ValueView},
};
use crate::{
    error::RuntimeError,
    execution::{fixtures::FixtureExecution, owned::DriveResult},
    executor::Executor,
    interpreter_tests::common::round_trip_bytecode,
    test_support::native::program::ProgramFixture,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction as I, CallTarget, NativeImportId, Slot},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::ModuleRef,
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::{
    ids::FunctionRef,
    native_import::NativeImport,
    representation::semantic_representation,
    types::{
        ConcreteFunctionIdentity, PublicItem,
        named::{NamedContract, NamedDeclaration},
    },
};

use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty, visibility::Visibility};
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
        roots: RootSlotLayout::from_types(&[registers.to_vec(), params.to_vec()].concat()),
        params: params.clone(),
        return_type: semantic_representation(&result),
        slots: [registers, params].concat(),
        ..Default::default()
    };
    metadata.effects.may_suspend = suspends;
    metadata.semantic.params.insert(0, parameter.clone());
    metadata.semantic.slots.insert(2, parameter.clone());
    metadata.semantic.slots.insert(0, parameter);
    metadata.semantic.slots.insert(1, result.clone());
    metadata.semantic.result = Some(result);
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

type PendingReplies = Vec<(i32, Completion<i32>)>;

struct Fixture {
    vm: Runtime,
    module: ExecutableModule,
    sent: Arc<Mutex<PendingReplies>>,
    starts: Arc<AtomicUsize>,
    cancels: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_functions(|_| {})
    }

    fn with_functions(edit: impl FnOnce(&mut Vec<BytecodeFunction>)) -> Self {
        let context = DefinitionContext::new().unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let starts = Arc::new(AtomicUsize::new(0));
        let cancels = Arc::new(AtomicUsize::new(0));
        let mut builder = ModuleBuilder::new("test::io", &DeclarationCatalog::default()).unwrap();
        let mut future = builder.define_type("Future").unwrap();
        future.type_parameter("T").unwrap();
        future.native_storage(NativeStorage::future()).unwrap();
        let future = future.finish().unwrap().apply([Type::i32()]).unwrap();
        let future = DefinitionMetadata::checked(
            future.definitions().unwrap().clone(),
            future.abi().clone(),
            &Default::default(),
        )
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
        let (sent_to, started, cancelled) = (sent.clone(), starts.clone(), cancels.clone());
        {
            let registered_function = builder
                .define_function(
                    FunctionDecl::new("request")
                        .parameter(
                            "values",
                            <(i32,)>::kagari_type(builder.declarations()).unwrap(),
                        )
                        .returns(
                            builder
                                .declarations()
                                .future_type()
                                .unwrap()
                                .apply([Type::i32()])
                                .unwrap(),
                        ),
                )
                .unwrap();
            builder.bind::<Async<((i32,),)>, i32>(
                registered_function,
                move |(values,): ((i32,),), completion: Completion<i32>| {
                    started.fetch_add(1, Ordering::Relaxed);
                    let input = values.0;
                    if input == 0 {
                        return Ok(NativeStart::Ready(7));
                    }
                    if input == -1 {
                        completion.complete(Ok(8));
                    }
                    if input == -2 {
                        return Err(RuntimeError::from(TrapReason::Provider(
                            "submission failed".into(),
                        )));
                    }
                    sent_to.lock().unwrap().push((input, completion));
                    let cancelled = cancelled.clone();
                    Ok(NativeStart::cancellable(move || {
                        cancelled.fetch_add(1, Ordering::Relaxed);
                        assert_ne!(input, -4, "provider cleanup panic");
                    }))
                },
            )
        }
        .unwrap();
        {
            let registered_function = builder
                .define_function(
                    FunctionDecl::new("read")
                        .parameter("values", Type::array(Type::i32()).unwrap())
                        .parameter("index", Type::i32())
                        .returns(Type::i32()),
                )
                .unwrap();
            builder
                .bind(
                    registered_function.clone(),
                    |cx: &mut NativeCall<'_>,
                     values: ValueHandle<'_>,
                     index: i32|
                     -> NativeResult<i32> {
                        let values = values.value(cx)?;
                        cx.array_element(&values, index)?
                            .and_then(ValueView::as_i32)
                            .ok_or_else(|| {
                                RuntimeError::from(IndexError::SequenceIndexIsOutOfBounds)
                            })
                    },
                )
                .map(|()| registered_function)
        }
        .unwrap();
        let native = builder.finish().unwrap();
        let declaration = native
            .to_declaration()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
        let mut native_declarations = declaration
            .native_declarations(&context.snapshot())
            .unwrap();
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
                    I::Move {
                        dst: Slot::new(0),
                        src: Slot::new(2),
                    },
                    I::Call {
                        dst: Some(Slot::new(1)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![Slot::new(0)],
                    },
                    I::Return(Some(Slot::new(1))),
                ],
                false,
            ),
            function(
                1,
                "wait",
                future.clone(),
                Ty::Builtin(BuiltinType::I32),
                vec![
                    I::Move {
                        dst: Slot::new(0),
                        src: Slot::new(2),
                    },
                    I::Await {
                        dst: Slot::new(1),
                        value: Slot::new(0),
                        future: future.clone(),
                    },
                    I::Return(Some(Slot::new(1))),
                ],
                true,
            ),
        ];
        edit(&mut functions);
        let module = BytecodeModule {
            identity: declaration.identity.clone(),
            dependencies: vec![],
            native_imports: native_declarations
                .iter()
                .map(|entry| NativeImport {
                    receiver_access: entry.function.method_policy.receiver_access,
                    instance: ConcreteFunctionIdentity {
                        declaration: entry.declaration,
                        arguments: vec![],
                    },
                    binding: entry.declaration,
                    signature: Signature {
                        params: entry.function.params.iter().map(|p| p.ty.clone()).collect(),
                        result: entry.function.return_type.clone(),
                    },
                    result_adapter: None,
                    generic: None,
                    requirements: vec![],
                    callables: vec![],
                })
                .collect(),
            public_items: declaration
                .types
                .into_iter()
                .map(PublicItem::Type)
                .chain(declaration.functions.into_iter().map(PublicItem::Function))
                .collect(),
            named_declarations: native_declarations
                .iter()
                .map(|entry| NamedDeclaration {
                    declaration: entry.declaration,
                    visibility: Visibility::Public,
                    ambiguous_local_body: None,
                    contract: NamedContract::Function(entry.function.clone()),
                })
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
        let decoded = round_trip_bytecode(
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        );
        let mut config = RuntimeConfig::default();
        config.async_limits.max_pending_operations = NonZeroUsize::new(1).unwrap();
        let mut runtime = Runtime::new(config);
        native.install(&mut runtime).unwrap();
        let module = load_bytecode(&mut runtime, "native-wait", decoded).unwrap();
        let _module_program = module;
        let module = runtime.select_program(&_module_program).unwrap();
        Self {
            vm: runtime,
            module,
            sent,
            starts,
            cancels,
        }
    }

    fn cold(&mut self, input: i32) -> RootedValue {
        let array = ConversionContext::new(&mut self.vm, &self.module)
            .unwrap()
            .encode((input,))
            .unwrap();
        let execution = self
            .vm
            .start_fixture(
                &self.module,
                FunctionRef::new(0),
                from_ref(&array),
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
fn async_wait_live_storage_contract() {
    let mut f = Fixture::with_functions(|functions| {
        let array = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
        let mut function = functions[1].clone();
        function.id = FunctionRef::new(2);
        function.name = "wait_with_storage".into();
        function.parameter_count = 3;
        function.slot_count = 8;
        function.metadata.params.extend([ValueType::HeapObject; 2]);
        function.metadata.slots = vec![
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::HeapObject,
            ValueType::HeapObject,
        ];
        function.metadata.parameter_slots = (5..8).map(Slot::new).collect();
        for index in [1, 2] {
            function
                .metadata
                .semantic
                .params
                .insert(index, array.clone());
        }
        function.metadata.semantic.slots.retain(|&slot, _| slot < 2);
        for (&index, ty) in &function.metadata.semantic.params {
            function
                .metadata
                .semantic
                .slots
                .insert(5 + index, ty.clone());
        }
        for index in [2, 3] {
            function
                .metadata
                .semantic
                .slots
                .insert(index, array.clone());
        }
        function
            .metadata
            .semantic
            .slots
            .insert(4, Ty::Builtin(BuiltinType::I32));
        function.metadata.roots = RootSlotLayout::from_types(&function.metadata.slots);
        let await_instruction = function.instructions[1].clone();
        function.instructions = vec![
            I::Move {
                dst: Slot::new(3),
                src: Slot::new(7),
            },
            I::Move {
                dst: Slot::new(2),
                src: Slot::new(6),
            },
            I::Move {
                dst: Slot::new(0),
                src: Slot::new(5),
            },
            await_instruction,
            I::Call {
                dst: Some(Slot::new(4)),
                callee: CallTarget::Native(NativeImportId::new(1)),
                args: vec![Slot::new(2), Slot::new(1)],
            },
            I::Return(Some(Slot::new(4))),
        ];
        function.metadata.origins = (0..function.instructions.len())
            .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                instruction,
                span: Default::default(),
                line: None,
                column: None,
            })
            .collect();
        functions.push(function);
    });
    f.vm.collect_garbage().unwrap();
    let baseline = f.vm.gc_stats();
    for exit in 0..3 {
        let future = f.cold(12);
        let mut allocate = |value| {
            f.vm.alloc_array(
                &f.module,
                Ty::Builtin(BuiltinType::I32),
                &[RootedValue::from(value)],
            )
            .unwrap()
        };
        let live = allocate(41i32);
        let dead = allocate(99i32);
        let execution =
            f.vm.start_fixture(
                &f.module,
                FunctionRef::new(2),
                &[future, live.clone(), dead.clone()],
                ExecutionOptions::default(),
            )
            .unwrap();
        assert!(matches!(
            f.vm.drive(&execution, slice()).unwrap(),
            DriveResult::Waiting
        ));
        // Stabilize all unrelated garbage while this one Host root still owns
        // the dead operand. No execution/allocation occurs between these GCs.
        f.vm.collect_garbage().unwrap();
        let with_dead = f.vm.gc_stats();
        drop(dead);
        f.vm.collect_garbage().unwrap();
        assert_eq!(
            f.vm.gc_stats().objects,
            with_dead.objects - 1,
            "dead parked slots must not retain the scalar array"
        );
        assert_eq!(
            f.vm.gc_stats().root_registrations,
            with_dead.root_registrations - 1
        );
        {
            let mut cx = ConversionContext::new(&mut f.vm, &f.module).unwrap();
            let live = cx.import(&live).unwrap();
            cx.array_set(&live, 0, &ValueView::from(3i32)).unwrap();
            cx.array_set(&live, 0, &ValueView::from(42i32)).unwrap();
        }
        let (_, completion) = f.sent.lock().unwrap().pop().unwrap();
        match exit {
            0 => {
                assert_eq!(completion.complete(Ok(0)), CompletionStatus::Accepted);
                let DriveResult::Complete(result) = f.vm.drive(&execution, slice()).unwrap() else {
                    panic!("resumed array read");
                };
                assert_eq!(
                    result.unwrap().as_i32(),
                    Some(42),
                    "live aliases survive parking and storage reuse"
                );
            }
            1 => {
                execution.cancel();
                let DriveResult::Complete(Err(error)) = f.vm.drive(&execution, slice()).unwrap()
                else {
                    panic!("cancelled waiter");
                };
                assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
            }
            _ => {
                drop(execution);
                assert_eq!(f.vm.drain_retired_executions().unwrap(), 1);
            }
        }
        assert_eq!(completion.complete(Ok(9)), CompletionStatus::Stale);
        {
            let mut cx = ConversionContext::new(&mut f.vm, &f.module).unwrap();
            let live = cx.import(&live).unwrap();
            cx.array_set(&live, 0, &ValueView::from(3i32)).unwrap();
            assert_eq!(cx.array_len(&live).unwrap(), 1);
            assert_eq!(
                cx.array_element(&live, 0).unwrap().unwrap().as_i32(),
                Some(3)
            );
        }
        assert_eq!(f.vm.resources().counters().current_call_depth, 0);
    }
    f.vm.collect_garbage().unwrap();
    assert_eq!(f.vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        f.vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn async_native_completion_contract() {
    let mut f = Fixture::new();
    let future = f.cold(41);
    assert_eq!(f.starts.load(Ordering::Relaxed), 0);
    f.vm.collect_garbage().unwrap();
    let value = future;
    let execution =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
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
    f.vm.collect_garbage().unwrap();
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
    assert_eq!(result.unwrap().as_i32(), Some(42));
    assert_eq!(f.cancels.load(Ordering::Relaxed), 0);
    let duplicate =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            &[value],
            ExecutionOptions::default(),
        )
        .unwrap();
    let DriveResult::Complete(Err(error)) = f.vm.drive(&duplicate, slice()).unwrap() else {
        panic!("completed Future cannot be claimed twice");
    };
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(matches!(
        error.reason(),
        ErrorReason::Trap(TrapReason::FutureHasAlreadyBeenAwaited)
    ));
    assert_eq!(f.starts.load(Ordering::Relaxed), 1);

    for (input, expected) in [(0, 7), (-1, 8)] {
        let future = f.cold(input);
        let execution =
            f.vm.start_fixture(
                &f.module,
                FunctionRef::new(1),
                from_ref(&future),
                ExecutionOptions::default(),
            )
            .unwrap();
        let DriveResult::Complete(result) = f.vm.drive(&execution, slice()).unwrap() else {
            panic!("immediate completion");
        };
        assert_eq!(result.unwrap().as_i32(), Some(expected));
    }
    let future = f.cold(5);
    let execution =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            from_ref(&future),
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
    let DriveResult::Complete(Err(error)) = f.vm.drive(&execution, slice()).unwrap() else {
        panic!("cancellation wins before result publication");
    };
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(f.cancels.load(Ordering::Relaxed), 1);
    assert_eq!(completion.complete(Ok(9)), CompletionStatus::Stale);
}

#[test]
fn async_native_completion_contract_retirement_and_admission() {
    let mut f = Fixture::new();
    drop(f.cold(10));
    f.vm.collect_garbage().unwrap();
    assert_eq!(f.starts.load(Ordering::Relaxed), 0);
    assert_eq!(f.cancels.load(Ordering::Relaxed), 0);
    let cold = f.cold(11);
    let value = cold;
    {
        let mut cx = ConversionContext::new(&mut f.vm, &f.module).unwrap();
        let view = cx.import(&value).unwrap();
        let arguments = cx.argument_values(&[view]).unwrap();
        assert!(
            Executor::new(cx.runtime_mut(), &f.module, FunctionRef::new(1), arguments).is_err()
        );
    }
    assert_eq!(
        f.starts.load(Ordering::Relaxed),
        0,
        "synchronous entry rejects before submission"
    );
    let first =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            from_ref(&value),
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&first, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let alias =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            &[value],
            ExecutionOptions::default(),
        )
        .unwrap();
    let DriveResult::Complete(Err(error)) = f.vm.drive(&alias, slice()).unwrap() else {
        panic!("pending Future cannot be claimed by a second waiter");
    };
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(matches!(
        error.reason(),
        ErrorReason::Trap(TrapReason::FutureHasAlreadyBeenAwaited)
    ));
    let other = f.cold(12);
    let full =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            from_ref(&other),
            ExecutionOptions::default(),
        )
        .unwrap();
    let DriveResult::Complete(Err(error)) = f.vm.drive(&full, slice()).unwrap() else {
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
    assert_eq!(f.vm.drain_retired_executions().unwrap(), 1);
    assert_eq!(f.cancels.load(Ordering::Relaxed), 1);
    assert_eq!(endpoint.complete(Ok(22)), CompletionStatus::Stale);

    let failed = f.cold(-2);
    let execution =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            from_ref(&failed),
            ExecutionOptions::default(),
        )
        .unwrap();
    let DriveResult::Complete(Err(error)) = f.vm.drive(&execution, slice()).unwrap() else {
        panic!("submission failure terminates its waiter");
    };
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(
        matches!(error.reason(), ErrorReason::Trap(TrapReason::Provider(message)) if message == "submission failed")
    );
    assert_eq!(f.starts.load(Ordering::Relaxed), 2);
    let failed = f.cold(14);
    let execution =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            from_ref(&failed),
            ExecutionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        f.vm.drive(&execution, slice()).unwrap(),
        DriveResult::Waiting
    ));
    let (_, endpoint) = f.sent.lock().unwrap().pop().unwrap();
    endpoint.complete(Err(RuntimeError::from(TrapReason::Provider(
        "provider failure".into(),
    ))));
    let DriveResult::Complete(Err(error)) = f.vm.drive(&execution, slice()).unwrap() else {
        panic!("provider failure terminates its waiter");
    };
    assert_eq!(error.to_string(), "provider failure");
    assert_eq!(
        f.cancels.load(Ordering::Relaxed),
        1,
        "accepted completion disarms cancellation"
    );
    let last = f.cold(13);
    let execution =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            from_ref(&last),
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
    let mut f = Fixture::new();
    let future = f.cold(-4);
    let execution =
        f.vm.start_fixture(
            &f.module,
            FunctionRef::new(1),
            from_ref(&future),
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
    assert!(f.vm.is_quarantined());
    assert_eq!(f.cancels.load(Ordering::Relaxed), 1);
    assert_eq!(endpoint.complete(Ok(1)), CompletionStatus::Stale);
}
