#![cfg(feature = "source")]
use crate::test_support::bytecode::{load_bytecode, reload_bytecode};
use crate::{
    BytecodeArtifact,
    engine::{
        KagariEngine,
        source::{ArtifactOptions, compilation::Compilation as LoweredCompilation},
    },
    program::Compilation,
    runtime::Runtime,
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::{
    Runtime as CoreRuntime, RuntimeConfig,
    module::{ExecutableModule, InstalledProgram},
    native::module::NativeModule,
    value::carriers::RootedValue,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_types::declaration::module::ModuleDecl;
use rstest::rstest;
use std::slice;

pub(super) fn prepare_static_program(
    engine: &KagariEngine,
    source: &str,
    decoded: bool,
) -> Compilation {
    let checked = engine
        .compile_source(SourceFile::new("static-values.kgr", source))
        .unwrap();
    if !decoded {
        return engine.prepare(&checked, Default::default()).unwrap();
    }
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    Compilation::from_artifact(
        BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn static_call(runtime: &mut Runtime, module: &InstalledProgram, entry: &str) -> RootedValue {
    runtime
        .execute(module, entry, &[], &Default::default())
        .unwrap()
        .return_value
}

#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_char_preserves_state_and_rejects_integer_retyping(#[case] decoded: bool) {
    let source = r#"
static var STATE: char = 'a';
pub fn read() -> u32 { u32::from(STATE) }
pub fn write() { STATE = '🙂'; }
"#;
    let retyped = r#"
static var STATE: u32 = 97u32;
pub fn read() -> u32 { u32::from(STATE) }
pub fn write() { STATE = 128578u32; }
"#;
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let first = prepare_static_program(&engine, source, decoded);
    let changed_initializer =
        prepare_static_program(&engine, &source.replace("'a'", "'z'"), decoded);
    let second = prepare_static_program(
        &engine,
        &source.replace("u32::from(STATE)", "u32::from(STATE) + 0u32"),
        decoded,
    );
    let incompatible = prepare_static_program(&engine, retyped, decoded);
    let mut runtime = engine.runtime(Default::default());
    let old = runtime
        .load_program(first.program(), Default::default())
        .unwrap();
    assert_eq!(
        static_call(&mut runtime, &old, "read").as_i64().unwrap(),
        97
    );
    let _ = static_call(&mut runtime, &old, "write");
    assert_eq!(
        runtime
            .program(&old)
            .map_err(ReloadValidationError::Runtime)
            .and_then(
                |base| base.prepare_replacement(changed_initializer.program(), &Default::default())
            )
            .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
            .unwrap_err()
            .code(),
        "KG_RELOAD_UPDATE_EVIDENCE"
    );
    runtime
        .program(&old)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(second.program(), &Default::default()))
        .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
        .unwrap();
    let current = old.clone();
    assert!(
        runtime
            .program(&current)
            .map_err(ReloadValidationError::Runtime)
            .and_then(|base| base.prepare_replacement(incompatible.program(), &Default::default()))
            .and_then(|candidate| runtime.replace_program(&current, &candidate, Default::default()))
            .is_err()
    );
    for module in [&old, &current] {
        assert_eq!(
            static_call(&mut runtime, module, "read").as_i64().unwrap(),
            128578
        );
    }
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_objects_and_recursive_nominal_collections_preserve_aliases(
    #[case] decoded: bool,
) {
    let source = include_str!(
        "../../../../tests/fixtures/test-fixtures/embedding/persistent-static-objects.kgr"
    );
    assert_eq!(source.matches("value: 1,").count(), 1);
    assert_eq!(source.matches("value: 90,").count(), 1);
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let first = prepare_static_program(&engine, source, decoded);
    let changed_initializer =
        prepare_static_program(&engine, &source.replace("value: 1,", "value: 2,"), decoded);
    let second = prepare_static_program(
        &engine,
        &source.replace("value: 90,", "value: 91,"),
        decoded,
    );
    let mut runtime = engine.runtime(Default::default());
    let old = runtime
        .load_program(first.program(), Default::default())
        .unwrap();
    assert!(static_call(&mut runtime, &old, "check").as_bool().unwrap());
    assert_eq!(
        runtime
            .program(&old)
            .map_err(ReloadValidationError::Runtime)
            .and_then(
                |base| base.prepare_replacement(changed_initializer.program(), &Default::default())
            )
            .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
            .unwrap_err()
            .code(),
        "KG_RELOAD_UPDATE_EVIDENCE"
    );
    runtime
        .program(&old)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(second.program(), &Default::default()))
        .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
        .unwrap();
    let current = old.clone();
    let _ = static_call(&mut runtime, &current, "replace");
    for module in [&old, &current] {
        assert!(
            static_call(&mut runtime, module, "check")
                .as_bool()
                .unwrap()
        );
        assert!(
            static_call(&mut runtime, module, "distinct")
                .as_bool()
                .unwrap()
        );
        assert_eq!(
            static_call(&mut runtime, module, "read").as_i32().unwrap(),
            791
        );
    }
}
*/

/* stdlib test support paused (NC01-NC06).
const STATIC_SOURCE: &str =
    include_str!("../../../../tests/fixtures/embedding/reload/static-values-v1.kgr"); */

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_callbacks_and_interfaces_call_current_helpers(#[case] decoded: bool) {
    let source = STATIC_SOURCE;
    // Initializer expressions stay fixed; retained values call current helpers.
    // Explicit assignment still replaces cells without changing their saved aliases.
    let replacement =
        include_str!("../../../../tests/fixtures/embedding/reload/static-callbacks-v2.kgr");
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let first = prepare_static_program(&engine, source, decoded);
    let second = prepare_static_program(&engine, replacement, decoded);
    let mut runtime = engine.runtime(Default::default());
    let old = runtime
        .load_program(first.program(), Default::default())
        .unwrap();
    assert!(static_call(&mut runtime, &old, "init").as_bool().unwrap());
    // Force the aliases before replacing their original cells.
    assert_eq!(
        static_call(&mut runtime, &old, "saved").as_i32().unwrap(),
        22
    );
    assert_eq!(
        static_call(&mut runtime, &old, "function")
            .as_i32()
            .unwrap(),
        10
    );
    runtime
        .program(&old)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(second.program(), &Default::default()))
        .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
        .unwrap();
    let current = old.clone();
    let _ = static_call(&mut runtime, &current, "set_state");

    assert!(
        static_call(&mut runtime, &current, "fresh_lookup")
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        static_call(&mut runtime, &current, "current")
            .as_i32()
            .unwrap(),
        50
    );
    assert_eq!(
        static_call(&mut runtime, &current, "function")
            .as_i32()
            .unwrap(),
        20
    );
    let _ = static_call(&mut runtime, &current, "replace");
    assert_eq!(
        static_call(&mut runtime, &current, "current")
            .as_i32()
            .unwrap(),
        70
    );
    assert_eq!(
        static_call(&mut runtime, &current, "saved")
            .as_i32()
            .unwrap(),
        50
    );
    assert_eq!(
        static_call(&mut runtime, &old, "current").as_i32().unwrap(),
        70
    );
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_hash_comparators_use_current_bodies_and_helpers(#[case] decoded: bool) {
    let source = STATIC_SOURCE;
    let bodies =
        include_str!("../../../../tests/fixtures/embedding/reload/static-hash-bodies-v2.kgr");
    let helpers =
        include_str!("../../../../tests/fixtures/embedding/reload/static-hash-helpers-v3.kgr");
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let first = prepare_static_program(&engine, source, decoded);
    let mut runtime = engine.runtime(Default::default());
    let old = runtime
        .load_program(first.program(), Default::default())
        .unwrap();
    for entry in ["map_lookup", "set_lookup"] {
        assert!(static_call(&mut runtime, &old, entry).as_bool().unwrap());
    }
    assert_eq!(
        static_call(&mut runtime, &old, "compare_contents")
            .as_i32()
            .unwrap(),
        15
    );
    for (updated, expected) in [(bodies, 2323), (helpers, 2424)] {
        let candidate = prepare_static_program(&engine, updated, decoded);
        runtime
            .program(&old)
            .map_err(ReloadValidationError::Runtime)
            .and_then(|base| base.prepare_replacement(candidate.program(), &Default::default()))
            .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
            .unwrap();
        for entry in ["map_lookup", "set_lookup"] {
            let _ = static_call(&mut runtime, &old, "reset_markers");
            assert!(static_call(&mut runtime, &old, entry).as_bool().unwrap());
            assert_eq!(
                static_call(&mut runtime, &old, "markers").as_i32().unwrap(),
                expected
            );
        }
    }
    let current = old.clone();
    assert_eq!(
        static_call(&mut runtime, &current, "compare_contents")
            .as_i32()
            .unwrap(),
        15
    );
    assert!(
        static_call(&mut runtime, &current, "content_hashes")
            .as_bool()
            .unwrap()
    );
    assert!(
        static_call(&mut runtime, &current, "fresh_lookup")
            .as_bool()
            .unwrap()
    );
    for entry in ["map_lookup", "set_lookup", "mutate_keys"] {
        assert!(
            static_call(&mut runtime, &current, entry)
                .as_bool()
                .unwrap()
        );
    }
    let changed_hash =
        include_str!("../../../../tests/fixtures/embedding/reload/static-hash-change-v4.kgr");
    let candidate = prepare_static_program(&engine, changed_hash, decoded);
    runtime
        .program(&old)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(candidate.program(), &Default::default()))
        .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
        .unwrap();
    for entry in ["map_lookup", "set_lookup"] {
        let _ = static_call(&mut runtime, &old, "reset_markers");
        // Changed hashes need not find old entries. Only current dispatch is asserted.
        let _ = static_call(&mut runtime, &old, entry);
        assert_eq!(
            static_call(&mut runtime, &old, "hash_observed")
                .as_i32()
                .unwrap(),
            34
        );
    }
}
*/

fn prepare(engine: &KagariEngine, source: &str) -> DefinitionMetadata<BytecodeProgram> {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("generic-reload.kgr", source),
            Default::default(),
        )
        .unwrap();
    decode_program(artifact)
}

fn prepare_with_demands(
    engine: &KagariEngine,
    source: &str,
    baseline: &ExecutableModule,
) -> DefinitionMetadata<BytecodeProgram> {
    let checked = engine
        .compile_source(SourceFile::new("generic-reload.kgr", source))
        .unwrap();
    let options = ArtifactOptions::default();
    let demands = baseline.retained_demands();
    let compilation = LoweredCompilation::lower_with_demands(
        &checked,
        &options.lowering,
        demands.metadata(),
        demands.implementation_headers(),
    )
    .unwrap();
    decode_program(compilation.export(options).unwrap())
}

fn decode_program(
    artifact: DefinitionMetadata<BytecodeArtifact>,
) -> DefinitionMetadata<BytecodeProgram> {
    let (definitions, artifact) =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap()
            .into_parts();
    DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap()
}

/// Compare released executions/values against a cold graph of the current product.
/// A new revision may contain a different amount of prepared managed metadata.
fn assert_cold_reclamation(
    runtime: &mut CoreRuntime,
    installed: &InstalledProgram,
    current: DefinitionMetadata<BytecodeProgram>,
) {
    let mut cold = CoreRuntime::default();
    NativeModule::install_all(&stdlib::modules().unwrap(), &mut cold).unwrap();
    let _cold = load_bytecode(&mut cold, "cold-reload-reference", current.clone()).unwrap();
    cold.collect_garbage().unwrap();
    let baseline = cold.gc_stats();
    reload_bytecode(runtime, installed, None, current).unwrap();
    runtime.collect_garbage().unwrap();
    let released = runtime.gc_stats();
    assert_eq!(released.objects, baseline.objects);
    assert_eq!(released.provider_units, baseline.provider_units);
    assert_eq!(released.root_registrations, baseline.root_registrations);
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn retained_local_function_values_call_current_bodies_and_clean_up_all_exits() {
    let source = include_str!("../../../../tests/fixtures/embedding/reload/local-functions-v1.kgr");
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        include_str!("../../../../tests/fixtures/embedding/reload/local-functions-v2.kgr"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = load_bytecode(&mut runtime, "local-reload", program).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let run_path = old.definitions().resolve(run).unwrap().to_path();
    let old_key = old.key();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let base_roots = vm.gc_stats().root_registrations;
    let saved = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("local-reload".into()),
        replacement.clone(),
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    let current_run = current.definitions().lookup(&run_path).unwrap();
    drop(old);
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(old_key).is_some());
    let fresh = vm
        .execute_module(&current, "make", vm.execution_options())
        .unwrap()
        .return_value;
    for (value, method, expected) in [(&saved, &run, 43), (&fresh, &current_run, 43)] {
        assert_eq!(
            vm.invoke_interface_method(value, method, &[RootedValue::from(0_i32)])
                .unwrap()
                .as_i32()
                .unwrap(),
            expected
        );
    }
    for (mode, expected) in [(1, "division by zero"), (2, "call depth")] {
        let error = vm
            .invoke_interface_method(&saved, &run, &[RootedValue::from(mode)])
            .unwrap_err();
        assert!(format!("{error:?}").contains(expected), "{error:?}");
        assert_eq!(vm.resources().counters().current_call_depth, 0);
        assert!(vm.execution_root().is_none());
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().root_registrations, base_roots + 2);
        assert!(!vm.is_quarantined());
    }
    let mut session = support::cancel_after(&mut vm, &current, 48);
    let error = session
        .runtime_mut()
        .invoke_interface_method(&fresh, &current_run, &[RootedValue::from(3_i32)])
        .unwrap_err();
    drop(session);
    assert!(format!("{error:?}").contains("cancel"), "{error:?}");
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().root_registrations, base_roots + 2);
    assert!(!vm.is_quarantined());
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[RootedValue::from(0_i32)])
            .unwrap()
            .as_i32()
            .unwrap(),
        43
    );
    drop((saved, fresh));
    assert_cold_reclamation(&mut vm, &_old_program, replacement);
    assert_eq!(vm.modules().retention_counts(old_key).runtime_values, 0);
}
*/

#[rstest]
#[case::default(include_str!("../../../../tests/fixtures/embedding/reload/generic-default-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/generic-default-v2.kgr"))]
#[case::explicit_override(include_str!("../../../../tests/fixtures/embedding/reload/generic-override-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/generic-override-v2.kgr"))]
fn retained_generic_default_and_override_closures_use_current_operations_after_reload(
    #[case] source: &str,
    #[case] replacement: &str,
) {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, source);
    let replacement = prepare(&engine, replacement);
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = {
        let mut registered = CoreRuntime::new(config);
        NativeModule::install_all(&stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    let old = load_bytecode(&mut runtime, "generic-reload", program).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let selected_default =
        ModuleDecl::method_id(contract.declaration, "selected_default", old.definitions()).unwrap();
    let constructed =
        ModuleDecl::method_id(contract.declaration, "constructed", old.definitions()).unwrap();
    let late_constructed =
        ModuleDecl::method_id(contract.declaration, "late_constructed", old.definitions()).unwrap();
    let method_paths = [run, selected_default, constructed, late_constructed]
        .map(|id| old.definitions().resolve(id).unwrap().to_path());
    let old_key = old.key();
    let mut vm = runtime;
    let saved = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("generic-reload".into()),
        replacement.clone(),
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    let [
        current_run,
        current_selected_default,
        current_constructed,
        current_late_constructed,
    ] = method_paths.map(|path| current.definitions().lookup(&path).unwrap());
    drop(old);
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(old_key).is_some());
    let fresh = vm
        .execute_module(&current, "make", vm.execution_options())
        .unwrap()
        .return_value;
    assert!(
        !vm.invoke_interface_method(&saved, &run, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !vm.invoke_interface_method(&fresh, &current_run, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !vm.invoke_interface_method(&saved, &selected_default, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !vm.invoke_interface_method(&fresh, &current_selected_default, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !vm.invoke_interface_method(&saved, &constructed, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !vm.invoke_interface_method(&fresh, &current_constructed, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !vm.invoke_interface_method(&saved, &late_constructed, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    assert!(
        !vm.invoke_interface_method(&fresh, &current_late_constructed, &[])
            .unwrap()
            .as_bool()
            .unwrap()
    );
    drop((saved, fresh));
    assert_cold_reclamation(&mut vm, &_old_program, replacement);
    assert_eq!(vm.modules().retention_counts(old_key).runtime_values, 0);
}

#[test]
fn retained_generic_try_calls_use_current_carrier_operations() {
    let source = include_str!("../../../../tests/fixtures/embedding/reload/try-carrier-v1.kgr");
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        include_str!("../../../../tests/fixtures/embedding/reload/try-carrier-v2.kgr"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = load_bytecode(&mut runtime, "try-reload", program).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let run_path = old.definitions().resolve(run).unwrap().to_path();
    let old_key = old.key();
    let mut vm = runtime;
    let saved = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let stopped = vm
        .execute_module(&old, "make_stopped", vm.execution_options())
        .unwrap()
        .return_value;
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("try-reload".into()),
        replacement.clone(),
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    let current_run = current.definitions().lookup(&run_path).unwrap();
    drop(old);
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(old_key).is_some());
    let fresh = vm
        .execute_module(&current, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let fresh_stopped = vm
        .execute_module(&current, "make_stopped", vm.execution_options())
        .unwrap()
        .return_value;
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[])
            .unwrap()
            .as_i32()
            .unwrap(),
        43
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &current_run, &[])
            .unwrap()
            .as_i32()
            .unwrap(),
        43
    );
    for (value, method) in [(&stopped, &run), (&fresh_stopped, &current_run)] {
        assert_eq!(
            vm.invoke_interface_method(value, method, &[])
                .unwrap()
                .as_i32()
                .unwrap(),
            0
        );
    }
    drop((saved, fresh, stopped, fresh_stopped));
    assert_cold_reclamation(&mut vm, &_old_program, replacement);
    assert_eq!(vm.modules().retention_counts(old_key).runtime_values, 0);
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn content_helper_uses_current_interface_iteration_for_retained_values() {
    let source =
        include_str!("../../../../tests/fixtures/embedding/reload/interface-iteration-v1.kgr");
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let original = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        include_str!("../../../../tests/fixtures/embedding/reload/interface-iteration-v2.kgr"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = load_bytecode(&mut runtime, "receiver-reload", original).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_key = old.key();
    let mut vm = runtime;
    let saved = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("receiver-reload".into()),
        replacement.clone(),
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    drop(old);
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(old_key).is_some());
    let fresh = vm
        .execute_module(&current, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let probe = vm
        .execute_module(&current, "probe", vm.execution_options())
        .unwrap()
        .return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "same", current.definitions()).unwrap();
    for (root, expected) in [(&saved, false), (&fresh, false)] {
        assert_eq!(
            vm.invoke_interface_method(&probe, &method, &[root.clone(), root.clone()])
                .unwrap()
                .as_bool()
                .unwrap(),
            expected
        );
    }
    drop((saved, fresh, probe));
    assert_cold_reclamation(&mut vm, &_old_program, replacement);
    assert_eq!(vm.modules().retention_counts(old_key).runtime_values, 0);
}
*/

enum ReloadPreparation {
    /// Preserve the application inventory and test independently compiled artifacts.
    BodyOnlyArtifact,
    /// New source uses replace old applications still required by retained callers.
    #[expect(dead_code, reason = "stdlib tests temporarily commented out")]
    ExpandRetainedApplications,
}

#[rstest]
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::nominal(include_str!("../../../../tests/fixtures/embedding/reload/collection-nominal-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-nominal-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
*/
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::new_primitive(include_str!("../../../../tests/fixtures/embedding/reload/collection-nominal-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-new_primitive-v2.kgr"), ReloadPreparation::ExpandRetainedApplications)]
*/
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::retained_local_element(include_str!("../../../../tests/fixtures/embedding/reload/collection-retained_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-retained_local_element-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
*/
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::new_argument(include_str!("../../../../tests/fixtures/embedding/reload/collection-retained_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-new_argument-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
*/
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::same_nominal_argument(include_str!("../../../../tests/fixtures/embedding/reload/collection-same_nominal_argument-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-same_nominal_argument-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
*/
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::tuple_local_element(include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_local_element-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
*/
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::tuple_new_argument(include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_new_argument-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
*/
/* Temporarily paused: stdlib restoration (NC01-NC06).
#[case::tuple_same_nominal(include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_same_nominal-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_same_nominal-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
*/
#[case::bound_construction(include_str!("../../../../tests/fixtures/embedding/reload/collection-bound_construction-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-bound_construction-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
fn shared_collection_comparison_selects_current_operations(
    #[case] source: &str,
    #[case] replacement_source: &str,
    #[case] preparation: ReloadPreparation,
) {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let original = prepare(&engine, source);
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = load_bytecode(&mut runtime, "shared-comparison-reload", original).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_definitions = old.definitions().clone();
    let old_key = old.key();
    let replacement = match preparation {
        ReloadPreparation::BodyOnlyArtifact => prepare(&engine, replacement_source),
        ReloadPreparation::ExpandRetainedApplications => {
            prepare_with_demands(&engine, replacement_source, &old)
        }
    };
    let mut vm = runtime;
    let factory = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let old_probe = vm
        .execute_module(&old, "probe", vm.execution_options())
        .unwrap()
        .return_value;
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("shared-comparison-reload".into()),
        replacement.clone(),
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    drop(old);
    vm.collect_garbage().unwrap();
    let fresh_probe = vm
        .execute_module(&current, "probe", vm.execution_options())
        .unwrap()
        .return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "check", current.definitions()).unwrap();
    let old_method = old_definitions
        .lookup(&current.definitions().resolve(method).unwrap().to_path())
        .unwrap();
    for (probe, method, expected) in [
        (&old_probe, &old_method, false),
        (&fresh_probe, &method, false),
    ] {
        assert_eq!(
            vm.invoke_interface_method(probe, method, slice::from_ref(&factory),)
                .unwrap()
                .as_bool()
                .unwrap(),
            expected,
        );
    }
    drop((factory, old_probe, fresh_probe));
    assert_cold_reclamation(&mut vm, &_old_program, replacement);
    assert_eq!(vm.modules().retention_counts(old_key).runtime_values, 0,);
}
