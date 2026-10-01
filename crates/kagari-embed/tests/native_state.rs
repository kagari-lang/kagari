//! Returned native state survives idle GC and shares checked, pinned progress.
#[path = "fixtures/native_state_api.rs"]
pub mod fixture_api;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{
    gc::GcHeap,
    value::{EnumTag, Value},
};
const ARTIFACT: &[u8] = include_bytes!("fixtures/native_state.kbc");
fn engine(inputs: &fixture_api::inputs::Inputs) -> KagariEngine {
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
fn option(heap: &GcHeap, value: Value) -> Option<i32> {
    let Value::Enum(id) = value else {
        panic!("Option expected: {value:?}")
    };
    let snapshot = heap.enum_snapshot(id).unwrap();
    match (&snapshot.tag, snapshot.fields.as_slice()) {
        (EnumTag::OptionSome, [Value::I32(value)]) => Some(*value),
        (EnumTag::OptionNone, []) => None,
        _ => panic!("Option<i32> expected: {snapshot:?}"),
    }
}
#[test]
fn source_and_mapped_progress_are_lazy_shared_and_non_fused_offline() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in ["shared_main", "non_fused_main", "heap_main"] {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::Bool(true)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}
#[test]
fn idle_gc_preserves_captured_closures_and_cycles_are_collectable() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for (factory, expected) in [
        ("factory", vec![Some(8), Some(9), Some(10), None]),
        ("cycle_factory", vec![Some(1), Some(2), Some(3)]),
    ] {
        let value = runtime
            .execute(&loaded, factory, &[], &context)
            .unwrap()
            .return_value;
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
        for expected in expected {
            runtime.runtime().collect_garbage().unwrap();
            let result = runtime
                .execute(&loaded, "next_input", &[], &context)
                .unwrap()
                .return_value;
            assert_eq!(option(runtime.runtime().gc(), result), expected);
            assert_eq!(runtime.runtime().gc().active_roots(), 1);
        }
        inputs.cursor.borrow_mut().take();
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn nested_guards_release_after_callback_traps_and_source_progress_is_committed() {
    for (factory, message, effects) in [
        ("trapped_factory", "division", 1),
        ("structural_factory", "iterat", 0),
        ("reentry_factory", "already active", 0),
    ] {
        let inputs = fixture_api::inputs::Inputs::default();
        let engine = engine(&inputs);
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(&prepared(ARTIFACT), Default::default())
            .unwrap();
        let id = runtime
            .runtime()
            .alloc_array(vec![Value::I32(1), Value::I32(2)])
            .unwrap();
        *inputs.values.borrow_mut() = Some(runtime.runtime().root_value(Value::Array(id)).unwrap());
        let value = runtime
            .execute(&loaded, factory, &[], &context)
            .unwrap()
            .return_value;
        *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
        let error = runtime
            .execute(&loaded, "next_input", &[], &context)
            .unwrap_err();
        assert!(
            format!("{error:?}").contains(message),
            "{factory}: {error:?}"
        );
        assert_eq!(inputs.effects.get(), effects);
        assert_eq!(runtime.runtime().gc().active_roots(), 2);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime
            .runtime()
            .gc()
            .array_set(id, 0, Value::I32(20))
            .unwrap();
        if factory == "trapped_factory" {
            let result = runtime
                .execute(&loaded, "next_input", &[], &context)
                .unwrap()
                .return_value;
            assert_eq!(option(runtime.runtime().gc(), result), Some(2));
            assert_eq!(inputs.effects.get(), 2);
        }
        runtime
            .runtime()
            .gc()
            .array_push(id, Value::I32(999))
            .unwrap();
        inputs.cursor.borrow_mut().take();
        inputs.values.borrow_mut().take();
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn structural_change_while_idle_invalidates_the_source_without_leaking_guards() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime.runtime().alloc_array(vec![Value::I32(1)]).unwrap();
    *inputs.values.borrow_mut() = Some(runtime.runtime().root_value(Value::Array(id)).unwrap());
    let value = runtime
        .execute(&loaded, "array_factory", &[], &context)
        .unwrap()
        .return_value;
    *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
    runtime
        .runtime()
        .gc()
        .array_push(id, Value::I32(2))
        .unwrap();
    let error = runtime
        .execute(&loaded, "next_input", &[], &context)
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("structurally modified iterator source"),
        "{error:?}"
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 2);
    runtime
        .runtime()
        .gc()
        .array_push(id, Value::I32(3))
        .unwrap();
}

#[test]
fn cancellation_releases_active_state_and_preserves_completed_source_progress() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext {
        cancellation: inputs.cancellation.clone(),
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime
        .runtime()
        .alloc_array(vec![Value::I32(1), Value::I32(2)])
        .unwrap();
    *inputs.values.borrow_mut() = Some(runtime.runtime().root_value(Value::Array(id)).unwrap());
    let value = runtime
        .execute(&loaded, "probe_factory", &[], &context)
        .unwrap()
        .return_value;
    *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
    inputs.cancel.set(true);
    let error = runtime
        .execute(&loaded, "next_input", &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
    assert_eq!(inputs.effects.get(), 1);
    assert_eq!(runtime.runtime().gc().active_roots(), 2);
    runtime
        .runtime()
        .gc()
        .array_set(id, 0, Value::I32(9))
        .unwrap();
    inputs.cancel.set(false);
    let result = runtime
        .execute(&loaded, "next_input", &[], &ExecutionContext::default())
        .unwrap()
        .return_value;
    assert_eq!(option(runtime.runtime().gc(), result), Some(102));
    runtime
        .runtime()
        .gc()
        .array_push(id, Value::I32(999))
        .unwrap();
}

#[test]
fn every_budget_cut_releases_state_and_exposes_once_only_source_commit() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime
        .runtime()
        .alloc_array(vec![Value::I32(1), Value::I32(2)])
        .unwrap();
    *inputs.values.borrow_mut() = Some(runtime.runtime().root_value(Value::Array(id)).unwrap());
    let mut committed_cut = None;
    let mut callback_cut = None;
    let mut finished = false;
    for limit in 0..500 {
        inputs.cursor.borrow_mut().take();
        runtime.runtime().collect_garbage().unwrap();
        inputs.effects.set(0);
        let value = runtime
            .execute(&loaded, "probe_factory", &[], &context)
            .unwrap()
            .return_value;
        *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
        let mut limited = context.clone();
        limited.resources.max_instruction_steps = Some(limit);
        let result = runtime.execute(&loaded, "next_input", &[], &limited);
        let effects = inputs.effects.get();
        let succeeded = result.is_ok();
        if let Err(error) = result {
            assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 2, "cut {limit}");
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime
            .runtime()
            .gc()
            .array_set(id, 0, Value::I32(1))
            .unwrap();
        runtime.runtime().collect_garbage().unwrap();
        let next = runtime
            .execute(&loaded, "next_input", &[], &context)
            .unwrap()
            .return_value;
        let next = option(runtime.runtime().gc(), next);
        assert!(matches!(next, Some(101 | 102)), "cut {limit}: {next:?}");
        if next == Some(102) {
            committed_cut.get_or_insert(limit);
        } else {
            assert!(
                committed_cut.is_none(),
                "source commit regressed at {limit}"
            );
        }
        if effects != 0 {
            callback_cut.get_or_insert(limit);
            assert!(committed_cut.is_some());
        }
        runtime
            .runtime()
            .gc()
            .array_push(id, Value::I32(999))
            .unwrap();
        assert_eq!(
            runtime.runtime().gc().array_pop(id).unwrap(),
            Some(Value::I32(999))
        );
        if succeeded {
            finished = true;
            break;
        }
    }
    assert!(finished && committed_cut.is_some() && callback_cut.is_some());
    assert!(
        committed_cut.unwrap() < callback_cut.unwrap(),
        "a source commit must survive interruption before mapping"
    );
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};
    const SOURCE: &str = include_str!("fixtures/native_state.kgr");
    #[test]
    fn returned_representation_and_step_navigation_use_actual_registration_spans() {
        let inputs = fixture_api::inputs::Inputs::default();
        let engine = engine(&inputs);
        let text =
            "use game::stream::{Cursor, Source}; fn run() { Cursor::from_array([1]).next(); }";
        let file = engine
            .set_source("memory://state-tooling.kgr", text.into(), SourceLayer::Base)
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
        for member in ["from_array", "next"] {
            let definition = snapshot
                .definition_at(file, text.find(&format!("{member}(")).unwrap())
                .unwrap();
            let source = snapshot.source(definition.location.file).unwrap();
            assert_eq!(source.name(), "kagari://native/game/stream.kgr");
            assert_eq!(
                &source.text()[definition.location.range.start..definition.location.range.end],
                member
            );
        }
    }
    #[test]
    fn actual_registration_rebuilds_exact_portable_product_and_checks_callback_types() {
        let inputs = fixture_api::inputs::Inputs::default();
        let engine = engine(&inputs);
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://native-state.kgr", SOURCE),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
        for (before, after) in [
            ("|item| item + bias.calls", "|item: bool| item"),
            ("type Item = i32", "type Item = bool"),
        ] {
            assert!(
                engine
                    .compile_to_artifact(
                        SourceFile::new(
                            "memory://invalid-state.kgr",
                            SOURCE.replace(before, after)
                        ),
                        Default::default(),
                        Default::default()
                    )
                    .is_err()
            );
        }
    }
    #[test]
    fn idle_selected_targets_and_closures_pin_their_generation_across_reload() {
        let inputs = fixture_api::inputs::Inputs::default();
        let engine = engine(&inputs);
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let old = runtime
            .load_program(&prepared(ARTIFACT), Default::default())
            .unwrap();
        let value = runtime
            .execute(&old, "script_factory", &[], &context)
            .unwrap()
            .return_value;
        let old_cursor = runtime.runtime().root_value(value).unwrap();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-state.kgr",
                    SOURCE
                        .replace("self.position += 1", "self.position += 3")
                        .replace("calls: 7", "calls: 70"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let new = runtime
            .reload_program(
                &old,
                &prepared(&artifact.to_bytes().unwrap()),
                Default::default(),
            )
            .unwrap();
        let value = runtime
            .execute(&new, "script_factory", &[], &context)
            .unwrap()
            .return_value;
        let new_cursor = runtime.runtime().root_value(value).unwrap();
        assert!(
            runtime
                .runtime()
                .modules()
                .retention_counts(old.key())
                .runtime_values
                > 0
        );
        for (cursor, expected) in [
            (&old_cursor, Some(8)),
            (&new_cursor, Some(73)),
            (&old_cursor, None),
            (&new_cursor, None),
            (&old_cursor, Some(10)),
        ] {
            *inputs.cursor.borrow_mut() = Some(cursor.clone());
            runtime.runtime().collect_garbage().unwrap();
            let result = runtime
                .execute(&new, "next_input", &[], &context)
                .unwrap()
                .return_value;
            assert_eq!(option(runtime.runtime().gc(), result), expected);
            assert_eq!(runtime.runtime().gc().active_roots(), 2);
        }
        inputs.cursor.borrow_mut().take();
        drop(old_cursor);
        drop(new_cursor);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        assert_eq!(
            runtime
                .runtime()
                .modules()
                .retention_counts(old.key())
                .runtime_values,
            0
        );
    }
}

#[test]
fn typed_capture_updates_persist_and_invalid_capture_or_result_never_escape() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for factory in [
        "counter_factory",
        "invalid_capture_factory",
        "invalid_result_factory",
    ] {
        let value = runtime
            .execute(&loaded, factory, &[], &context)
            .unwrap()
            .return_value;
        *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
        if factory == "counter_factory" {
            for expected in [41, 42, 43] {
                runtime.runtime().collect_garbage().unwrap();
                let result = runtime
                    .execute(&loaded, "next_input", &[], &context)
                    .unwrap()
                    .return_value;
                assert_eq!(option(runtime.runtime().gc(), result), Some(expected));
            }
        } else {
            for _ in 0..2 {
                let error = runtime
                    .execute(&loaded, "next_input", &[], &context)
                    .unwrap_err();
                assert_eq!(error.code(), "KG_BYTECODE_VERIFICATION_FAILED", "{error:?}");
                assert_eq!(runtime.runtime().gc().active_roots(), 1);
                assert!(!runtime.runtime().is_quarantined());
            }
        }
        inputs.cursor.borrow_mut().take();
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn retained_step_access_is_invalid_after_completion_and_cannot_clear_a_new_lease() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let value = runtime
        .execute(&loaded, "lease_factory", &[], &context)
        .unwrap()
        .return_value;
    *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
    for expected in 0..3 {
        let result = runtime
            .execute(&loaded, "next_input", &[], &context)
            .unwrap()
            .return_value;
        assert_eq!(option(runtime.runtime().gc(), result), Some(expected));
        assert!(fixture_api::stream::escaped_is_invalid());
        runtime.runtime().collect_garbage().unwrap();
    }
    fixture_api::stream::clear_escaped();
    assert_eq!(runtime.runtime().gc().active_roots(), 1);
    let value = runtime
        .execute(&loaded, "lease_error_factory", &[], &context)
        .unwrap()
        .return_value;
    *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
    let error = runtime
        .execute(&loaded, "next_input", &[], &context)
        .unwrap_err();
    assert!(format!("{error:?}").contains("fixture state factory failed"));
    assert!(fixture_api::stream::escaped_is_invalid());
    let result = runtime
        .execute(&loaded, "next_input", &[], &context)
        .unwrap()
        .return_value;
    assert_eq!(option(runtime.runtime().gc(), result), Some(1));
    assert!(fixture_api::stream::escaped_is_invalid());
    fixture_api::stream::clear_escaped();
    assert_eq!(runtime.runtime().gc().active_roots(), 1);
    inputs.cursor.borrow_mut().take();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn repeated_steps_keep_temporary_roots_bounded_and_idle_traversal_guards_are_released() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime
        .runtime()
        .alloc_array((0..128).map(Value::I32).collect())
        .unwrap();
    *inputs.values.borrow_mut() = Some(runtime.runtime().root_value(Value::Array(id)).unwrap());
    let value = runtime
        .execute(&loaded, "probe_factory", &[], &context)
        .unwrap()
        .return_value;
    *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
    let loop_guard = runtime
        .runtime()
        .gc()
        .begin_collection_iteration(&inputs.cursor.borrow().as_ref().unwrap().value())
        .unwrap();
    assert!(
        runtime
            .runtime()
            .gc()
            .array_push(id, Value::I32(999))
            .is_err()
    );
    let held_roots = runtime.runtime().gc().active_roots();
    assert!(held_roots > 2);
    let mut peak = None;
    for expected in 100..228 {
        let result = runtime
            .execute(&loaded, "next_input", &[], &context)
            .unwrap()
            .return_value;
        assert_eq!(option(runtime.runtime().gc(), result), Some(expected));
        assert_eq!(
            *peak.get_or_insert(inputs.root_peak.get()),
            inputs.root_peak.get()
        );
        assert_eq!(runtime.runtime().gc().active_roots(), held_roots);
        runtime.runtime().collect_garbage().unwrap();
        assert!(
            runtime
                .runtime()
                .gc()
                .array_push(id, Value::I32(999))
                .is_err()
        );
    }
    drop(loop_guard);
    assert_eq!(runtime.runtime().gc().active_roots(), 2);
    runtime
        .runtime()
        .gc()
        .array_set(id, 0, Value::I32(0))
        .unwrap();
    inputs.cursor.borrow_mut().take();
    inputs.values.borrow_mut().take();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn allocation_failure_releases_state_before_and_after_source_commit() {
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    let id = runtime
        .runtime()
        .alloc_array(vec![Value::I32(1), Value::I32(2)])
        .unwrap();
    *inputs.values.borrow_mut() = Some(runtime.runtime().root_value(Value::Array(id)).unwrap());
    let mut failed_before = false;
    let mut failed_after = false;
    let mut finished = false;
    for limit in 0..100 {
        inputs.cursor.borrow_mut().take();
        runtime.runtime().collect_garbage().unwrap();
        let value = runtime
            .execute(&loaded, "probe_factory", &[], &context)
            .unwrap()
            .return_value;
        *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
        let mut limited = context.clone();
        limited.resources.max_allocation_units = Some(limit);
        let result = runtime.execute(&loaded, "next_input", &[], &limited);
        let succeeded = result.is_ok();
        if let Err(error) = result {
            assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 2);
        runtime
            .runtime()
            .gc()
            .array_set(id, 0, Value::I32(1))
            .unwrap();
        let next = runtime
            .execute(&loaded, "next_input", &[], &context)
            .unwrap()
            .return_value;
        match option(runtime.runtime().gc(), next) {
            Some(101) if !succeeded => failed_before = true,
            Some(102) if !succeeded => failed_after = true,
            Some(102) => finished = true,
            other => panic!("cut {limit}: {other:?}"),
        }
        runtime
            .runtime()
            .gc()
            .array_push(id, Value::I32(999))
            .unwrap();
        assert_eq!(
            runtime.runtime().gc().array_pop(id).unwrap(),
            Some(Value::I32(999))
        );
        if succeeded {
            break;
        }
    }
    assert!(failed_before && failed_after && finished);
}

#[cfg(feature = "native")]
#[test]
fn source_free_backend_fallback_uses_the_same_checked_state_driver() {
    use kagari_codegen_cranelift::CraneliftBackend;
    use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};
    let inputs = fixture_api::inputs::Inputs::default();
    let engine = engine(&inputs);
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let mut runtime = engine.runtime(context.clone());
    let program = prepared(ARTIFACT);
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let value = runtime
        .execute(&loaded, "factory", &[], &context)
        .unwrap()
        .return_value;
    *inputs.cursor.borrow_mut() = Some(runtime.runtime().root_value(value).unwrap());
    let native = runtime
        .prepare_native(
            &program,
            &loaded,
            "next_input",
            &mut CraneliftBackend::for_host().unwrap(),
            &Default::default(),
        )
        .unwrap();
    assert!(matches!(native, PreparedNativeEntry::Unsupported { .. }));
    for expected in [8, 9, 10] {
        runtime.runtime().collect_garbage().unwrap();
        let report = runtime
            .execute_prepared(&loaded, "next_input", &[], &context, &native)
            .unwrap();
        assert_eq!(
            report.jit.unwrap().status,
            JitExecutionStatus::InterpreterFallback
        );
        assert_eq!(
            option(runtime.runtime().gc(), report.return_value),
            Some(expected)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 1);
    }
}
