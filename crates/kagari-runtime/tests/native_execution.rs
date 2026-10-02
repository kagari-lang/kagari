use std::{
    ffi::c_void,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use kagari_abi::{
    ids::FunctionRef,
    native::{
        BackendId, BackendTarget, ExecutableEntryPoint, ExecutableFunctionArtifact,
        ExecutableSafepoint, ExecutableSafepointKind, ExecutableStackMap, NativeCodeOwner,
        NativeCompilationProduct,
    },
    native_call::{JIT_STATUS_INTEGER_OVERFLOW, JIT_STATUS_OK, JitCompiledFunction, JitValue},
    representation::ValueType,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::BytecodeInstruction,
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    backend::{BackendInvocationError, native::InstalledNativeFunction},
    error::RuntimeErrorKind,
    jit_abi::jit_poll_execution,
    module::VerifiedProgram,
    reload::ReloadValidationError,
    resource::RuntimeLimits,
    value::Value,
};

#[derive(Debug)]
struct Owner(Arc<AtomicUsize>);
impl NativeCodeOwner for Owner {}
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn runtime(limit: Option<u64>) -> Runtime {
    Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            max_instruction_steps: limit,
            ..Default::default()
        },
        ..Default::default()
    })
}
fn program() -> BytecodeProgram {
    let function = BytecodeFunction {
        name: "main".into(),
        metadata: FunctionMetadata {
            ..Default::default()
        },
        instructions: vec![
            BytecodeInstruction::BudgetCheckpoint,
            BytecodeInstruction::Return(None),
        ],
        ..Default::default()
    };
    BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            types: vec![ValueType::Unit],
            function_table: vec![FunctionRecord {
                id: function.id,
                identity: None,
                name: function.name.clone(),
                params: vec![],
                return_type: ValueType::Unit,
                effects: Default::default(),
            }],
            functions: vec![function],
            ..Default::default()
        }],
    }
}
unsafe extern "C" fn execute(runtime: *const c_void, result: *mut JitValue) -> i32 {
    for offset in 0..2 {
        let status = unsafe { jit_poll_execution(runtime.cast(), offset) };
        if status != JIT_STATUS_OK {
            return status;
        }
    }
    unsafe {
        result.write(JitValue::unit());
    }
    JIT_STATUS_OK
}
unsafe extern "C" fn trap(runtime: *const c_void, _: *mut JitValue) -> i32 {
    let status = unsafe { jit_poll_execution(runtime.cast(), 0) };
    if status == JIT_STATUS_OK {
        JIT_STATUS_INTEGER_OVERFLOW
    } else {
        status
    }
}
unsafe extern "C" fn bad_result(runtime: *const c_void, result: *mut JitValue) -> i32 {
    let status = unsafe { execute(runtime, result) };
    if status == JIT_STATUS_OK {
        unsafe {
            result.write(JitValue::i32(42));
        }
    }
    status
}
fn product(entry: JitCompiledFunction, dropped: Arc<AtomicUsize>) -> Rc<NativeCompilationProduct> {
    let mut artifact = ExecutableFunctionArtifact::new(
        BackendId::new("fixture"),
        BackendTarget::new("host-fixture", usize::BITS as u8),
        FunctionRef::new(0),
    );
    artifact.entry = ExecutableEntryPoint::Native {
        symbol: "fixture".into(),
        address: entry as usize,
    };
    Rc::new(NativeCompilationProduct {
        artifact,
        owner: Rc::new(Owner(dropped)),
    })
}
fn install(runtime: &mut Runtime, entry: JitCompiledFunction) -> InstalledNativeFunction {
    let module = runtime.load_program("native", program()).unwrap();
    // Static fixtures implement the current ABI; fault probes intentionally test
    // malformed status/results without violating Rust memory safety.
    unsafe { runtime.install_native_function(&module, product(entry, Arc::default())) }.unwrap()
}
fn assert_clean(runtime: &Runtime) {
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(runtime.execution_root().is_none());
}

#[test]
fn native_calls_use_frames_and_exact_budgets_and_unwind_on_failure() {
    for limit in [None, Some(1)] {
        let mut runtime = runtime(limit);
        let installed = install(&mut runtime, execute);
        let result = runtime.invoke_native_function(&installed);
        if limit.is_none() {
            assert_eq!(result.unwrap(), Value::Unit);
            assert_eq!(runtime.resources().counters().instruction_steps, 2);
        } else {
            let failure = result.unwrap_err();
            assert!(
                matches!(failure.error, BackendInvocationError::RuntimeFailure(ref error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
            );
            assert_eq!(failure.trace.frames[0].instruction_offset, 1);
            assert_eq!(runtime.resources().counters().instruction_steps, 1);
        }
        assert_clean(&runtime);
    }
}

#[test]
fn native_traps_capture_the_frame_before_cleanup_and_never_request_fallback() {
    let mut runtime = runtime(None);
    let installed = install(&mut runtime, trap);
    let failure = runtime.invoke_native_function(&installed).unwrap_err();
    assert!(
        matches!(failure.error, BackendInvocationError::RuntimeFailure(ref error) if error.kind() == RuntimeErrorKind::ScriptTrap)
    );
    assert_eq!(failure.trace.frames[0].function_name, "main");
    assert_eq!(failure.trace.frames[0].instruction_offset, 0);
    assert_eq!(runtime.resources().counters().instruction_steps, 1);
    assert_clean(&runtime);
}

#[test]
fn installed_handles_retain_code_and_old_versions_until_the_last_clone_drops() {
    let mut runtime = runtime(None);
    let module = runtime.load_program("native", program()).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let code = product(execute, dropped.clone());
    let installed = unsafe { runtime.install_native_function(&module, code.clone()) }.unwrap();
    let clone = installed.clone();
    drop(code);
    let candidate = runtime
        .stage_reload_program(&module, "native", program())
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    assert_ne!(new.key(), module.key());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        1
    );
    runtime.modules().collect_unreachable_epochs();
    assert_eq!(
        runtime.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    drop(installed);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    drop(clone);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        0
    );
    runtime.modules().collect_unreachable_epochs();
    assert!(runtime.modules().loaded(module.key()).is_none());
    assert!(runtime.modules().loaded(new.key()).is_some());
}

#[test]
fn invocation_checks_runtime_ownership_without_permission_flags() {
    let mut first = runtime(None);
    let installed = install(&mut first, execute);
    let second = runtime(None);
    assert!(
        matches!(second.invoke_native_function(&installed).unwrap_err().error, BackendInvocationError::RuntimeFailure(ref error) if error.kind() == RuntimeErrorKind::ModuleValidation)
    );
    assert_eq!(
        first.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    assert_clean(&first);
    assert_clean(&second);
}

#[test]
fn malformed_native_results_quarantine_and_clean_up() {
    let mut runtime = runtime(None);
    let installed = install(&mut runtime, bad_result);
    let failure = runtime.invoke_native_function(&installed).unwrap_err();
    assert!(
        matches!(failure.error, BackendInvocationError::RuntimeFailure(ref error) if error.kind() == RuntimeErrorKind::EngineFault)
    );
    assert!(runtime.is_quarantined());
    assert_clean(&runtime);
}

#[test]
fn unresolved_entries_are_rejected_before_retaining_versions_or_running_code() {
    let mut runtime = runtime(None);
    let module = runtime.load_program("native", program()).unwrap();
    let mut code = product(execute, Arc::default());
    Rc::get_mut(&mut code).unwrap().artifact.entry = ExecutableEntryPoint::Unresolved;
    assert!(matches!(
        unsafe { runtime.install_native_function(&module, code) },
        Err(BackendInvocationError::UnsupportedArtifact(_))
    ));
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        0
    );
    assert_eq!(runtime.resources().counters().instruction_steps, 0);
}

#[test]
fn installation_retains_descriptors_and_rejects_unknown_functions_without_leaking_owners() {
    let mut runtime = runtime(None);
    let module = runtime.load_program("native", program()).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut code = product(execute, dropped.clone());
    Rc::get_mut(&mut code)
        .unwrap()
        .artifact
        .safepoints
        .push(ExecutableSafepoint {
            instruction_offset: 0,
            kind: ExecutableSafepointKind::RuntimeHelperCall {
                helper: kagari_abi::native_call::JIT_POLL_EXECUTION_SYMBOL.into(),
            },
            stack_map: ExecutableStackMap::empty(),
        });
    let expected = code.artifact.clone();
    // The static unit-returning fixture implements this program's native ABI.
    let installed = unsafe { runtime.install_native_function(&module, code) }.unwrap();
    assert_eq!(installed.module().key(), module.key());
    assert_eq!(installed.artifact(), &expected);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        1
    );

    let mut invalid = product(execute, dropped.clone());
    Rc::get_mut(&mut invalid).unwrap().artifact.function = FunctionRef::new(99);
    // Invalid metadata is rejected before the otherwise valid static entry can run.
    assert!(matches!(
        unsafe { runtime.install_native_function(&module, invalid) },
        Err(BackendInvocationError::UnsupportedArtifact(_))
    ));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        1
    );
    assert_eq!(runtime.resources().counters().instruction_steps, 0);
    assert_eq!(
        runtime.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    drop(installed);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        0
    );
    assert_clean(&runtime);
}

#[test]
fn failed_reload_keeps_installed_code_callable_and_its_version_retained() {
    let mut runtime = runtime(None);
    let module = runtime.load_program("native", program()).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let installed =
        unsafe { runtime.install_native_function(&module, product(execute, dropped.clone())) }
            .unwrap();
    let mut candidate = KbcArtifact::from_program(program(), Default::default()).unwrap();
    candidate.header.content_hash.0 ^= 1;
    assert!(matches!(
        runtime.stage_reload_artifact(&module, "native", candidate, &Default::default()),
        Err(ReloadValidationError::Artifact(_))
    ));
    assert_eq!(
        runtime.modules().latest("native").unwrap().key(),
        module.key()
    );
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        1
    );
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        runtime.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    drop(installed);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .compiled_artifacts,
        0
    );
    assert_clean(&runtime);
}

#[test]
fn native_reentry_preserves_the_callers_frame_and_shared_budget() {
    let mut runtime = runtime(None);
    let installed = install(&mut runtime, execute);
    let outer = runtime.enter_execution_stack(installed.module()).unwrap();
    outer
        .push(installed.module().slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    assert_eq!(
        runtime.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    assert_eq!(outer.frames().unwrap().len(), 1);
    assert_eq!(runtime.resources().counters().current_call_depth, 1);
    assert_eq!(runtime.resources().counters().instruction_steps, 2);
    drop(outer);
    assert_clean(&runtime);
}

#[test]
fn cancellation_of_an_active_session_prevents_native_progress() {
    let mut runtime = runtime(None);
    let installed = install(&mut runtime, execute);
    let options = runtime.execution_options();
    let cancel = options.cancellation.clone();
    let session = runtime
        .begin_execution(installed.module(), options)
        .unwrap();
    cancel.cancel();
    let failure = runtime.invoke_native_function(&installed).unwrap_err();
    assert!(
        matches!(failure.error, BackendInvocationError::RuntimeFailure(ref error) if error.kind() == RuntimeErrorKind::Cancelled)
    );
    assert_eq!(runtime.resources().counters().instruction_steps, 0);
    drop(session);
    assert_clean(&runtime);
}

#[test]
fn executable_memory_can_be_shared_without_sharing_runtime_instances() {
    let mut first = runtime(None);
    let mut second = runtime(None);
    let a = first.load_program("native", program()).unwrap();
    let b = second.load_program("native", program()).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let code = product(execute, dropped.clone());
    let x = unsafe { first.install_native_function(&a, code.clone()) }.unwrap();
    let y = unsafe { second.install_native_function(&b, code.clone()) }.unwrap();
    drop(code);
    assert_eq!(first.invoke_native_function(&x).unwrap(), Value::Unit);
    assert_eq!(second.resources().counters().instruction_steps, 0);
    drop(x);
    drop(first);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(second.invoke_native_function(&y).unwrap(), Value::Unit);
    drop(y);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn incompatible_native_abis_are_rejected_before_installation() {
    for helper in [false, true] {
        let mut runtime = runtime(None);
        let module = runtime.load_program("native", program()).unwrap();
        let mut code = product(execute, Arc::default());
        let artifact = &mut Rc::get_mut(&mut code).unwrap().artifact;
        if helper {
            artifact.runtime_helper_abi_version = "previous-helper".into();
        } else {
            artifact.runtime_abi_version = "previous-runtime".into();
        }
        assert!(matches!(
            unsafe { runtime.install_native_function(&module, code) },
            Err(BackendInvocationError::UnsupportedArtifact(_))
        ));
        assert_eq!(
            runtime
                .modules()
                .retention_counts(module.key())
                .compiled_artifacts,
            0
        );
    }
}

#[test]
fn native_handles_pin_the_entire_dependency_program_across_reload() {
    use kagari_common::identity::ModuleIdentity;
    let mut graph = program();
    graph.modules[0].dependencies.push(ModuleRef::new(0));
    graph.modules.insert(
        0,
        BytecodeModule {
            identity: ModuleIdentity::single_file("dependency"),
            ..Default::default()
        },
    );
    graph.root = ModuleRef::new(1);
    let mut runtime = runtime(None);
    let module = runtime.load_program("native", graph.clone()).unwrap();
    let old_dependency = module.member(ModuleRef::new(0)).unwrap();
    let installed =
        unsafe { runtime.install_native_function(&module, product(execute, Arc::default())) }
            .unwrap();
    let candidate = runtime
        .stage_reload_program(&module, "native", graph)
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    runtime.modules().collect_unreachable_epochs();
    assert_eq!(
        runtime
            .modules()
            .retention_counts(old_dependency.key())
            .compiled_artifacts,
        1
    );
    assert!(runtime.modules().loaded(old_dependency.key()).is_some());
    assert_ne!(
        old_dependency.key(),
        new.member(ModuleRef::new(0)).unwrap().key()
    );
    assert_eq!(
        runtime.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    drop(installed);
    runtime.modules().collect_unreachable_epochs();
    assert!(runtime.modules().loaded(old_dependency.key()).is_none());
}

#[test]
fn execution_observers_prevent_native_entry_without_debug_callbacks() {
    use kagari_runtime::{
        error::RuntimeError,
        frame::ExecutionFrame,
        session::{ExecutionEvent, ExecutionObserver},
    };
    #[derive(Debug)]
    struct Observer;
    impl ExecutionObserver for Observer {
        fn observe(
            &self,
            _: &Runtime,
            _: ExecutionEvent,
            _: &[ExecutionFrame],
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
    }
    let mut runtime = runtime(None);
    let installed = install(&mut runtime, execute);
    let session = runtime
        .begin_execution(installed.module(), runtime.execution_options())
        .unwrap();
    runtime
        .attach_execution_observer(Rc::new(Observer))
        .unwrap();
    assert!(matches!(
        runtime
            .invoke_native_function(&installed)
            .unwrap_err()
            .error,
        BackendInvocationError::UnsupportedArtifact(_)
    ));
    assert_eq!(runtime.resources().counters().instruction_steps, 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    drop(session);
    assert_clean(&runtime);
}

#[test]
fn shared_verified_program_identity_survives_independent_runtime_linking() {
    let verified = VerifiedProgram::new(program()).unwrap();
    let separate = VerifiedProgram::new(program()).unwrap();
    let mut first = runtime(None);
    let mut second = runtime(None);
    let a = first
        .load_verified_program("first", verified.clone())
        .unwrap();
    let b = second
        .load_verified_program("second", verified.clone())
        .unwrap();
    let c = second
        .load_verified_program("separate", separate.clone())
        .unwrap();
    assert!(a.verified_program().same_version(&verified));
    assert!(a.verified_program().same_version(b.verified_program()));
    assert!(Arc::ptr_eq(&a.bytecode, &b.bytecode));
    assert_eq!(a.program_fingerprint(), c.program_fingerprint());
    assert!(!a.verified_program().same_version(c.verified_program()));
    assert!(first.validate_loaded_module(&b).is_err());
    assert!(second.validate_loaded_module(&a).is_err());
    drop(verified);
    assert!(a.verified_program().same_version(b.verified_program()));
}
