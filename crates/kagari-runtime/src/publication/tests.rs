use crate::test_support::bytecode::load_bytecode;
use crate::test_support::update_programs::load_empty as load;
use crate::{
    Runtime, StagedReload,
    closure::ClosureMode,
    error::{RuntimeError, RuntimeErrorKind},
    execution::roots::ExecutionRoots,
    frame::{ExecutionFrame, ExecutionStack},
    host::HostCallContext,
    module::{ExecutableModule, InstalledProgram, managed::source::RevisionSource},
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host::callback, module::NativeModule,
    },
    session::{ExecutionEntry, ExecutionEvent, ExecutionObserver},
    test_support::native::{
        invoke::invoke,
        program::{ProgramFixture, free_function_program},
    },
    value::view::ValueView,
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
use kagari_gc::{
    error::GcError,
    object::GcObject,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::declaration::host::{HostCallPolicy, HostFunctionEffects};
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
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    let mut nested = runtime
        .admit_execution(
            &module,
            Default::default(),
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    outer.retire(&mut runtime);
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
    nested.retire(&mut runtime);
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
    let closure_root = {
        let mut cx = ConversionContext::new(&mut runtime, &module).unwrap();
        let value = cx
            .allocate_script_closure(FunctionRef::new(0), &[], None, ClosureMode::Retained)
            .unwrap();
        cx.retain(&value).unwrap()
    };
    // This source retains an already selected computation without a synchronous
    // activation. Like parked frame storage, it does not bar publication itself.
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let location = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = closure_root.get(slots.scope())?;
                slots.push_local(execution, value)
            })
        })
        .unwrap();
    let value = runtime.memory.inspect_value(location).unwrap();
    let closure = runtime.select_closure_call(execution, &value).unwrap();
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
    assert_eq!(
        runtime
            .resolve_closure(&value)
            .unwrap()
            .implementation
            .key(),
        module.key(),
    );
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
    drop(closure);
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    drop(closure_root);
    runtime.close().unwrap();
}

struct DisposalProbe(Arc<AtomicUsize>);

impl Drop for DisposalProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

// SAFETY: the owned diagnostic Arc has no managed edge or separate backing
// charge. Its destructor increments only an inert external counter.
unsafe impl GcObject for DisposalProbe {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }
}

#[test]
fn publication_rejects_foreign_targets_and_staging_defers_disposal() {
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
    let drops = Arc::new(AtomicUsize::new(0));
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            // SAFETY: this edge-free provider owns only its inert counter. No handle
            // escapes the scope; the unrooted object stays eligible for collection.
            unsafe {
                cx.alloc_with(|_| Ok(DisposalProbe(drops.clone())))?;
            }
            Ok(())
        })
        .unwrap();
    let mut guard = runtime.begin_publication(&target).unwrap();
    let candidate = guard
        .stage_reload_verified_program(None, module.verified_program().clone())
        .unwrap();
    guard.permit.recheck(guard.runtime()).unwrap();
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "metadata staging must defer collection and host destruction"
    );
    drop(candidate);
    drop(guard);
    assert_eq!(runtime.select_program(&target).unwrap().key(), module.key());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
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
            callback(move |cx: &mut HostCallContext<'_>, _| {
                rejected(cx.runtime());
                assert_eq!(
                    NativeModule::install_all(&[], cx.runtime_mut())
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation,
                );
                drop(candidate.lock().unwrap().take().unwrap());
                callback_calls.fetch_add(1, Ordering::SeqCst);
                Ok(ValueView::unit())
            }),
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
        .runtime_mut()
        .observe_execution(ExecutionEvent::BeforeInstruction)
        .unwrap();
    let _ = invoke(session.runtime_mut(), &caller, 0, &[]).unwrap();
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
