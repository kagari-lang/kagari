use kagari_ir::bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};
use kagari_runtime::{ModuleInitializationState, Runtime, RuntimeErrorKind, value::Value};

fn program() -> BytecodeProgram {
    BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    }
}

#[test]
fn dropping_initialization_records_failure_and_releases_version_retention() {
    let mut runtime = Runtime::default();
    let module = runtime.load_program("guard.kgr", program()).unwrap();
    let initialization = runtime.begin_module_initialization(&module).unwrap();
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        1
    );
    assert!(runtime.begin_module_initialization(&module).is_err());
    // Module access remains a short borrow while the lifecycle guard is live.
    assert_eq!(
        runtime.module_instance_snapshot(&module).unwrap().state,
        ModuleInitializationState::Initializing
    );
    drop(initialization);
    assert_eq!(
        runtime.module_instance_snapshot(&module).unwrap().state,
        ModuleInitializationState::Failed
    );
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    assert!(runtime.begin_module_initialization(&module).is_err());
}

#[test]
fn successful_initialization_roots_its_result_and_cannot_be_failed_by_cleanup() {
    let mut runtime = Runtime::default();
    let module = runtime.load_program("guard.kgr", program()).unwrap();
    let initialization = runtime.begin_module_initialization(&module).unwrap();
    let array = runtime.alloc_array(vec![Value::I32(42)]).unwrap();
    assert_eq!(
        initialization.finish(Value::Array(array)).unwrap(),
        Value::Array(array)
    );
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().array_get(array, 0), Some(Value::I32(42)));
    assert!(runtime.fail_module_initialization(&module).is_err());
    assert_eq!(
        runtime.module_instance_snapshot(&module).unwrap().state,
        ModuleInitializationState::Initialized
    );
}

#[test]
fn invalid_initialization_result_fails_without_retaining_a_foreign_heap_value() {
    let mut runtime = Runtime::default();
    let module = runtime.load_program("guard.kgr", program()).unwrap();
    let initialization = runtime.begin_module_initialization(&module).unwrap();
    let foreign = Runtime::default();
    let array = foreign.alloc_array(vec![]).unwrap();
    let error = initialization.finish(Value::Array(array)).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    let instance = runtime.module_instance_snapshot(&module).unwrap();
    assert_eq!(instance.state, ModuleInitializationState::Failed);
    assert!(instance.init_result.is_none());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    assert!(!runtime.is_quarantined());
}
