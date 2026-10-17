//! Storage lifecycle accounting and field throughput, measured in separate builds.
//! Run with --release; --features diagnostics selects allocation accounting only.
#[cfg(feature = "diagnostics")]
mod memory;
#[cfg(not(feature = "diagnostics"))]
mod revisions;

use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_source::source::SourceFile;
use std::time::Instant;

#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::allocations::Allocator;
#[cfg(not(feature = "diagnostics"))]
use kagari_runtime::native::value_handle::ScriptValue;
#[cfg(not(feature = "diagnostics"))]
use std::hint::black_box;

#[cfg(feature = "diagnostics")]
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

const SOURCE: &str = include_str!("workload.kgr");
const POPULATION: i32 = 512;
#[cfg(not(feature = "diagnostics"))]
const ITERATIONS: i32 = 50_000;

#[cfg(not(feature = "diagnostics"))]
fn sample(name: &str, operations: i32, mut action: impl FnMut() -> i32) {
    for _ in 0..3 {
        assert_eq!(black_box(action()), operations);
    }
    let mut times = [0; 11];
    for time in &mut times {
        let start = Instant::now();
        let result = black_box(action());
        *time = start.elapsed().as_nanos();
        assert_eq!(result, operations);
    }
    let samples = times;
    times.sort_unstable();
    println!(
        "execute,{name},operations={operations},median_ns={},min_ns={},max_ns={},samples=11,warmups=3,samples_ns={samples:?}",
        times[5], times[0], times[10]
    );
}

#[cfg(not(feature = "diagnostics"))]
fn lifecycle(engine: &KagariEngine, program: &PreparedProgram, context: &ExecutionContext) {
    for name in [
        "points",
        "nodes",
        "interfaces",
        "wide_interfaces",
        "generic_interfaces",
        "numbers",
    ] {
        let mut phases = [[0; 11]; 3];
        for sample in 0..14 {
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime.load_program(program, Default::default()).unwrap();
            let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
            let start = Instant::now();
            let values: ScriptValue = runtime
                .execute_typed(&loaded, name, (POPULATION,), context)
                .unwrap();
            let construction = start.elapsed().as_nanos();
            let start = Instant::now();
            runtime.runtime().collect_garbage().unwrap();
            let live_gc = start.elapsed().as_nanos();
            drop(values);
            let start = Instant::now();
            let empty = runtime.runtime().collect_garbage().unwrap();
            let release_gc = start.elapsed().as_nanos();
            assert_eq!(empty.live_objects, baseline);
            if sample >= 3 {
                for (phase, ns) in phases.iter_mut().zip([construction, live_gc, release_gc]) {
                    phase[sample - 3] = ns;
                }
            }
        }
        for (label, mut times) in ["construct", "live_gc", "release_gc"]
            .into_iter()
            .zip(phases)
        {
            let samples = times;
            times.sort_unstable();
            println!(
                "{label},{name},objects={POPULATION},median_ns={},min_ns={},max_ns={},samples=11,warmups=3,samples_ns={samples:?}",
                times[5], times[0], times[10]
            );
        }
    }
}

fn main() {
    // Explicit collections separate live storage, reserved capacity and release.
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = None;
    let engine = KagariEngine::new(config);
    let start = Instant::now();
    let artifact = engine
        .compile_to_artifact(SourceFile::new("storage.kgr", SOURCE), Default::default())
        .unwrap();
    let compiled = start.elapsed().as_nanos();
    let start = Instant::now();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    println!(
        "setup,compile_ns={compiled},prepare_ns={}",
        start.elapsed().as_nanos()
    );
    let context = ExecutionContext::default();

    #[cfg(feature = "diagnostics")]
    memory::run(&engine, &program, &context, POPULATION);

    #[cfg(not(feature = "diagnostics"))]
    {
        let mut runtime = engine.runtime(context.clone());
        let start = Instant::now();
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        println!("setup,link_ns={}", start.elapsed().as_nanos());
        let point: ScriptValue = runtime
            .execute_typed(&loaded, "point", (), &context)
            .unwrap();
        let floating: ScriptValue = runtime
            .execute_typed(&loaded, "floating", (), &context)
            .unwrap();
        let points: ScriptValue = runtime
            .execute_typed(&loaded, "points", (POPULATION,), &context)
            .unwrap();
        let pairs: ScriptValue = runtime
            .execute_typed(&loaded, "pairs", (POPULATION,), &context)
            .unwrap();
        let numbers: ScriptValue = runtime
            .execute_typed(&loaded, "numbers", (POPULATION,), &context)
            .unwrap();
        runtime.runtime().collect_garbage().unwrap();
        sample("scalar", ITERATIONS, || {
            runtime
                .execute_typed(&loaded, "scalar", (ITERATIONS,), &context)
                .unwrap()
        });
        for name in ["read_fields", "write_fields", "fields"] {
            sample(name, ITERATIONS, || {
                runtime
                    .execute_typed(&loaded, name, (point.clone(), ITERATIONS), &context)
                    .unwrap()
            });
        }
        sample("float_fields", ITERATIONS, || {
            let result: f64 = runtime
                .execute_typed(
                    &loaded,
                    "float_fields",
                    (floating.clone(), ITERATIONS),
                    &context,
                )
                .unwrap();
            assert_eq!(result, f64::from(ITERATIONS));
            result as i32
        });
        for (name, values) in [
            ("traverse", points),
            ("traverse_pairs", pairs),
            ("numeric_buffer", numbers),
        ] {
            sample(name, POPULATION, || {
                runtime
                    .execute_typed(&loaded, name, (values.clone(), POPULATION), &context)
                    .unwrap()
            });
        }
        drop(point);
        drop(floating);
        drop(loaded);
        drop(runtime);
        lifecycle(&engine, &program, &context);
        revisions::run(&engine, &context);
    }
}
