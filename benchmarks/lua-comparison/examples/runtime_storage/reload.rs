//! Separate preparation, publication and execution with old work absent/alive/collected.
use super::sample;
use kagari::{context::ExecutionContext, engine::KagariEngine};
use kagari_runtime::native::function_handle::Function;
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use std::time::Instant;

const SOURCE: &str = include_str!("reload.kgr");
const ITERATIONS: i32 = 5_000;

pub(super) fn run() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let start = Instant::now();
    let original = engine
        .compile_to_program(SourceFile::new("reload.kgr", SOURCE), Default::default())
        .unwrap();
    let original_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    let replacement = SOURCE
        .replace("value + 1", "value + 2")
        .replace("total + 10", "total + 100");
    assert_eq!(SOURCE.matches("value + 1").count(), 1);
    let candidate = engine
        .compile_to_program(
            SourceFile::new("reload.kgr", replacement),
            Default::default(),
        )
        .unwrap();
    let candidate_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    let candidate = original
        .prepare_replacement(&candidate, &Default::default())
        .unwrap();
    let compatibility_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    let mut fresh = engine.runtime(context.clone());
    let fresh_program = fresh.load_program(&candidate, Default::default()).unwrap();
    let fresh_load_ns = start.elapsed().as_nanos();
    let expected = ITERATIONS * (ITERATIONS + 3) / 2;
    sample("reload_current_fresh", ITERATIONS, || {
        assert_eq!(
            fresh
                .execute_typed::<_, i32>(&fresh_program, "current", (ITERATIONS,), &context)
                .unwrap(),
            expected
        );
        ITERATIONS
    });
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&original, Default::default()).unwrap();
    let old_key = runtime.select_program(&loaded).unwrap().key();
    runtime.collect_garbage().unwrap();
    let roots_before = runtime.gc_stats().root_registrations;
    let saved: Function<(i32,), i32> = runtime
        .execute_typed(&loaded, "saved", (), &context)
        .unwrap();
    sample("reload_retained_before", ITERATIONS, || {
        assert_eq!(
            runtime.call(&saved, (ITERATIONS,)).unwrap(),
            ITERATIONS * (ITERATIONS + 1) / 2 + 10
        );
        ITERATIONS
    });
    drop(original);
    let start = Instant::now();
    runtime
        .replace_program(&loaded, &candidate, Default::default())
        .unwrap();
    let publication_ns = start.elapsed().as_nanos();
    drop(candidate);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old_key).is_some());
    let alive_records = runtime.modules().loaded_count();
    sample("reload_current_old_alive", ITERATIONS, || {
        assert_eq!(
            runtime
                .execute_typed::<_, i32>(&loaded, "current", (ITERATIONS,), &context)
                .unwrap(),
            expected
        );
        ITERATIONS
    });
    sample("reload_retained_after", ITERATIONS, || {
        // The selected old lambda keeps +10; its ordinary helper enters +2.
        assert_eq!(runtime.call(&saved, (ITERATIONS,)).unwrap(), expected + 10);
        ITERATIONS
    });
    drop(saved);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old_key).is_none());
    let collected_records = runtime.modules().loaded_count();
    sample("reload_current_old_collected", ITERATIONS, || {
        assert_eq!(
            runtime
                .execute_typed::<_, i32>(&loaded, "current", (ITERATIONS,), &context)
                .unwrap(),
            expected
        );
        ITERATIONS
    });
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
    println!(
        "reload_setup,original_prepare_ns={original_ns},candidate_prepare_ns={candidate_ns},compatibility_ns={compatibility_ns},fresh_load_ns={fresh_load_ns},publication_ns={publication_ns}"
    );
    println!("reload_records,alive={alive_records},collected={collected_records}");
}
