use super::load;
use crate::{
    Runtime, StagedReload,
    closure::{ClosureTarget, ClosureValueSnapshot},
    error::{RuntimeError, RuntimeErrorKind},
    frame::{ExecutionFrame, ExecutionStack},
    host::HostCallContext,
    module::{ExecutableModule, Program},
    native::{
        builder::ModuleBuilder, callable::PreparedClosure, declarations::FunctionDecl,
        sequence::SequencePayload,
    },
    reload::ReloadValidationError,
    session::{ExecutionEvent, ExecutionObserver},
    test_support::native::{
        invoke::invoke,
        program::{ProgramFixture, free_function_program},
    },
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::BytecodeInstruction,
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
    },
    program::ModuleRef,
};
use kagari_contract::ids::FunctionRef;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects},
    scalar::BuiltinType,
    ty::Ty,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Waker},
};

fn stage(runtime: &Runtime, target: &Program) -> StagedReload {
    runtime
        .stage_reload_verified_program(
            target,
            None,
            runtime
                .select_program(target)
                .unwrap()
                .verified_program()
                .clone(),
        )
        .unwrap()
}

fn rejected(runtime: &Runtime, target: &Program) {
    assert_eq!(
        runtime
            .begin_publication(target)
            .err()
            .expect("publication must reject")
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation,
    );
}

#[test]
fn publication_counts_nested_and_hidden_sessions_not_just_the_visible_root() {
    let mut runtime = Runtime::default();
    let target = load(&mut runtime, "target");
    let other = load(&mut runtime, "other");
    let module = runtime.select_program(&target).unwrap();
    let candidate = stage(&runtime, &target);
    let other_candidate = stage(&runtime, &other);
    let outer = runtime
        .begin_execution(&module, Default::default())
        .unwrap();
    let nested = runtime
        .begin_execution(&module, Default::default())
        .unwrap();
    drop(outer);
    rejected(&runtime, &other);

    // Candidate entry temporarily hides the ordinary session; neither identity
    // nor the visible root is publication authority for any installation.
    let candidate_session = runtime.begin_candidate_initialization(&candidate).unwrap();
    rejected(&runtime, &target);
    assert!(matches!(runtime.publish_staged_reload(other_candidate),
        Err(ReloadValidationError::Runtime(error))
            if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation));
    drop(candidate_session);
    rejected(&runtime, &target);

    let hidden = runtime.resources().replace_session(None);
    rejected(&runtime, &target);
    runtime.resources().replace_session(hidden);
    drop(nested);
    runtime.publish_staged_reload(candidate).unwrap();
    assert_ne!(runtime.select_program(&target).unwrap().key(), module.key());
    assert!(!runtime.is_quarantined());
}

#[test]
fn publication_guard_excludes_entry_and_releases_on_errors_and_unwind() {
    let mut runtime = Runtime::default();
    let target = load(&mut runtime, "target");
    let other = load(&mut runtime, "same-label-is-not-authority");
    let module = runtime.select_program(&target).unwrap();
    let candidate = stage(&runtime, &target);
    let guard = runtime.begin_publication(&target).unwrap();
    rejected(&runtime, &target);
    rejected(&runtime, &other);
    assert_eq!(
        runtime
            .begin_execution(&module, Default::default())
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        runtime
            .begin_candidate_initialization(&candidate)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(candidate.initialization_error().is_none());
    assert_eq!(
        runtime.close().unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(runtime.select_program(&target).is_ok());
    drop(guard);

    let wrong = stage(&runtime, &other);
    assert!(
        runtime
            .begin_publication(&target)
            .unwrap()
            .publish(wrong)
            .is_err()
    );
    assert_eq!(runtime.select_program(&target).unwrap().key(), module.key());
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        let _guard = runtime.begin_publication(&target).unwrap();
        panic!("abandoned source preparation");
    }));
    assert!(unwound.is_err());
    runtime
        .begin_publication(&target)
        .unwrap()
        .publish(candidate)
        .unwrap();

    let stale = stage(&runtime, &target);
    let guard = runtime.begin_publication(&target).unwrap();
    let next = guard
        .stage_reload_verified_program(
            None,
            runtime
                .select_program(&target)
                .unwrap()
                .verified_program()
                .clone(),
        )
        .unwrap();
    guard.publish(next).unwrap();
    assert!(runtime.publish_staged_reload(stale).is_err());
    drop(runtime.begin_publication(&target).unwrap());
    runtime.close().unwrap();
    assert!(runtime.begin_publication(&target).is_err());
    assert!(runtime.select_program(&target).is_err());
}

fn parked_program(runtime: &mut Runtime) -> Program {
    let function = BytecodeFunction {
        id: FunctionRef::new(0),
        name: "parked".into(),
        metadata: FunctionMetadata {
            origins: vec![InstructionOrigin {
                instruction: 0,
                span: Default::default(),
                line: None,
                column: None,
            }],
            ..Default::default()
        },
        instructions: vec![BytecodeInstruction::Return(None)],
        ..Default::default()
    };
    let program = ProgramFixture {
        root: ModuleRef::new(0),
        exports: Default::default(),
        modules: vec![BytecodeModule {
            types: vec![ValueType::Unit],
            function_table: vec![FunctionRecord {
                id: function.id,
                identity: function.identity.clone(),
                name: function.name.clone(),
                params: vec![],
                return_type: function.metadata.return_type,
                effects: Default::default(),
            }],
            functions: vec![function],
            ..Default::default()
        }],
    }
    .metadata(runtime.host().definitions(), &Default::default())
    .unwrap();
    runtime.load_program("parked", program).unwrap()
}

#[test]
fn publication_allows_parked_frames_but_excludes_resume_and_scheduler_turns() {
    let mut runtime = Runtime::default();
    let target = parked_program(&mut runtime);
    let module = runtime.select_program(&target).unwrap();
    let closure = Value::Closure(
        runtime
            .gc()
            .alloc_closure(ClosureValueSnapshot {
                environment: None,
                implementation: module.clone(),
                target: ClosureTarget::Script(FunctionRef::new(0)),
                captures: vec![],
            })
            .unwrap(),
    );
    let closure_root = runtime.root_value(closure).unwrap();
    let closure = PreparedClosure::from_value(&runtime, closure).unwrap();
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let guard = runtime.begin_publication(&target).unwrap();
    let cx = Context::from_waker(Waker::noop());
    assert_eq!(
        runtime.resume_owned_execution(&owner).err().unwrap().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        runtime
            .begin_scheduler_turn(&cx, None)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        runtime.drain_retired_executions().unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    let candidate = guard
        .stage_reload_verified_program(None, module.verified_program().clone())
        .unwrap();
    assert_eq!(
        runtime
            .enter_closure_execution_stack(&closure)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    guard.publish(candidate).unwrap();
    let closure_stack = runtime.enter_closure_execution_stack(&closure).unwrap();
    rejected(&runtime, &target);
    drop(closure_stack);
    assert!(closure_root.value(runtime.gc()).is_some());
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    rejected(&runtime, &target);
    assert!(ExecutionStack::try_park_ready(&mut activation, &runtime).unwrap());
    drop(runtime.begin_publication(&target).unwrap());

    let turn = runtime.begin_scheduler_turn(&cx, None).unwrap();
    rejected(&runtime, &target);
    assert!(
        turn.next_work().unwrap().is_none(),
        "the turn must not reject itself"
    );
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    assert!(ExecutionStack::try_park_ready(&mut activation, &runtime).unwrap());
    drop(turn);
    drop(runtime.begin_publication(&target).unwrap());
    runtime.finish_owned_execution(&owner).unwrap();
    runtime.close().unwrap();
}

#[test]
fn publication_rechecks_native_borrows_and_rejects_foreign_targets() {
    let mut runtime = Runtime::default();
    let target = load(&mut runtime, "same-label");
    let mut foreign = Runtime::default();
    let foreign_target = load(&mut foreign, "same-label");
    assert_eq!(
        runtime
            .begin_publication(&foreign_target)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::StaleHandle
    );
    let module = runtime.select_program(&target).unwrap();
    let array = runtime
        .alloc_array(&module, Ty::Builtin(BuiltinType::Unit), vec![])
        .unwrap();
    runtime
        .gc()
        .with_native::<SequencePayload, _>(array, |_| {
            assert_eq!(
                runtime.begin_publication(&target).err().unwrap().kind(),
                RuntimeErrorKind::ModuleValidation
            );
            Ok(())
        })
        .unwrap();
    let guard = runtime.begin_publication(&target).unwrap();
    let candidate = guard
        .stage_reload_verified_program(None, module.verified_program().clone())
        .unwrap();
    runtime.gc().with_native::<SequencePayload, _>(array, |_| {
        assert!(matches!(guard.publish(candidate),
            Err(ReloadValidationError::Runtime(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
        Ok(())
    }).unwrap();
    assert_eq!(runtime.select_program(&target).unwrap().key(), module.key());
    let guard = runtime.begin_publication(&target).unwrap();
    let candidate = guard
        .stage_reload_verified_program(None, module.verified_program().clone())
        .unwrap();
    assert!(
        runtime.gc().array_len(array).is_some(),
        "metadata staging must defer collection and host destruction"
    );
    drop(candidate);
    drop(guard);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().array_len(array).is_none());
    assert!(!runtime.is_quarantined());
}

#[derive(Debug)]
struct AdmissionObserver {
    target: Program,
    calls: Arc<AtomicUsize>,
    fail: bool,
}

impl ExecutionObserver for AdmissionObserver {
    fn begin(&mut self, runtime: &Runtime, _: &ExecutableModule) -> Result<(), RuntimeError> {
        rejected(runtime, &self.target);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(RuntimeError::module_validation(
                "observer admission failure",
            ))
        } else {
            Ok(())
        }
    }

    fn observe(
        &mut self,
        runtime: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        rejected(runtime, &self.target);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn publication_is_rejected_inside_observers_and_native_callbacks() {
    let mut runtime = Runtime::default();
    let target = load(&mut runtime, "target");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = ModuleBuilder::new("publication", &Default::default()).unwrap();
    let function = builder
        .define_function(FunctionDecl::new("callback").host_policy(HostCallPolicy {
            passing: vec![],
            effects: HostFunctionEffects {
                may_call_host_services: true,
                may_mutate_host_state: true,
                may_trap: true,
                ..Default::default()
            },
        }))
        .unwrap();
    let callback_target = target.clone();
    let callback_calls = calls.clone();
    let candidate = Mutex::new(Some(stage(&runtime, &target)));
    builder
        .bind(function, move |cx: &HostCallContext<'_>, _: &[Value]| {
            rejected(cx.runtime(), &callback_target);
            let candidate = candidate.lock().unwrap().take().unwrap();
            assert!(matches!(cx.runtime().publish_staged_reload(candidate),
                Err(ReloadValidationError::Runtime(error))
                    if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation));
            callback_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Unit)
        })
        .unwrap();
    let native = builder.finish().unwrap();
    let program = free_function_program(&native.to_declaration());
    native.install(&mut runtime).unwrap();
    let caller = runtime.load_program("callback", program).unwrap();
    let caller = runtime.select_program(&caller).unwrap();
    runtime
        .set_execution_observer(AdmissionObserver {
            target: target.clone(),
            calls: calls.clone(),
            fail: false,
        })
        .unwrap();
    let session = runtime
        .begin_execution(&caller, Default::default())
        .unwrap();
    runtime.attach_execution_observer().unwrap();
    runtime
        .observe_execution(ExecutionEvent::BeforeInstruction)
        .unwrap();
    invoke(&runtime, &caller, 0, &[]).unwrap();
    drop(session);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    drop(runtime.begin_publication(&target).unwrap());

    runtime
        .set_execution_observer(AdmissionObserver {
            target: target.clone(),
            calls,
            fail: true,
        })
        .unwrap();
    let parked = parked_program(&mut runtime);
    let module = runtime.select_program(&parked).unwrap();
    assert!(
        runtime
            .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
            .is_err()
    );
    drop(runtime.begin_publication(&target).unwrap());
    assert!(runtime.execution_root().is_none());
    assert!(!runtime.is_quarantined());
}
