use kagari_common::cancellation::CancellationToken;
use kagari_ir::bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};
use kagari_runtime::{Runtime, RuntimeErrorKind, value::Value};

fn load(runtime: &mut Runtime, name: &str) -> kagari_runtime::LoadedModule {
    runtime
        .load_program(
            name,
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap()
}

#[test]
fn nested_scopes_inherit_permissions_budget_and_lifetime_even_if_outer_drops_first() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let mut options = runtime.execution_options();
    options.resources.max_instruction_steps = Some(2);
    let outer = runtime.begin_execution(&module, options.clone()).unwrap();
    runtime.consume_instruction_step().unwrap();
    let mut escalation = options.clone();
    escalation.resources.max_instruction_steps = None;
    escalation.security.profile.allow_host_calls = true;
    escalation.security.capabilities.host_calls = true;
    let nested = runtime.begin_execution(&module, escalation).unwrap();
    assert!(!runtime.security().allows_host_calls());
    runtime.consume_instruction_step().unwrap();
    let error = runtime.consume_instruction_step().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    assert_eq!(nested.counters().instruction_steps, 2);
    drop(outer);
    assert!(runtime.consume_instruction_step().is_err());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        1
    );
    drop(nested);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    assert!(runtime.resources().termination().is_none());
    let next = runtime.begin_execution(&module, options).unwrap();
    runtime.consume_instruction_step().unwrap();
    runtime.consume_instruction_step().unwrap();
    assert_eq!(next.counters().instruction_steps, 2);
    assert_eq!(runtime.resources().counters().instruction_steps, 4);
}

#[test]
fn root_permissions_and_host_policy_are_fixed_until_last_scope_exits() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    let mut security = runtime.security();
    security.profile.allow_host_calls = true;
    security.capabilities.host_calls = true;
    runtime.set_security_context(security);
    let mut exposure = (*runtime.host_exposure()).clone();
    exposure.allowed_host_functions.push("new.host".into());
    runtime.set_host_exposure_policy(exposure);
    assert!(!runtime.security().allows_host_calls());
    assert!(!runtime.host_exposure().exposes_host_function("new.host"));
    drop(session);
    assert!(runtime.security().allows_host_calls());
    assert!(runtime.host_exposure().exposes_host_function("new.host"));
}

#[test]
fn nested_execution_rejects_an_unpinned_program_or_foreign_runtime() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let other = load(&mut runtime, "other");
    let mut foreign = Runtime::default();
    let foreign_module = load(&mut foreign, "main");
    let session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    assert!(
        runtime
            .begin_execution(&other, runtime.execution_options())
            .is_err()
    );
    assert!(
        runtime
            .begin_execution(&foreign_module, runtime.execution_options())
            .is_err()
    );
    assert_eq!(session.root().key(), module.key());
    assert_eq!(
        runtime.modules().retention_counts(other.key()).active_calls,
        0
    );
}

#[test]
fn cancellation_is_sticky_until_all_scopes_exit_and_next_root_can_run() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let token = CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let session = runtime.begin_execution(&module, options.clone()).unwrap();
    let object = runtime.alloc_array(vec![Value::I32(42)]).unwrap();
    token.cancel();
    assert_eq!(
        runtime.gc_safepoint().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        runtime.alloc_array(vec![]).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert!(
        runtime
            .begin_execution(&module, runtime.execution_options())
            .is_err()
    );
    assert!(!runtime.is_quarantined());
    drop(session);
    assert!(runtime.resources().termination().is_none());
    assert!(runtime.begin_execution(&module, options).is_err());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    let next = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().array_len(object).is_none());
    runtime.consume_instruction_step().unwrap();
    assert_eq!(next.counters().instruction_steps, 1);
}

#[test]
fn each_root_gets_an_allocation_budget_while_live_heap_and_cumulative_counts_persist() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let mut options = runtime.execution_options();
    options.resources.max_allocation_units = Some(2);
    let mut roots = Vec::new();
    for index in 1..=2 {
        let session = runtime.begin_execution(&module, options.clone()).unwrap();
        let array = runtime.alloc_array(vec![Value::I32(index)]).unwrap();
        roots.push(runtime.root_value(Value::Array(array)).unwrap());
        assert_eq!(session.counters().allocation_units, 2);
        assert!(runtime.alloc_array(vec![]).is_err());
        drop(session);
    }
    assert_eq!(runtime.resources().counters().allocation_units, 4);
    assert_eq!(runtime.resources().counters().current_heap_units, 4);
    drop(roots);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 0);
    assert_eq!(runtime.resources().counters().allocation_units, 4);
}

#[test]
fn root_heap_peak_counters_do_not_reuse_a_previous_roots_peak() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    for depth in [2, 1] {
        let session = runtime
            .begin_execution(&module, runtime.execution_options())
            .unwrap();
        let array = runtime
            .alloc_array(vec![Value::Unit; depth as usize])
            .unwrap();
        assert_eq!(session.counters().peak_heap_units, depth as usize + 1);
        runtime.collect_garbage().unwrap();
        assert!(runtime.gc().array_len(array).is_none());
        drop(session);
    }
    assert_eq!(runtime.resources().counters().peak_heap_units, 3);
}

#[test]
fn zero_wall_budget_rejects_before_initialization_or_counter_charges() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let mut options = runtime.execution_options();
    options.resources.max_wall_time_ms = Some(0);
    let before = runtime.resources().counters();
    let error = runtime.begin_execution(&module, options).err().unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    assert!(error.message().contains("wall time"));
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    assert_eq!(
        runtime.module_instance_snapshot(&module).unwrap().state,
        kagari_runtime::ModuleInitializationState::Uninitialized
    );
}
