//! Duplicate-heavy sort probe; setup, construction and sorting are separate.
//! Timings use ordinary builds; diagnostic runs attribute library and runtime work.
#[cfg(feature = "diagnostics")]
use kagari::runtime::Runtime as HostRuntime;
use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::native::value_handle::ScriptValue;
#[cfg(feature = "diagnostics")]
use kagari_runtime::{
    diagnostics::{self, allocations},
    error::RuntimeError,
    module::Program,
    native::typed::NativeContext,
};
use kagari_source::source::SourceFile;
#[cfg(feature = "diagnostics")]
use kagari_stdlib::collections::{diagnostics as sort_diagnostics, vector::ScriptVec};
#[cfg(not(feature = "diagnostics"))]
use std::hint::black_box;
#[cfg(feature = "diagnostics")]
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

const ELEMENTS: i32 = 512;
const WARMUPS: usize = 3;
const SAMPLES: usize = 11;

fn report(name: &str, phase: &str, samples: [u128; SAMPLES]) {
    let mut ordered = samples;
    ordered.sort_unstable();
    println!(
        "sort,{name},phase={phase},elements={ELEMENTS},median_ns={},min_ns={},max_ns={},samples={SAMPLES},warmups={WARMUPS},samples_ns={samples:?}",
        ordered[SAMPLES / 2],
        ordered[0],
        ordered[SAMPLES - 1]
    );
}

pub(super) fn run(check_only: bool) {
    assert!(
        check_only || !cfg!(feature = "diagnostics"),
        "sort timing requires the ordinary build without diagnostics"
    );
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = if cfg!(feature = "diagnostics") {
        Some(1024)
    } else {
        None
    };
    let engine = KagariEngine::new(config);
    let start = (!check_only).then(Instant::now);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("sort.kgr", include_str!("sort.kgr")),
            Default::default(),
        )
        .unwrap();
    let compiled = start.map(|start| start.elapsed().as_nanos());
    let start = (!check_only).then(Instant::now);
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    if let Some(start) = start {
        println!(
            "sort_setup,compile_ns={},prepare_ns={}",
            compiled.unwrap(),
            start.elapsed().as_nanos()
        );
    }
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let start = (!check_only).then(Instant::now);
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    if let Some(start) = start {
        println!("sort_setup,link_ns={}", start.elapsed().as_nanos());
    }
    let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
    let mut script_calls = None;
    for (name, input, entry, check) in [
        (
            "scalar_default",
            "scalar_input",
            "scalar_sort",
            "check_scalar",
        ),
        ("cheap_script", "script_input", "cheap_sort", "check_script"),
        (
            "allocating_script",
            "script_input",
            "allocating_sort",
            "check_script",
        ),
    ] {
        let mut construction = [0; SAMPLES];
        let mut sorting = [0; SAMPLES];
        let mut expected_calls = None;
        // Check mode exercises repeatability without opening any timing windows.
        let repetitions = if check_only { 2 } else { WARMUPS + SAMPLES };
        for sample in 0..repetitions {
            let start = (!check_only).then(Instant::now);
            let values: ScriptValue = runtime
                .execute_typed(&loaded, input, (ELEMENTS,), &context)
                .unwrap();
            let construct_ns = start.map(|start| start.elapsed().as_nanos());
            runtime.runtime().collect_garbage().unwrap();
            // Root cloning and input construction are outside the sort window.
            let argument = values.clone();
            #[cfg(feature = "diagnostics")]
            let before_sort = runtime.runtime().gc().stats();
            let start = (!check_only).then(Instant::now);
            #[cfg(feature = "diagnostics")]
            let (((calls, work), execution), allocation) = allocations::measure(|| {
                diagnostics::measure(|| {
                    sort_diagnostics::measure(|| {
                        runtime
                            .execute_typed::<_, i32>(&loaded, entry, (argument,), &context)
                            .unwrap()
                    })
                })
            });
            #[cfg(feature = "diagnostics")]
            println!(
                "sort_work,{name},sample={sample},work={work:?},execution={execution:?},allocation={allocation:?},collections={},heap_units_after={}",
                runtime.runtime().gc().stats().collections - before_sort.collections,
                runtime.runtime().gc().stats().current_heap_units
            );
            #[cfg(not(feature = "diagnostics"))]
            let calls: i32 = black_box(
                runtime
                    .execute_typed(&loaded, entry, (argument,), &context)
                    .unwrap(),
            );
            let sort_ns = start.map(|start| start.elapsed().as_nanos());
            let checked: i32 = runtime
                .execute_typed(&loaded, check, (values.clone(), ELEMENTS, calls), &context)
                .unwrap();
            assert_eq!(
                checked, ELEMENTS,
                "{name}: exact stable output and callback effects"
            );
            assert_eq!(
                *expected_calls.get_or_insert(calls),
                calls,
                "{name}: deterministic callback count"
            );
            if name != "scalar_default" {
                assert_eq!(
                    *script_calls.get_or_insert(calls),
                    calls,
                    "same ordering decisions for both script comparators"
                );
            }
            drop(values);
            assert_eq!(
                runtime.runtime().collect_garbage().unwrap().live_objects,
                baseline,
                "{name}: release all inputs and callback allocations"
            );
            if !check_only && sample >= WARMUPS {
                construction[sample - WARMUPS] = construct_ns.unwrap();
                sorting[sample - WARMUPS] = sort_ns.unwrap();
            }
        }
        println!(
            "sort_check,{name},elements={ELEMENTS},callbacks={},repetitions={repetitions},stable_output=passed,release=passed",
            expected_calls.unwrap()
        );
        if !check_only {
            report(name, "construct", construction);
            report(name, "sort", sorting);
        }
    }
    #[cfg(feature = "diagnostics")]
    {
        recovery_probe(&runtime, &loaded, &context);
        assert_eq!(
            runtime.runtime().collect_garbage().unwrap().live_objects,
            baseline
        );
    }
    println!(
        "sort_limits,diagnostics={},automatic_gc={},gc_step_delay=not_incremental,algorithm_copies=diagnostics_only,scalar_internal_moves=unavailable,scalar_leaf_stack_bound_bytes=4096,gc_moves=zero_nonmoving_heap,barrier_enqueue=not_implemented_synchronous_pg03,input=duplicate_heavy_32_keys",
        cfg!(feature = "diagnostics"),
        if cfg!(feature = "diagnostics") {
            "enabled"
        } else {
            "disabled"
        }
    );
}

#[cfg(feature = "diagnostics")]
fn recovery_probe(runtime: &HostRuntime, loaded: &Program, context: &ExecutionContext) {
    for unwind in [false, true] {
        let input: ScriptValue = runtime
            .execute_typed(loaded, "scalar_input", (ELEMENTS,), context)
            .unwrap();
        let owner = runtime.runtime().select_program(loaded).unwrap();
        let mut cx = NativeContext::new(runtime.runtime(), &owner).unwrap();
        let vector: ScriptVec<i32> = input.decode(&mut cx).unwrap();
        runtime.runtime().collect_garbage().unwrap();
        let before = runtime.runtime().gc().stats();
        let mut calls = 0;
        let start = Instant::now();
        let (((result, work), execution), allocation) = allocations::measure(|| {
            diagnostics::measure(|| {
                sort_diagnostics::measure(|| {
                    catch_unwind(AssertUnwindSafe(|| {
                        vector.sort_by(&mut cx, |cx, left, right| {
                            calls += 1;
                            cx.collect_garbage()?;
                            if calls == 17 {
                                if unwind {
                                    panic!("intentional sort recovery probe");
                                }
                                return Err(RuntimeError::module_validation(
                                    "intentional sort recovery probe",
                                ));
                            }
                            Ok(left.cmp(&right))
                        })
                    }))
                })
            })
        });
        let elapsed = start.elapsed().as_nanos();
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        assert_eq!(calls, 17);
        let mut values = Vec::new();
        vector
            .for_each(&mut cx, |_, value| {
                values.push(value);
                Ok(())
            })
            .unwrap();
        values.sort_unstable();
        assert_eq!(
            values,
            (0..ELEMENTS)
                .map(|i| i / (ELEMENTS / 32))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            runtime.runtime().gc().stats().current_heap_units,
            before.current_heap_units
        );
        assert!(work.recovery_copies > 0);
        println!(
            "sort_recovery,unwind={unwind},calls={calls},elapsed_instrumented_ns={elapsed},work={work:?},execution={execution:?},allocation={allocation:?},multiset=passed,accounting=passed"
        );
    }
}
