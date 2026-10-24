//! Owned retirement must release every parked execution despite host destructor faults.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::{ExecutionStack, types::arguments::TypeArgument},
    module::{ExecutableModule, Program},
    native::{
        adapter::Async,
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::{Completion, CompletionRegistry, CompletionStatus},
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        future::NativeStart,
        installation::NativeInstallation,
        module::NativeModule,
        storage::NativeStorage,
        types::Type,
    },
    session::owned::OwnedExecution,
    test_support::native::{
        invoke::invoke,
        program::{ProgramFixture, free_function_program},
    },
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction as I, CallTarget, NativeImportId, Slot},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
        RootSlotLayout,
    },
    program::ModuleRef,
};
use kagari_common::{cancellation::CancellationToken, identity::map::DefinitionContext};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_stdlib::catalog;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Poll, Wake, Waker},
};

#[derive(Default)]
struct Probe {
    cancelled: [AtomicUsize; 2],
    converted: AtomicUsize,
    payload_drops: AtomicUsize,
    hook_drops: AtomicUsize,
    continued: AtomicUsize,
    sent: Mutex<Vec<Completion<Reply>>>,
}

struct Reply {
    probe: Arc<Probe>,
    panic_on_drop: bool,
}

impl KagariType for Reply {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(Type::i32())
    }
}

impl IntoKagari for Reply {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        self.probe.converted.fetch_add(1, Ordering::SeqCst);
        cx.encode_value(expected, 7_i32)
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.probe.payload_drops.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic_on_drop, "queued reply destructor");
    }
}

struct HookCapture(Arc<Probe>);

impl Drop for HookCapture {
    fn drop(&mut self) {
        self.0.hook_drops.fetch_add(1, Ordering::SeqCst);
        panic!("successful poll discards cancellation capture");
    }
}

struct ControlWake(Arc<AtomicUsize>);

#[expect(
    clippy::manual_noop_waker,
    reason = "The owned waker's destructor is the failure under test"
)]
impl Wake for ControlWake {
    fn wake(self: Arc<Self>) {}

    fn wake_by_ref(self: &Arc<Self>) {}
}

impl Drop for ControlWake {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("retired host control waker destructor");
    }
}

#[test]
fn owned_initializer_unwind_releases_session_and_cancellation_owner_immediately() {
    let f = Fixture::new(false);
    let session = f
        .runtime
        .begin_execution(&f.module, Default::default())
        .unwrap();
    let id = session.id;
    let external = CancellationToken::default();
    let mut owner = None;
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = f
            .runtime
            .initialize_owned_factory(session, external.clone(), || {
                let state = f.runtime.resources().active_session().unwrap();
                owner = Some(Arc::downgrade(state.owner.borrow().as_ref().unwrap()));
                panic!("admission conversion unwound");
            });
    }))
    .expect_err("preserve the original panic");
    assert_eq!(
        panic.downcast_ref::<&str>(),
        Some(&"admission conversion unwound")
    );
    assert!(f.runtime.resources().active_session().is_none());
    assert!(
        f.runtime.resources().sessions.get(id).is_none(),
        "no parked session remains"
    );
    assert!(
        owner.unwrap().upgrade().is_none(),
        "owner and its two cancellation subscriptions must be released"
    );
    assert_eq!(
        f.runtime.resources().frame_values.borrow().active_windows(),
        0
    );
    assert_eq!(f.runtime.resources().counters().current_call_depth, 0);
    assert_eq!(f.runtime.gc().active_roots(), 0);
    external.cancel();
    assert!(f.runtime.check_scheduler_health().is_ok());
}

struct Fixture {
    runtime: Runtime,
    program: Program,
    module: ExecutableModule,
    request: usize,
    probe: Arc<Probe>,
}

impl Fixture {
    fn new(panic_hook_capture: bool) -> Self {
        let probe = Arc::new(Probe::default());
        let declarations = catalog::declarations().unwrap();
        let catalog = DeclarationCatalog::from_declarations(&declarations).unwrap();
        let future = catalog.future_type().unwrap().apply([Type::i32()]).unwrap();
        let mut native = ModuleBuilder::new("test::owned_cleanup", &catalog).unwrap();
        let request = native
            .define_function(
                FunctionDecl::new("request")
                    .parameter("index", Type::i32())
                    .returns(future),
            )
            .unwrap();
        let submitted = probe.clone();
        native
            .bind::<Async<(i32,)>, Reply>(
                request,
                move |(index,): (i32,), completion: Completion<Reply>| {
                    submitted.sent.lock().unwrap().push(completion);
                    let probe = submitted.clone();
                    let capture = panic_hook_capture.then(|| HookCapture(probe.clone()));
                    Ok(NativeStart::cancellable(move || {
                        probe.cancelled[index as usize].fetch_add(1, Ordering::SeqCst);
                        drop(capture);
                    }))
                },
            )
            .unwrap();
        let continued = native
            .define_function(
                FunctionDecl::new("continued")
                    .parameter("saved", Type::scalar(BuiltinType::String))
                    .returns(Type::unit()),
            )
            .unwrap();
        let continuation = probe.clone();
        native
            .bind(
                continued,
                NativeBinding::new(
                    vec![Codec::Exact(Type::scalar(BuiltinType::String))],
                    Codec::Exact(Type::unit()),
                    move |_| {
                        continuation.continued.fetch_add(1, Ordering::SeqCst);
                        Ok(Value::Unit)
                    },
                ),
            )
            .unwrap();
        let native = native.finish().unwrap();
        let definitions = DefinitionContext::new().unwrap();
        let (snapshot, mut program) = free_function_program(&native.to_declaration())
            .import_into(&definitions, &Default::default())
            .unwrap()
            .into_parts();
        let application = &mut program.modules[0];
        let request = application
            .native_declarations
            .iter()
            .position(|decl| decl.function.name == "request")
            .unwrap();
        let continued = application
            .native_declarations
            .iter()
            .position(|decl| decl.function.name == "continued")
            .unwrap();
        let future = application.native_imports[request].signature.result.clone();
        let Ty::NativeObject(nominal) = &future else {
            panic!("nominal Future")
        };
        let owner = snapshot
            .resolve(nominal.declaration)
            .unwrap()
            .module()
            .clone();
        let foundation_declaration = catalog::shared()
            .unwrap()
            .into_iter()
            .find(|decl| decl.records().identity == owner)
            .unwrap();
        let foundation_native = NativeModule::checked(
            foundation_declaration
                .import_into(catalog.types.context(), &Default::default())
                .unwrap()
                .into_records(),
            vec![],
            [(catalog.future_type().unwrap().id(), NativeStorage::future())].into(),
            vec![],
            &catalog,
        )
        .unwrap();
        let foundation = foundation_declaration
            .import_into(&definitions, &Default::default())
            .unwrap()
            .into_records();
        let slots = vec![ValueType::HeapObject, ValueType::Str, ValueType::I32];
        let mut metadata = FunctionMetadata {
            params: slots[..2].to_vec(),
            parameter_slots: vec![Slot::new(0), Slot::new(1)],
            return_type: ValueType::I32,
            roots: RootSlotLayout::from_types(&slots),
            slots,
            ..Default::default()
        };
        metadata.effects.may_suspend = true;
        metadata.semantic.params =
            [(0, future.clone()), (1, Ty::Builtin(BuiltinType::String))].into();
        metadata.semantic.slots = [
            (0, future.clone()),
            (1, Ty::Builtin(BuiltinType::String)),
            (2, Ty::Builtin(BuiltinType::I32)),
        ]
        .into();
        metadata.semantic.result = Some(Ty::Builtin(BuiltinType::I32));
        let instructions = vec![
            I::Await {
                dst: Slot::new(2),
                value: Slot::new(0),
                future,
            },
            I::Call {
                dst: None,
                callee: CallTarget::Native(NativeImportId::new(continued)),
                args: vec![Slot::new(1)],
            },
            I::Return(Some(Slot::new(2))),
        ];
        metadata.origins = (0..instructions.len())
            .map(|instruction| InstructionOrigin {
                instruction,
                span: Default::default(),
                line: None,
                column: None,
            })
            .collect();
        let function = BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "wait".into(),
            parameter_count: 2,
            slot_count: 3,
            metadata,
            instructions,
        };
        application.types = vec![
            ValueType::Unit,
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::Str,
        ];
        application.dependencies.push(ModuleRef::new(1));
        application.function_table.push(FunctionRecord {
            id: function.id,
            identity: None,
            name: function.name.clone(),
            params: function.metadata.params.clone(),
            return_type: function.metadata.return_type,
            effects: function.metadata.effects,
        });
        application.functions.push(function);
        program.modules.push(BytecodeModule {
            identity: owner,
            public_items: foundation.types.into_iter().map(PublicItem::Type).collect(),
            types: vec![ValueType::Unit],
            ..Default::default()
        });
        let modules = [foundation_native, native];
        let mut runtime = NativeInstallation::prepare(&modules)
            .unwrap()
            .runtime(Default::default());
        let module_program = runtime
            .load_program(
                "owned-cleanup",
                ProgramFixture::from(program)
                    .metadata(definitions.snapshot(), &Default::default())
                    .unwrap(),
            )
            .unwrap();
        let module = runtime.select_program(&module_program).unwrap();
        Self {
            runtime,
            program: module_program,
            module,
            request,
            probe,
        }
    }

    fn park(&self, index: i32) -> (OwnedExecution, Value) {
        self.park_after_poll(index, |_| {})
    }

    fn park_after_poll(
        &self,
        index: i32,
        after_poll: impl FnOnce(&OwnedExecution),
    ) -> (OwnedExecution, Value) {
        let future = invoke(
            &self.runtime,
            &self.module,
            self.request,
            &[Value::I32(index)],
        )
        .unwrap();
        let future_root = self.runtime.root_value(future).unwrap();
        let saved = self
            .runtime
            .gc()
            .alloc_string(format!("live parked frame {index}"))
            .unwrap();
        let owner = self
            .runtime
            .start_owned_execution(
                &self.module,
                FunctionRef::new(0),
                &[future, saved],
                Default::default(),
            )
            .unwrap();
        drop(future_root);
        let stack = self.runtime.resume_owned_execution(&owner).unwrap();
        let I::Await { dst, value, future } =
            stack.current_mut().unwrap().next_instruction().unwrap()
        else {
            panic!("fixture starts with await");
        };
        let value = stack
            .current()
            .unwrap()
            .read_slot(&self.runtime, value)
            .unwrap();
        stack
            .begin_await(&self.runtime, value, dst, &future)
            .unwrap();
        assert!(matches!(
            stack.poll_await(&self.runtime).unwrap(),
            Poll::Pending
        ));
        assert_eq!(self.runtime.resources().counters().current_call_depth, 1);
        after_poll(&owner);
        ExecutionStack::park_waiting(&mut Some(stack), &self.runtime).unwrap();
        self.runtime.collect_garbage().unwrap();
        assert!(
            self.runtime.gc().validate_value(&saved),
            "only the parked frame retains this string"
        );
        assert_eq!(
            self.runtime
                .resources()
                .sessions
                .get(owner.id)
                .unwrap()
                .parked_depth
                .get(),
            1
        );
        (owner, saved)
    }

    fn registry(&self) -> CompletionRegistry {
        self.runtime
            .operations
            .get()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone()
    }

    fn take_completions(&self) -> Vec<Completion<Reply>> {
        std::mem::take(&mut *self.probe.sent.lock().unwrap())
    }

    fn assert_released(&self, registry: &CompletionRegistry) {
        assert_eq!(self.runtime.gc().active_roots(), 0);
        assert_eq!(
            self.runtime
                .resources()
                .frame_values
                .borrow()
                .active_windows(),
            0
        );
        assert_eq!(self.runtime.resources().counters().current_call_depth, 0);
        assert!(self.runtime.resources().active_session().is_none());
        assert_eq!(registry.active_count(), 0);
        assert_eq!(self.probe.converted.load(Ordering::SeqCst), 0);
        assert_eq!(self.probe.continued.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn pending_commit_preserves_completion_and_cancellation_notifications() {
    for mode in 0..3 {
        let f = Fixture::new(false);
        let (owner, _) = f.park_after_poll(0, |owner| {
            assert!(
                !owner.is_ready(),
                "the pending poll has no notification yet"
            );
            if mode == 1 {
                let completion = f.take_completions().pop().unwrap();
                assert_eq!(
                    completion.complete(Ok(Reply {
                        probe: f.probe.clone(),
                        panic_on_drop: false,
                    })),
                    CompletionStatus::Accepted
                );
            } else if mode == 2 {
                owner.cancel();
            }
            assert_eq!(owner.is_ready(), mode != 0);
        });
        assert_eq!(
            owner.is_ready(),
            mode != 0,
            "commit neither loses nor invents readiness"
        );
        let stack = f.runtime.resume_owned_execution(&owner).unwrap();
        let result = stack.poll_await(&f.runtime);
        match mode {
            0 => assert!(matches!(result.unwrap(), Poll::Pending)),
            1 => {
                assert!(matches!(result.unwrap(), Poll::Ready(None)));
                assert_eq!(f.probe.converted.load(Ordering::SeqCst), 1);
            }
            _ => assert_eq!(result.unwrap_err().kind(), RuntimeErrorKind::Cancelled),
        }
        drop(stack);
        f.runtime.finish_owned_execution(&owner).unwrap();
        assert!(!owner.is_ready());
        assert_eq!(f.registry().active_count(), 0);
        assert_eq!(f.runtime.resources().counters().current_call_depth, 0);
    }
}

#[test]
fn publication_queues_completions_without_polling_a_parked_future() {
    let f = Fixture::new(false);
    let (owner, saved) = f.park(0);
    let completion = f.take_completions().pop().unwrap();
    let guard = f.runtime.begin_publication(&f.program).unwrap();
    assert_eq!(
        completion.complete(Ok(Reply {
            probe: f.probe.clone(),
            panic_on_drop: false,
        })),
        CompletionStatus::Accepted
    );
    assert!(owner.is_ready());
    assert_eq!(
        f.runtime
            .resume_owned_execution(&owner)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(f.probe.converted.load(Ordering::SeqCst), 0);
    assert_eq!(f.probe.continued.load(Ordering::SeqCst), 0);
    assert_eq!(f.probe.cancelled[0].load(Ordering::SeqCst), 0);
    assert!(f.runtime.gc().validate_value(&saved));
    let candidate = guard
        .stage_reload_verified_program(None, f.module.verified_program().clone())
        .unwrap();
    guard.publish(candidate).unwrap();
    assert_eq!(f.probe.converted.load(Ordering::SeqCst), 0);
    let stack = f.runtime.resume_owned_execution(&owner).unwrap();
    assert!(matches!(
        stack.poll_await(&f.runtime).unwrap(),
        Poll::Ready(None)
    ));
    assert_eq!(f.probe.converted.load(Ordering::SeqCst), 1);
    drop(stack);
    f.runtime.finish_owned_execution(&owner).unwrap();
    assert_eq!(f.registry().active_count(), 0);
    assert_eq!(f.runtime.resources().counters().current_call_depth, 0);
    assert!(!f.runtime.is_quarantined());
}

fn assert_stale(completions: &[Completion<Reply>]) {
    for completion in completions {
        assert_eq!(
            completion.complete(Err(RuntimeError::module_validation("late reply"))),
            CompletionStatus::Stale
        );
    }
}

#[test]
fn owned_retirement_drains_all_parked_roots_after_payload_drop_fault() {
    let f = Fixture::new(false);
    let (first, first_saved) = f.park(0);
    let (second, second_saved) = f.park(1);
    let ids = [first.id, second.id];
    let registry = f.registry();
    let completions = f.take_completions();
    assert_eq!(registry.active_count(), 2);
    assert_eq!(
        f.runtime.resources().frame_values.borrow().active_windows(),
        2
    );
    assert!(f.runtime.gc().validate_value(&first_saved));
    assert!(f.runtime.gc().validate_value(&second_saved));
    assert_eq!(
        completions[0].complete(Ok(Reply {
            probe: f.probe.clone(),
            panic_on_drop: true
        })),
        CompletionStatus::Accepted
    );
    // The first fault must also retire the sibling whose host owner is still retained.
    drop(first);
    let error = f.runtime.drain_retired_executions().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(f.runtime.is_quarantined());
    assert_eq!(
        registry.check().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        f.probe
            .cancelled
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst)),
        [1, 1]
    );
    assert_eq!(f.probe.payload_drops.load(Ordering::SeqCst), 1);
    for id in ids {
        assert!(f.runtime.resources().sessions.get(id).is_none());
    }
    assert!(!second.is_ready());
    f.assert_released(&registry);
    assert_stale(&completions);
    assert_eq!(f.runtime.drain_retired_executions().unwrap(), 0);
    drop(second);
    assert_eq!(f.runtime.drain_retired_executions().unwrap(), 0);
    assert_eq!(
        f.probe
            .cancelled
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst)),
        [1, 1]
    );
    assert_eq!(f.probe.payload_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn owned_retirement_drains_all_parked_roots_after_control_waker_drop_fault() {
    let f = Fixture::new(false);
    let (first, first_saved) = f.park(0);
    let (second, second_saved) = f.park(1);
    let ids = [first.id, second.id];
    let registry = f.registry();
    let completions = f.take_completions();
    let dropped = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(ControlWake(dropped.clone())));
    first.set_waker(&waker);
    drop(waker);
    assert_eq!(registry.active_count(), 2);
    assert_eq!(
        f.runtime.resources().frame_values.borrow().active_windows(),
        2
    );
    assert!(f.runtime.gc().validate_value(&first_saved));
    assert!(f.runtime.gc().validate_value(&second_saved));
    drop(first);
    // Notifications are harmless; only retirement owns the last control-waker reference.
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.probe
            .cancelled
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst)),
        [0, 0]
    );

    let error = f.runtime.drain_retired_executions().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(f.runtime.is_quarantined());
    registry.check().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.probe
            .cancelled
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst)),
        [1, 1]
    );
    for id in ids {
        assert!(f.runtime.resources().sessions.get(id).is_none());
    }
    assert!(!second.is_ready());
    f.assert_released(&registry);
    assert_stale(&completions);
    assert_eq!(f.runtime.drain_retired_executions().unwrap(), 0);
    drop(second);
    assert_eq!(f.runtime.drain_retired_executions().unwrap(), 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.probe
            .cancelled
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst)),
        [1, 1]
    );
}

#[test]
fn owned_successful_poll_hook_drop_fault_prevents_conversion_and_continuation() {
    let f = Fixture::new(true);
    let (owner, _) = f.park(0);
    let registry = f.registry();
    let completions = f.take_completions();
    assert_eq!(
        completions[0].complete(Ok(Reply {
            probe: f.probe.clone(),
            panic_on_drop: false
        })),
        CompletionStatus::Accepted
    );
    let stack = f.runtime.resume_owned_execution(&owner).unwrap();
    let error = stack.poll_await(&f.runtime).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(f.runtime.is_quarantined());
    assert_eq!(
        stack.current_mut().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    drop(stack);
    f.runtime.finish_owned_execution(&owner).unwrap();
    drop(owner);
    assert_eq!(f.probe.hook_drops.load(Ordering::SeqCst), 1);
    assert_eq!(f.probe.payload_drops.load(Ordering::SeqCst), 1);
    assert_eq!(f.probe.cancelled[0].load(Ordering::SeqCst), 0);
    f.assert_released(&registry);
    assert_stale(&completions);
    assert_eq!(f.runtime.drain_retired_executions().unwrap(), 0);
}

#[test]
fn owned_runtime_teardown_contains_queued_payload_drop_fault() {
    let f = Fixture::new(false);
    let (first, _) = f.park(0);
    let (second, _) = f.park(1);
    let registry = f.registry();
    let probe = f.probe.clone();
    let resources = f.runtime.resources().lifetime_probe();
    let completions = f.take_completions();
    let dropped = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(ControlWake(dropped.clone())));
    first.set_waker(&waker);
    drop(waker);
    assert_eq!(
        completions[0].complete(Ok(Reply {
            probe: probe.clone(),
            panic_on_drop: true
        })),
        CompletionStatus::Accepted
    );
    // Retain both owners: teardown must contain payload and control-waker destruction
    // in the first session and still cancel the second session.
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert!(catch_unwind(AssertUnwindSafe(|| drop(f))).is_ok());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(!first.is_ready());
    assert!(!second.is_ready());
    assert!(
        resources.upgrade().is_none(),
        "external owners must not retain frame storage"
    );
    assert_eq!(registry.active_count(), 0);
    assert_eq!(
        registry.check().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        probe
            .cancelled
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst)),
        [1, 1]
    );
    assert_eq!(probe.payload_drops.load(Ordering::SeqCst), 1);
    assert_eq!(probe.converted.load(Ordering::SeqCst), 0);
    assert_eq!(probe.continued.load(Ordering::SeqCst), 0);
    assert_stale(&completions);
    drop((first, second));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(probe.payload_drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        probe
            .cancelled
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst)),
        [1, 1]
    );
}
