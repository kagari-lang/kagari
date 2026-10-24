#![cfg(feature = "source")]
use crate::test_support::cancellation as support;
use crate::{
    BytecodeArtifact,
    engine::{
        KagariEngine,
        source::{ArtifactOptions, compilation::Compilation},
    },
    program::PreparedProgram,
    runtime::Runtime,
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    Runtime as CoreRuntime, RuntimeConfig,
    module::{ExecutableModule, Program},
    native::module::NativeModule,
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;
use rstest::rstest;

pub(super) fn prepare_static_program(
    engine: &KagariEngine,
    source: &str,
    decoded: bool,
) -> PreparedProgram {
    let checked = engine
        .compile_source(SourceFile::new("static-values.kgr", source))
        .unwrap();
    if !decoded {
        return engine.prepare(&checked, Default::default()).unwrap();
    }
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    PreparedProgram::from_artifact(
        BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn static_call(runtime: &Runtime, module: &Program, entry: &str) -> Value {
    runtime
        .execute(module, entry, &[], &Default::default())
        .unwrap()
        .return_value
        .value(runtime.runtime().gc())
        .unwrap()
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
    let engine = KagariEngine::default();
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
    let old = runtime.load_program(&first, Default::default()).unwrap();
    assert_eq!(static_call(&runtime, &old, "read"), Value::I64(97));
    static_call(&runtime, &old, "write");
    assert_eq!(
        runtime
            .reload_program(&old, &changed_initializer, Default::default())
            .unwrap_err()
            .code(),
        "KG_RELOAD_UPDATE_EVIDENCE"
    );
    runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    let current = old.clone();
    assert!(
        runtime
            .reload_program(&current, &incompatible, Default::default())
            .is_err()
    );
    for module in [&old, &current] {
        assert_eq!(static_call(&runtime, module, "read"), Value::I64(128578));
    }
}

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
    let engine = KagariEngine::default();
    let first = prepare_static_program(&engine, source, decoded);
    let changed_initializer =
        prepare_static_program(&engine, &source.replace("value: 1,", "value: 2,"), decoded);
    let second = prepare_static_program(
        &engine,
        &source.replace("value: 90,", "value: 91,"),
        decoded,
    );
    let mut runtime = engine.runtime(Default::default());
    let old = runtime.load_program(&first, Default::default()).unwrap();
    assert_eq!(static_call(&runtime, &old, "check"), Value::Bool(true));
    assert_eq!(
        runtime
            .reload_program(&old, &changed_initializer, Default::default())
            .unwrap_err()
            .code(),
        "KG_RELOAD_UPDATE_EVIDENCE"
    );
    runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    let current = old.clone();
    static_call(&runtime, &current, "replace");
    for module in [&old, &current] {
        assert_eq!(static_call(&runtime, module, "check"), Value::Bool(true));
        assert_eq!(static_call(&runtime, module, "distinct"), Value::Bool(true));
        assert_eq!(static_call(&runtime, module, "read"), Value::I32(791));
    }
}

const STATIC_SOURCE: &str =
    include_str!("../../../../tests/fixtures/embedding/reload/static-values-v1.kgr");

#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_callbacks_and_interfaces_call_current_helpers(#[case] decoded: bool) {
    let source = STATIC_SOURCE;
    // Initializer expressions stay fixed; retained values call current helpers.
    // Explicit assignment still replaces cells without changing their saved aliases.
    let replacement =
        include_str!("../../../../tests/fixtures/embedding/reload/static-callbacks-v2.kgr");
    let engine = KagariEngine::default();
    let first = prepare_static_program(&engine, source, decoded);
    let second = prepare_static_program(&engine, replacement, decoded);
    let mut runtime = engine.runtime(Default::default());
    let old = runtime.load_program(&first, Default::default()).unwrap();
    assert_eq!(static_call(&runtime, &old, "init"), Value::Bool(true));
    // Force the aliases before replacing their original cells.
    assert_eq!(static_call(&runtime, &old, "saved"), Value::I32(22));
    assert_eq!(static_call(&runtime, &old, "function"), Value::I32(10));
    runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    let current = old.clone();
    static_call(&runtime, &current, "set_state");

    assert_eq!(
        static_call(&runtime, &current, "fresh_lookup"),
        Value::Bool(true)
    );
    assert_eq!(static_call(&runtime, &current, "current"), Value::I32(50));
    assert_eq!(static_call(&runtime, &current, "function"), Value::I32(20));
    static_call(&runtime, &current, "replace");
    assert_eq!(static_call(&runtime, &current, "current"), Value::I32(70));
    assert_eq!(static_call(&runtime, &current, "saved"), Value::I32(50));
    assert_eq!(static_call(&runtime, &old, "current"), Value::I32(70));
}

#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_hash_comparators_use_current_bodies_and_helpers(#[case] decoded: bool) {
    let source = STATIC_SOURCE;
    let bodies =
        include_str!("../../../../tests/fixtures/embedding/reload/static-hash-bodies-v2.kgr");
    let helpers =
        include_str!("../../../../tests/fixtures/embedding/reload/static-hash-helpers-v3.kgr");
    let engine = KagariEngine::default();
    let first = prepare_static_program(&engine, source, decoded);
    let mut runtime = engine.runtime(Default::default());
    let old = runtime.load_program(&first, Default::default()).unwrap();
    for entry in ["map_lookup", "set_lookup"] {
        assert_eq!(static_call(&runtime, &old, entry), Value::Bool(true));
    }
    assert_eq!(
        static_call(&runtime, &old, "compare_contents"),
        Value::I32(15)
    );
    for (updated, expected) in [(bodies, 2323), (helpers, 2424)] {
        let candidate = prepare_static_program(&engine, updated, decoded);
        runtime
            .reload_program(&old, &candidate, Default::default())
            .unwrap();
        for entry in ["map_lookup", "set_lookup"] {
            static_call(&runtime, &old, "reset_markers");
            assert_eq!(static_call(&runtime, &old, entry), Value::Bool(true));
            assert_eq!(static_call(&runtime, &old, "markers"), Value::I32(expected));
        }
    }
    let current = old.clone();
    assert_eq!(
        static_call(&runtime, &current, "compare_contents"),
        Value::I32(15)
    );
    assert_eq!(
        static_call(&runtime, &current, "content_hashes"),
        Value::Bool(true)
    );
    assert_eq!(
        static_call(&runtime, &current, "fresh_lookup"),
        Value::Bool(true)
    );
    for entry in ["map_lookup", "set_lookup", "mutate_keys"] {
        assert_eq!(static_call(&runtime, &current, entry), Value::Bool(true));
    }
    let changed_hash =
        include_str!("../../../../tests/fixtures/embedding/reload/static-hash-change-v4.kgr");
    let candidate = prepare_static_program(&engine, changed_hash, decoded);
    runtime
        .reload_program(&old, &candidate, Default::default())
        .unwrap();
    for entry in ["map_lookup", "set_lookup"] {
        static_call(&runtime, &old, "reset_markers");
        // Changed hashes need not find old entries. Only current dispatch is asserted.
        let _ = static_call(&runtime, &old, entry);
        assert_eq!(static_call(&runtime, &old, "hash_observed"), Value::I32(34));
    }
}

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
    let compilation = Compilation::lower_with_demands(
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

#[test]
fn retained_local_function_values_call_current_bodies_and_clean_up_all_exits() {
    let source = include_str!("../../../../tests/fixtures/embedding/reload/local-functions-v1.kgr");
    let engine = KagariEngine::default();
    let program = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        include_str!("../../../../tests/fixtures/embedding/reload/local-functions-v2.kgr"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    config.limits.max_call_depth = Some(32);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime.load_program("local-reload", program).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved_root = vm.execute(&old, "make").unwrap().return_value;
    let saved = saved_root.value(vm.runtime().gc()).unwrap();
    vm.reload_program(&_old_program, Some("local-reload".into()), replacement)
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh_root = vm.execute(&current, "make").unwrap().return_value;
    let fresh = fresh_root.value(vm.runtime().gc()).unwrap();
    for (value, expected) in [(&saved, 43), (&fresh, 43)] {
        assert_eq!(
            vm.invoke_interface_method(value, &run, &[Value::I32(0)])
                .unwrap(),
            Value::I32(expected)
        );
    }
    for (mode, expected) in [(1, "division by zero"), (2, "call depth")] {
        let error = vm
            .invoke_interface_method(&saved, &run, &[Value::I32(mode)])
            .unwrap_err();
        assert!(format!("{error:?}").contains(expected), "{error:?}");
        assert_eq!(vm.runtime().gc().active_roots(), 2);
        assert!(!vm.runtime().is_quarantined());
    }
    let session = support::cancel_after(vm.runtime(), &current, 48);
    let error = vm
        .invoke_interface_method(&fresh, &run, &[Value::I32(3)])
        .unwrap_err();
    drop(session);
    assert!(format!("{error:?}").contains("cancel"), "{error:?}");
    assert_eq!(vm.runtime().gc().active_roots(), 2);
    assert!(!vm.runtime().is_quarantined());
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[Value::I32(0)])
            .unwrap(),
        Value::I32(43)
    );
    drop((saved_root, fresh_root));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

#[rstest]
#[case::default(include_str!("../../../../tests/fixtures/embedding/reload/generic-default-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/generic-default-v2.kgr"))]
#[case::explicit_override(include_str!("../../../../tests/fixtures/embedding/reload/generic-override-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/generic-override-v2.kgr"))]
fn retained_generic_default_and_override_closures_use_current_operations_after_reload(
    #[case] source: &str,
    #[case] replacement: &str,
) {
    let engine = KagariEngine::default();
    let program = prepare(&engine, source);
    let replacement = prepare(&engine, replacement);
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = {
        let mut registered = CoreRuntime::new(config);
        kagari_runtime::native::module::NativeModule::install_all(
            &kagari_stdlib::modules().unwrap(),
            &mut registered,
        )
        .unwrap();
        registered
    };
    let old = runtime.load_program("generic-reload", program).unwrap();
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
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(saved).unwrap();
    vm.reload_program(&_old_program, Some("generic-reload".into()), replacement)
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm
        .execute(&current, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let fresh_root = vm.runtime().root_value(fresh).unwrap();
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &run, &[]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        vm.invoke_interface_method(&saved, &selected_default, &[])
            .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &selected_default, &[])
            .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        vm.invoke_interface_method(&saved, &constructed, &[])
            .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &constructed, &[])
            .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        vm.invoke_interface_method(&saved, &late_constructed, &[])
            .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &late_constructed, &[])
            .unwrap(),
        Value::Bool(false)
    );
    drop((root, fresh_root));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

#[test]
fn retained_generic_try_calls_use_current_carrier_operations() {
    let source = include_str!("../../../../tests/fixtures/embedding/reload/try-carrier-v1.kgr");
    let engine = KagariEngine::default();
    let program = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        include_str!("../../../../tests/fixtures/embedding/reload/try-carrier-v2.kgr"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let old = runtime.load_program("try-reload", program).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(saved).unwrap();
    let stopped = vm
        .execute(&old, "make_stopped")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let stopped_root = vm.runtime().root_value(stopped).unwrap();
    vm.reload_program(&_old_program, Some("try-reload".into()), replacement)
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm
        .execute(&current, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let fresh_root = vm.runtime().root_value(fresh).unwrap();
    let fresh_stopped = vm
        .execute(&current, "make_stopped")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let fresh_stopped_root = vm.runtime().root_value(fresh_stopped).unwrap();
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[]).unwrap(),
        Value::I32(43)
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &run, &[]).unwrap(),
        Value::I32(43)
    );
    for value in [&stopped, &fresh_stopped] {
        assert_eq!(
            vm.invoke_interface_method(value, &run, &[]).unwrap(),
            Value::I32(0)
        );
    }
    drop((root, fresh_root, stopped_root, fresh_stopped_root));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

#[test]
fn content_helper_uses_current_interface_iteration_for_retained_values() {
    let source =
        include_str!("../../../../tests/fixtures/embedding/reload/interface-iteration-v1.kgr");
    let engine = KagariEngine::default();
    let original = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        include_str!("../../../../tests/fixtures/embedding/reload/interface-iteration-v2.kgr"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime.load_program("receiver-reload", original).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm.execute(&old, "make").unwrap().return_value;
    vm.reload_program(&_old_program, Some("receiver-reload".into()), replacement)
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm.execute(&current, "make").unwrap().return_value;
    let probe = vm.execute(&current, "probe").unwrap().return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "same", current.definitions()).unwrap();
    for (root, expected) in [(&saved, false), (&fresh, false)] {
        let value = root.value(vm.runtime().gc()).unwrap();
        assert_eq!(
            vm.invoke_interface_method(
                &probe.value(vm.runtime().gc()).unwrap(),
                &method,
                &[value, value]
            )
            .unwrap(),
            Value::Bool(expected)
        );
    }
    drop((saved, fresh, probe));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

enum ReloadPreparation {
    /// Preserve the application inventory and test independently compiled artifacts.
    BodyOnlyArtifact,
    /// New source uses replace old applications still required by retained callers.
    ExpandRetainedApplications,
}

#[rstest]
#[case::nominal(include_str!("../../../../tests/fixtures/embedding/reload/collection-nominal-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-nominal-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
#[case::new_primitive(include_str!("../../../../tests/fixtures/embedding/reload/collection-nominal-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-new_primitive-v2.kgr"), ReloadPreparation::ExpandRetainedApplications)]
#[case::retained_local_element(include_str!("../../../../tests/fixtures/embedding/reload/collection-retained_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-retained_local_element-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
#[case::new_argument(include_str!("../../../../tests/fixtures/embedding/reload/collection-retained_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-new_argument-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
#[case::same_nominal_argument(include_str!("../../../../tests/fixtures/embedding/reload/collection-same_nominal_argument-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-same_nominal_argument-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
#[case::tuple_local_element(include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_local_element-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
#[case::tuple_new_argument(include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_local_element-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_new_argument-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
#[case::tuple_same_nominal(include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_same_nominal-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-tuple_same_nominal-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
#[case::bound_construction(include_str!("../../../../tests/fixtures/embedding/reload/collection-bound_construction-v1.kgr"), include_str!("../../../../tests/fixtures/embedding/reload/collection-bound_construction-v2.kgr"), ReloadPreparation::BodyOnlyArtifact)]
fn shared_collection_comparison_selects_current_operations(
    #[case] source: &str,
    #[case] replacement_source: &str,
    #[case] preparation: ReloadPreparation,
) {
    let engine = KagariEngine::default();
    let original = prepare(&engine, source);
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = CoreRuntime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime
        .load_program("shared-comparison-reload", original)
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_key = old.key();
    let replacement = match preparation {
        ReloadPreparation::BodyOnlyArtifact => prepare(&engine, replacement_source),
        ReloadPreparation::ExpandRetainedApplications => {
            prepare_with_demands(&engine, replacement_source, &old)
        }
    };
    let vm = Vm::new(runtime);
    let factory = vm.execute(&old, "make").unwrap().return_value;
    let old_probe = vm.execute(&old, "probe").unwrap().return_value;
    vm.reload_program(
        &_old_program,
        Some("shared-comparison-reload".into()),
        replacement,
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    let fresh_probe = vm.execute(&current, "probe").unwrap().return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "check", current.definitions()).unwrap();
    for (probe, expected) in [(&old_probe, false), (&fresh_probe, false)] {
        assert_eq!(
            vm.invoke_interface_method(
                &probe.value(vm.runtime().gc()).unwrap(),
                &method,
                &[factory.value(vm.runtime().gc()).unwrap()],
            )
            .unwrap(),
            Value::Bool(expected),
        );
    }
    drop((factory, old_probe, fresh_probe));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0,
    );
}
