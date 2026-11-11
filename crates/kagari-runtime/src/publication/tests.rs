use crate::test_support::bytecode::load_bytecode;
use crate::test_support::update_programs::load_empty as load;
use crate::{
    Runtime, StagedReload,
    closure::{ClosureTarget, ClosureValueSnapshot},
    error::{RuntimeError, RuntimeErrorKind},
    frame::{ExecutionFrame, ExecutionStack},
    gc::arrays::ArrayPayload,
    host::HostCallContext,
    module::{ExecutableModule, InstalledProgram},
    native::{
        builder::ModuleBuilder, callable::PreparedClosure, declarations::FunctionDecl,
        module::NativeModule,
    },
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

fn stage(runtime: &mut Runtime, target: &InstalledProgram) -> StagedReload {
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

fn rejected(runtime: &Runtime) {
    assert_eq!(
        runtime
            .check_initial_publication()
            .expect_err("publication must reject")
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation,
    );
}

#[test]
fn publication_counts_nested_and_hidden_sessions_not_just_the_visible_root() {
    let mut runtime = Runtime::default();
    let target = load(&mut runtime, "target");
    let module = runtime.select_program(&target).unwrap();
    let candidate = stage(&mut runtime, &target);
    let mut outer = runtime
        .admit_execution(
            &module,
            Default::default(),
            crate::session::ExecutionEntry::Program,
        )
        .unwrap();
    let mut nested = runtime
        .admit_execution(
            &module,
            Default::default(),
            crate::session::ExecutionEntry::Program,
        )
        .unwrap();
    outer.retire(runtime.resources());
    rejected(&runtime);

    // Candidate entry temporarily hides the ordinary session; neither identity
    // nor the visible root is publication authority for any installation.
    let mut candidate_session = runtime.begin_candidate_initialization(&candidate).unwrap();
    rejected(candidate_session.runtime_mut());
    drop(candidate_session);
    rejected(&runtime);

    let hidden = runtime.resources().replace_session(None);
    rejected(&runtime);
    runtime.resources().replace_session(hidden);
    nested.retire(runtime.resources());
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
    let candidate = stage(&mut runtime, &target);
    let guard = runtime.begin_publication(&target).unwrap();
    rejected(guard.runtime());
    rejected(guard.runtime());
    assert_eq!(
        guard
            .runtime
            .begin_execution(&module, Default::default())
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        guard
            .runtime
            .begin_candidate_initialization(&candidate)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(candidate.initialization_error().is_none());
    assert_eq!(
        guard.runtime.close().unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        guard.runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(guard.runtime().select_program(&target).is_ok());
    drop(guard);

    let wrong = stage(&mut runtime, &other);
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

    let stale = stage(&mut runtime, &target);
    let mut guard = runtime.begin_publication(&target).unwrap();
    let next = guard
        .stage_reload_verified_program(
            None,
            guard
                .runtime()
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

fn parked_program(runtime: &mut Runtime) -> InstalledProgram {
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
    load_bytecode(runtime, "parked", program).unwrap()
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
    let mut guard = runtime.begin_publication(&target).unwrap();
    let cx = Context::from_waker(Waker::noop());
    assert_eq!(
        guard
            .runtime
            .resume_owned_execution(&owner)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        guard
            .runtime
            .begin_scheduler_turn(&cx, None)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        guard.runtime.drain_retired_executions().unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    let candidate = guard
        .stage_reload_verified_program(None, module.verified_program().clone())
        .unwrap();
    assert_eq!(
        guard
            .runtime
            .enter_closure_execution_stack(&closure)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    guard.publish(candidate).unwrap();
    let mut closure_stack_activation = runtime.enter_closure_execution_stack(&closure).unwrap();
    closure_stack_activation
        .with(|runtime, _closure_stack| {
            rejected(runtime);

            Ok(())
        })
        .unwrap();
    drop(closure_stack_activation);
    assert!(closure_root.value(runtime.gc()).is_some());
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());

    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            rejected(runtime);
            Ok(())
        })
        .unwrap();
    assert!(ExecutionStack::try_park_ready(&mut activation).unwrap());

    drop(activation);
    drop(runtime.begin_publication(&target).unwrap());

    let mut turn = runtime.begin_scheduler_turn(&cx, None).unwrap();
    assert!(
        turn.next_work().unwrap().is_none(),
        "the turn must not reject itself"
    );
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
        .with_native::<ArrayPayload, _>(array, |_| {
            assert_eq!(
                runtime.check_initial_publication().err().unwrap().kind(),
                RuntimeErrorKind::ModuleValidation
            );
            Ok(())
        })
        .unwrap();
    let mut guard = runtime.begin_publication(&target).unwrap();
    let candidate = guard
        .stage_reload_verified_program(None, module.verified_program().clone())
        .unwrap();
    guard
        .runtime()
        .gc()
        .with_native::<ArrayPayload, _>(array, |_| {
            assert_eq!(
                guard.permit.recheck(guard.runtime()).unwrap_err().kind(),
                RuntimeErrorKind::ModuleValidation
            );
            Ok(())
        })
        .unwrap();
    drop(candidate);
    drop(guard);
    assert_eq!(runtime.select_program(&target).unwrap().key(), module.key());
    let mut guard = runtime.begin_publication(&target).unwrap();
    let candidate = guard
        .stage_reload_verified_program(None, module.verified_program().clone())
        .unwrap();
    assert!(
        guard.runtime().gc().array_len(array).is_some(),
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
    calls: Arc<AtomicUsize>,
    fail: bool,
}

impl ExecutionObserver for AdmissionObserver {
    fn begin(&mut self, runtime: &Runtime, _: &ExecutableModule) -> Result<(), RuntimeError> {
        rejected(runtime);
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
        rejected(runtime);
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
    let callback_calls = calls.clone();
    let candidate = Mutex::new(Some(stage(&mut runtime, &target)));
    builder
        .bind(
            function,
            move |cx: &mut HostCallContext<'_>, _: &[Value]| {
                rejected(cx.runtime());
                assert_eq!(
                    NativeModule::install_all(&[], cx.runtime_mut())
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation,
                );
                drop(candidate.lock().unwrap().take().unwrap());
                callback_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Unit)
            },
        )
        .unwrap();
    let native = builder.finish().unwrap();
    let program = free_function_program(&native.to_declaration());
    native.install(&mut runtime).unwrap();
    let caller = load_bytecode(&mut runtime, "callback", program).unwrap();
    let caller = runtime.select_program(&caller).unwrap();
    runtime
        .set_execution_observer(AdmissionObserver {
            calls: calls.clone(),
            fail: false,
        })
        .unwrap();
    let mut session = runtime
        .begin_execution(&caller, Default::default())
        .unwrap();
    session.runtime().attach_execution_observer().unwrap();
    session
        .runtime()
        .observe_execution(ExecutionEvent::BeforeInstruction)
        .unwrap();
    invoke(session.runtime_mut(), &caller, 0, &[]).unwrap();
    drop(session);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    drop(runtime.begin_publication(&target).unwrap());

    runtime
        .set_execution_observer(AdmissionObserver { calls, fail: true })
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
