//! Enum allocation baseline; the design document owns workloads and interpretation.
//! Run in release with source and kagari-runtime/execution-diagnostics enabled.
use kagari_stdlib as stdlib;
mod model;

use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::Program,
};
use kagari_gc::heap::HeapStats;
use kagari_runtime::{
    diagnostics::allocations::{self, Allocator, Counts},
    module::entry::ProgramEntry,
    native::value_handle::ScriptValue,
    value::{
        carriers::{RootedValue, StoredValue},
        view::ValueView,
    },
};
use kagari_source::source::SourceFile;
use std::{env, hint::black_box, mem::size_of, time::Instant};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

const SOURCE: &str = include_str!("../../tests/fixtures/measurements/enum-layout-baseline.kgr");

const CASES: &[(&str, bool)] = &[
    ("scalar", false),
    ("object_alias", false),
    ("option_some", false),
    ("option_none", false),
    ("result_ok", false),
    ("result_err_capture", false),
    ("result_err_alias", false),
    ("result_err_forward", false),
    ("object_option", false),
    ("user_enum", false),
    ("nested_option", false),
    ("scalar_storage", true),
    ("object_storage", true),
    ("option_storage", true),
    ("result_storage", true),
    ("error_storage", true),
];

struct Sample {
    elapsed: u128,
    counts: Counts,
    live_objects_delta: i128,
    collections: u64,
    live: HeapStats,
    retired: HeapStats,
    collect_ns: u128,
    live_bytes: i128,
}

fn main() {
    let mut args = env::args().skip(1);
    let first = args.next();
    if first.as_deref() == Some("model") {
        model::run();
        return;
    }
    let count: u64 = first.as_deref().unwrap_or("512").parse().unwrap();
    let samples: usize = args.next().as_deref().unwrap_or("5").parse().unwrap();
    let automatic = args.next().as_deref().unwrap_or("off") == "on";
    assert!(count > 0 && samples > 0);
    let mut config = EngineConfig::default();
    if !automatic {
        config.default_runtime.gc.collection_threshold = None;
    }
    let start = Instant::now();
    let engine = KagariEngine::with_native_modules(config, stdlib::modules().unwrap()).unwrap();
    let engine_setup_us = start.elapsed().as_micros();
    let start = Instant::now();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("enum-layout-baseline.kgr", SOURCE),
            Default::default(),
        )
        .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    println!(
        "prepare_us={} engine_setup_us={engine_setup_us} stored_value_bytes={} host_value_bytes={} view_bytes={} count={count} samples={samples} automatic_gc={automatic}",
        start.elapsed().as_micros(),
        size_of::<StoredValue>(),
        size_of::<RootedValue>(),
        size_of::<ValueView<'_>>(),
    );
    let mut load_times = Vec::with_capacity(samples);
    for sample in 0..=samples {
        let start = Instant::now();
        let mut runtime = engine.runtime(ExecutionContext::default());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let elapsed = start.elapsed().as_nanos();
        black_box((&runtime, &loaded));
        if sample > 0 {
            load_times.push(elapsed);
        }
    }
    load_times.sort_unstable();
    println!(
        "fresh_runtime_load_ns={} samples={samples}",
        load_times[samples / 2]
    );
    for &(name, stored) in CASES {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let owner = runtime.select_program(&loaded).unwrap();
        let entry = ProgramEntry::Export(name);
        let length = stored.then(|| {
            runtime
                .bind_function::<(ScriptValue,), u64>(&loaded.root(), &format!("{name}_len"))
                .unwrap()
        });
        // Warm code/type/root capacities using the measured workload, then collect.
        for _ in 0..2 {
            drop(
                runtime
                    .execute_typed::<_, ScriptValue>(&loaded, entry, (count,), &context)
                    .unwrap(),
            );
            runtime.collect_garbage().unwrap();
        }
        let baseline_roots = runtime.gc_stats().root_registrations;
        let mut times = Vec::with_capacity(samples);
        for sample in 0..samples {
            let before = runtime.gc_stats();
            let (measurement, total) = allocations::measure(|| {
                let start = Instant::now();
                let result = runtime
                    .execute_typed::<_, ScriptValue>(&loaded, entry, (count,), &context)
                    .unwrap();
                let elapsed = start.elapsed().as_nanos();
                let counts = allocations::current().unwrap();
                let after = runtime.gc_stats();
                // This is retained occupancy, not a cumulative allocation count.
                let live_objects_delta = after.objects as i128 - before.objects as i128;
                runtime
                    .with_context(&owner, &context, |cx| {
                        if stored {
                            let actual = length.as_ref().unwrap().call(cx, (result.clone(),))?;
                            assert_eq!(actual, count, "{name}");
                            black_box(&result);
                        } else {
                            let value: u64 = result.decode(cx)?;
                            assert_eq!(value, count * (count - 1) / 2, "{name}");
                            black_box(value);
                        }
                        Ok(())
                    })
                    .unwrap();
                let start = Instant::now();
                runtime.collect_garbage().unwrap();
                let collect_ns = start.elapsed().as_nanos();
                let live = runtime.gc_stats();
                let live_bytes = allocations::current().unwrap().net_bytes;
                drop(result);
                runtime.collect_garbage().unwrap();
                let retired = runtime.gc_stats();
                Sample {
                    elapsed,
                    counts,
                    live_objects_delta,
                    collections: after.completed_cycles - before.completed_cycles,
                    live,
                    retired,
                    collect_ns,
                    live_bytes,
                }
            });
            let Sample {
                elapsed,
                counts,
                live_objects_delta,
                collections,
                live,
                retired,
                collect_ns,
                live_bytes,
            } = measurement;
            assert_eq!(
                runtime.gc_stats().root_registrations,
                baseline_roots,
                "{name}"
            );
            times.push(elapsed);
            println!(
                "case={name} sample={sample} exec_ns={elapsed} alloc_requests={} requested_bytes={} net_requested_bytes={} live_objects_delta={live_objects_delta} auto_collections={collections} ledger_with_result={} retired_ledger={} live_units={} retired_units={} explicit_live_collect_ns={collect_ns} live_requested_delta={live_bytes} retired_requested_delta={}",
                counts.requests,
                counts.requested_bytes,
                counts.net_bytes,
                live.indirect_bytes,
                retired.indirect_bytes,
                live.objects as u128 + live.provider_units,
                retired.objects as u128 + retired.provider_units,
                total.net_bytes,
            );
        }
        times.sort_unstable();
        println!("median case={name} exec_ns={}", times[times.len() / 2]);
    }
}
