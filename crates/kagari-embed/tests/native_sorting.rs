//! Registered sorting preserves stable preparation, callback effects and cleanup.
#[path = "fixtures/native_sorting_api.rs"]
pub mod fixture_api;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_sorting.kbc");
const ENTRIES: [&str; 8] = [
    "selected_main",
    "supplied_main",
    "script_main",
    "application_main",
    "application_supplied_main",
    "application_reverse_main",
    "edge_main",
    "many_main",
];
fn engine() -> KagariEngine {
    engine_with_inputs(&fixture_api::inputs::Inputs::default())
}
fn engine_with_inputs(inputs: &fixture_api::inputs::Inputs) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(fixture_api::api_with_inputs(inputs))
        .build()
        .unwrap()
}
fn prepared(bytes: &[u8]) -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}
fn values() -> Vec<Value> {
    vec![Value::I32(3), Value::I32(1), Value::I32(2)]
}

#[test]
fn actual_registered_methods_own_selected_requirements_and_tooling() {
    let api = fixture_api::api().unwrap();
    let module = api
        .modules()
        .iter()
        .find(|module| module.identity.path == ["array"])
        .unwrap();
    let declarations = module.native_declarations();
    let sort = declarations
        .iter()
        .find(|declaration| declaration.function.name == "sort")
        .unwrap();
    let supplied = declarations
        .iter()
        .find(|declaration| declaration.function.name == "sort_by")
        .unwrap();
    assert_eq!(sort.function.params.len(), 1);
    assert_eq!(sort.callable_requirements.len(), 1);
    assert_eq!(
        sort.callable_requirements[0]
            .interface
            .declaration
            .module
            .path,
        ["cmp"]
    );
    assert_eq!(
        sort.callable_requirements[0]
            .member
            .path
            .last()
            .unwrap()
            .name,
        "cmp"
    );
    assert!(!sort.function.bounds.is_empty());
    assert_eq!(supplied.function.params.len(), 2);
    assert!(supplied.function.bounds.is_empty());
    assert!(supplied.callable_requirements.is_empty());
    assert_eq!(
        module.declaration_source().unwrap().text,
        include_str!("../../../stdlib/array.kgr")
    );
}
#[test]
fn offline_selected_supplied_and_application_algorithms_sort_under_frequent_gc() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in ENTRIES {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::Bool(true),
            "{entry}"
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}
#[test]
fn repeated_selected_conversions_keep_a_bounded_number_of_temporary_roots() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine_with_inputs(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let mut peaks = Vec::new();
    let mut calls = Vec::new();
    for entry in ["script_main", "many_rank_main"] {
        inputs.root_peak.set(0);
        inputs.comparisons.set(0);
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::Bool(true)
        );
        peaks.push(inputs.root_peak.get());
        calls.push(inputs.comparisons.get());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
    assert!(calls[0] > 0 && calls[1] > 40 * calls[0], "{calls:?}");
    assert!(peaks[0] > 0);
    assert_eq!(
        peaks[0], peaks[1],
        "roots must depend on active frames, not comparison history"
    );
}
#[test]
fn cancellation_during_a_callback_releases_preparation_and_preserves_slots() {
    let inputs = fixture_api::inputs::Inputs::default();
    inputs.cancel_after.set(Some(1));
    let engine = engine_with_inputs(&inputs);
    let context = ExecutionContext {
        cancellation: inputs.cancellation.clone(),
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime.runtime().alloc_array(values()).unwrap();
    let root = runtime.runtime().root_value(Value::Array(id)).unwrap();
    *inputs.values.borrow_mut() = Some(root);
    let error = runtime
        .execute(&loaded, "supplied_argument", &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
    assert_eq!(inputs.comparisons.get(), 1);
    assert_eq!(runtime.runtime().gc().array_snapshot(id).unwrap(), values());
    assert_eq!(runtime.runtime().gc().active_roots(), 1);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 1);
    runtime
        .runtime()
        .gc()
        .array_set(id, 0, Value::I32(3))
        .unwrap();
    inputs.cancel_after.set(None);
    runtime
        .execute(&loaded, "sort_argument", &[], &ExecutionContext::default())
        .unwrap();
    assert_eq!(
        runtime.runtime().gc().array_snapshot(id).unwrap(),
        vec![Value::I32(1), Value::I32(2), Value::I32(3)]
    );
}
#[test]
fn every_budget_cut_keeps_original_slots_until_commit_and_releases_guards() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine_with_inputs(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime.runtime().alloc_array(values()).unwrap();
    let root = runtime.runtime().root_value(Value::Array(id)).unwrap();
    *inputs.values.borrow_mut() = Some(root.clone());
    for (entry, expected) in [
        (
            "sort_argument",
            vec![Value::I32(1), Value::I32(2), Value::I32(3)],
        ),
        (
            "supplied_argument",
            vec![Value::I32(3), Value::I32(2), Value::I32(1)],
        ),
    ] {
        let mut finished = false;
        let mut commit_cut = None;
        let mut preparation_cuts = 0;
        let mut committed_cuts = 0;
        for limit in 0..2000 {
            for (slot, value) in values().into_iter().enumerate() {
                runtime.runtime().gc().array_set(id, slot, value).unwrap();
            }
            let mut limited = context.clone();
            limited.resources.max_instruction_steps = Some(limit);
            match runtime.execute(&loaded, entry, &[], &limited) {
                Ok(report) => {
                    assert_eq!(report.return_value, Value::Unit);
                    assert_eq!(runtime.runtime().gc().array_snapshot(id).unwrap(), expected);
                    assert!(
                        commit_cut.is_some(),
                        "missing post-commit interruption: {entry}"
                    );
                    finished = true;
                }
                Err(error) => {
                    assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
                    let snapshot = runtime.runtime().gc().array_snapshot(id).unwrap();
                    if snapshot == values() {
                        assert!(commit_cut.is_none(), "commit regressed at {entry}/{limit}");
                        preparation_cuts += 1;
                    } else {
                        assert_eq!(snapshot, expected, "partial commit at {entry}/{limit}");
                        commit_cut.get_or_insert(limit);
                        committed_cuts += 1;
                    }
                }
            }
            assert_eq!(runtime.runtime().gc().active_roots(), 1);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(!runtime.runtime().is_quarantined());
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 1);
            // Even an interruption before Close must release iterator/mutation guards.
            runtime
                .runtime()
                .gc()
                .array_set(id, 0, Value::I32(3))
                .unwrap();
            if finished {
                break;
            }
        }
        assert!(finished, "{entry}");
        assert!(preparation_cuts > 0 && committed_cuts > 0, "{entry}");
    }
}
#[test]
fn allocation_limits_leave_target_unchanged_and_do_not_leak_preparation() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine_with_inputs(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime.runtime().alloc_array(values()).unwrap();
    let root = runtime.runtime().root_value(Value::Array(id)).unwrap();
    *inputs.values.borrow_mut() = Some(root.clone());
    let mut failed = false;
    let mut succeeded = false;
    for limit in 4..64 {
        for (slot, value) in values().into_iter().enumerate() {
            runtime.runtime().gc().array_set(id, slot, value).unwrap();
        }
        let mut limited = context.clone();
        limited.resources.max_heap_units = Some(limit);
        match runtime.execute(&loaded, "sort_argument", &[], &limited) {
            Ok(report) => {
                assert_eq!(report.return_value, Value::Unit);
                assert_eq!(
                    runtime.runtime().gc().array_snapshot(id).unwrap(),
                    vec![Value::I32(1), Value::I32(2), Value::I32(3)]
                );
                succeeded = true;
            }
            Err(error) => {
                assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
                assert_eq!(runtime.runtime().gc().array_snapshot(id).unwrap(), values());
                failed = true;
            }
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 1);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 1);
    }
    assert!(failed && succeeded);
}
#[test]
fn alias_writes_recursive_sorting_and_independent_iteration_cannot_change_slots() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine_with_inputs(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime.runtime().alloc_array(values()).unwrap();
    let root = runtime.runtime().root_value(Value::Array(id)).unwrap();
    *inputs.values.borrow_mut() = Some(root.clone());
    for entry in ["guarded_argument", "recursive_argument"] {
        let error = runtime.execute(&loaded, entry, &[], &context).unwrap_err();
        assert!(
            format!("{error:?}").contains("guarded callback"),
            "{error:?}"
        );
        assert_eq!(runtime.runtime().gc().array_snapshot(id).unwrap(), values());
        assert_eq!(runtime.runtime().gc().active_roots(), 1);
        runtime
            .runtime()
            .gc()
            .array_set(id, 0, Value::I32(3))
            .unwrap();
    }
    let guard = runtime
        .runtime()
        .gc()
        .begin_collection_iteration(&root.value())
        .unwrap();
    runtime
        .execute(&loaded, "sort_argument", &[], &context)
        .unwrap_err();
    assert_eq!(runtime.runtime().gc().array_snapshot(id).unwrap(), values());
    assert_eq!(runtime.runtime().gc().active_roots(), 2);
    drop(guard);
    runtime
        .execute(&loaded, "sort_argument", &[], &context)
        .unwrap();
    assert_eq!(
        runtime.runtime().gc().array_snapshot(id).unwrap(),
        vec![Value::I32(1), Value::I32(2), Value::I32(3)]
    );
}
#[test]
fn callback_trap_preserves_completed_payload_effects_without_committing_order() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine_with_inputs(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime.runtime().alloc_array(values()).unwrap();
    let root = runtime.runtime().root_value(Value::Array(id)).unwrap();
    *inputs.values.borrow_mut() = Some(root.clone());
    let error = runtime
        .execute(&loaded, "trap_argument", &[], &context)
        .unwrap_err();
    let trace = error.error_trace().unwrap();
    assert!(!trace.incomplete);
    assert_eq!(trace.omitted_frames, 0);
    assert_eq!(trace.frames.len(), 2);
    assert!(trace.frames[0].function_name.starts_with("closure_"));
    assert_eq!(trace.frames[1].function_name, "trap_argument");
    for frame in &trace.frames {
        assert_eq!(frame.source_uri, "memory://native-sorting.kgr");
        assert!(frame.source_span.is_some());
        assert!(frame.line.is_some() && frame.column.is_some());
    }
    let span = trace.frames[0].source_span.unwrap();
    assert_eq!(
        &include_str!("fixtures/native_sorting.kgr")[span.start..span.end],
        "(left / divisor).cmp(right)"
    );
    assert_eq!(inputs.effects.get(), 1);
    assert_eq!(runtime.runtime().gc().array_snapshot(id).unwrap(), values());
    assert_eq!(runtime.runtime().gc().active_roots(), 1);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime
        .execute(&loaded, "sort_argument", &[], &context)
        .unwrap();
}
#[test]
fn forged_selected_comparator_types_reject_portable_verification() {
    let mut artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let api = fixture_api::api().unwrap();
    let module = api
        .modules()
        .iter()
        .find(|module| module.identity.path == ["array"])
        .unwrap();
    let declaration = module
        .native_declarations()
        .into_iter()
        .find(|declaration| declaration.function.name == "sort")
        .unwrap()
        .declaration;
    let import = artifact
        .program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.native_imports)
        .find(|import| import.instance.declaration == declaration)
        .unwrap();
    import.callables[0].signature.params[1] = import.signature.params[0].clone();
    artifact.portable_mir = None;
    assert!(
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).is_err()
    );
}
#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};
    const SOURCE: &str = include_str!("fixtures/native_sorting.kgr");
    #[test]
    fn independently_registered_sorting_emits_the_exact_product() {
        let artifact = engine()
            .compile_to_artifact(
                SourceFile::new("memory://native-sorting.kgr", SOURCE),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }
    #[test]
    fn missing_total_order_and_readonly_targets_are_static_errors() {
        for source in [
            "fn invalid(values: ArrayList<f64>) { values.sort(); }",
            "fn invalid(values: [i32]) { values.sort(); }",
        ] {
            assert!(
                engine()
                    .compile_to_artifact(
                        SourceFile::new("memory://invalid-sort.kgr", format!("{SOURCE}\n{source}")),
                        Default::default(),
                        Default::default()
                    )
                    .is_err()
            );
        }
    }
    #[test]
    fn selected_script_comparators_retain_their_reload_generation() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-sorting.kgr",
                    SOURCE.replace("self.key < other.key", "self.key > other.key"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let old = runtime
            .load_program(&prepared(ARTIFACT), Default::default())
            .unwrap();
        let new = runtime
            .reload_program(
                &old,
                &prepared(&artifact.to_bytes().unwrap()),
                Default::default(),
            )
            .unwrap();
        for (loaded, expected) in [(&old, true), (&new, false)] {
            assert_eq!(
                runtime
                    .execute(loaded, "script_main", &[], &context)
                    .unwrap()
                    .return_value,
                Value::Bool(expected)
            );
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
        }
    }
    #[test]
    fn inherent_sorting_navigation_uses_registered_signature_coordinates() {
        let engine = engine();
        let text = "use std::array::ArrayList; fn run(values: ArrayList<i32>) { values.sort(); }";
        let file = engine
            .set_source("memory://sort-tooling.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = engine
            .analyze(
                engine.source_snapshot(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
        assert!(
            snapshot
                .file(file)
                .unwrap()
                .result()
                .diagnostics()
                .is_empty()
        );
        let definition = snapshot
            .definition_at(file, text.find("sort()").unwrap())
            .unwrap();
        let source = snapshot.source(definition.location.file).unwrap();
        assert_eq!(source.name(), "kagari://native/kagari-std/array.kgr");
        assert_eq!(
            &source.text()[definition.location.range.start..definition.location.range.end],
            "sort"
        );
    }
}
