//! Automatic activity on the existing families, separate from frozen explicit-GC modes.
mod pressure;
mod report;

use crate::{
    families::{self, Case},
    native_layouts, retained_reload,
};
use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::native::value_handle::ScriptValue;
use report::{Record, Scope};
use std::array;

fn lifecycle(engine: &KagariEngine, program: &PreparedProgram, case: &Case) -> Record {
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(program, Default::default()).unwrap();
    let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
    let mut record = Record::new();
    let values: ScriptValue =
        record.measure("construct", Scope::Automatic, &mut runtime, |runtime| {
            runtime
                .execute_typed(&loaded, case.entry, (case.count,), &context)
                .unwrap()
        });
    record.measure("live_gc", Scope::Explicit, &mut runtime, |runtime| {
        runtime.runtime().collect_garbage().unwrap();
    });
    record.measure("access", Scope::Automatic, &mut runtime, |runtime| {
        let checked: i32 = runtime
            .execute_typed(&loaded, case.check, (values.clone(),), &context)
            .unwrap();
        assert_eq!(checked, case.expected);
    });
    record.measure(
        "release_boundary",
        Scope::Automatic,
        &mut runtime,
        |runtime| {
            drop(values);
            runtime.runtime().gc_safepoint().unwrap();
        },
    );
    record.measure("release_gc", Scope::Explicit, &mut runtime, |runtime| {
        assert_eq!(
            runtime.runtime().collect_garbage().unwrap().live_objects,
            baseline
        );
    });
    record.finish(&runtime);
    record
}

fn sparse_cycles(engine: &KagariEngine, program: &PreparedProgram) -> Record {
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(program, Default::default()).unwrap();
    let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
    let mut record = Record::new();
    let dense: ScriptValue =
        record.measure("construct", Scope::Automatic, &mut runtime, |runtime| {
            runtime
                .execute_typed(&loaded, "nodes", (4096_i32,), &context)
                .unwrap()
        });
    record.measure("dense_gc", Scope::Explicit, &mut runtime, |runtime| {
        runtime.runtime().collect_garbage().unwrap();
    });
    let kept: ScriptValue = record.measure("sparsify", Scope::Automatic, &mut runtime, |runtime| {
        let kept: ScriptValue = runtime
            .execute_typed(&loaded, "sparse_nodes", (dense.clone(),), &context)
            .unwrap();
        drop(dense);
        runtime.runtime().gc_safepoint().unwrap();
        kept
    });
    let sparse = record.measure("sparsify_gc", Scope::Explicit, &mut runtime, |runtime| {
        runtime.runtime().collect_garbage().unwrap().live_objects
    });
    assert_eq!(
        sparse,
        baseline + 129,
        "64 Nodes, their 64 edges Vecs, and retained Vec"
    );
    for (construct, collect, access) in [
        ("churn_construct_0", "churn_gc_0", "churn_access_0"),
        ("churn_construct_1", "churn_gc_1", "churn_access_1"),
        ("churn_construct_2", "churn_gc_2", "churn_access_2"),
    ] {
        record.measure(construct, Scope::Automatic, &mut runtime, |runtime| {
            let dead: ScriptValue = runtime
                .execute_typed(&loaded, "nodes", (4096_i32,), &context)
                .unwrap();
            drop(dead);
            runtime.runtime().gc_safepoint().unwrap();
        });
        record.measure(collect, Scope::Explicit, &mut runtime, |runtime| {
            assert_eq!(
                runtime.runtime().collect_garbage().unwrap().live_objects,
                sparse
            );
        });
        record.measure(access, Scope::Automatic, &mut runtime, |runtime| {
            let checked: i32 = runtime
                .execute_typed(&loaded, "check_nodes", (kept.clone(),), &context)
                .unwrap();
            assert_eq!(checked, 64);
        });
    }
    record.measure("sparse_live_gc", Scope::Explicit, &mut runtime, |runtime| {
        runtime.runtime().collect_garbage().unwrap();
    });
    record.measure(
        "release_boundary",
        Scope::Automatic,
        &mut runtime,
        |runtime| {
            drop(kept);
            runtime.runtime().gc_safepoint().unwrap();
        },
    );
    record.measure("release_gc", Scope::Explicit, &mut runtime, |runtime| {
        assert_eq!(
            runtime.runtime().collect_garbage().unwrap().live_objects,
            baseline
        );
    });
    record.finish(&runtime);
    record
}

fn reload(engine: &KagariEngine, programs: &[PreparedProgram; 4]) -> Record {
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&programs[0], Default::default())
        .unwrap();
    let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
    let mut record = Record::new();
    let mut closures: [Option<ScriptValue>; 4] = array::from_fn(|_| None);
    let mut keys = Vec::with_capacity(4);
    record.measure(
        "construct_reload",
        Scope::Automatic,
        &mut runtime,
        |runtime| {
            for (index, program) in programs.iter().enumerate() {
                if index != 0 {
                    runtime
                        .reload_program(&loaded, program, Default::default())
                        .unwrap();
                }
                keys.push(runtime.runtime().select_program(&loaded).unwrap().key());
                closures[index] = Some(
                    runtime
                        .execute_typed(&loaded, "make", (256_i32,), &context)
                        .unwrap(),
                );
            }
        },
    );
    record.measure("retained_gc", Scope::Explicit, &mut runtime, |runtime| {
        runtime.runtime().collect_garbage().unwrap();
    });
    record.measure("access", Scope::Automatic, &mut runtime, |runtime| {
        for (index, callback) in closures.iter().enumerate() {
            assert!(runtime.runtime().modules().loaded(keys[index]).is_some());
            let result: i32 = runtime
                .execute_typed(
                    &loaded,
                    "invoke",
                    (callback.as_ref().unwrap().clone(),),
                    &context,
                )
                .unwrap();
            assert_eq!(result, 256 + 1 + index as i32 * 10 + 3);
        }
    });
    record.measure(
        "release_old_boundary",
        Scope::Automatic,
        &mut runtime,
        |runtime| {
            for callback in &mut closures[..3] {
                *callback = None;
            }
            runtime.runtime().gc_safepoint().unwrap();
        },
    );
    record.measure("release_old_gc", Scope::Explicit, &mut runtime, |runtime| {
        runtime.runtime().collect_garbage().unwrap();
    });
    for key in &keys[..3] {
        assert!(runtime.runtime().modules().loaded(*key).is_none());
    }
    assert!(runtime.runtime().modules().loaded(keys[3]).is_some());
    record.measure(
        "release_all_boundary",
        Scope::Automatic,
        &mut runtime,
        |runtime| {
            drop(closures);
            runtime.runtime().gc_safepoint().unwrap();
        },
    );
    record.measure("release_all_gc", Scope::Explicit, &mut runtime, |runtime| {
        assert_eq!(
            runtime.runtime().collect_garbage().unwrap().live_objects,
            baseline
        );
    });
    record.finish(&runtime);
    record
}

pub(super) fn run(check_only: bool, pressure: bool) {
    let mut config = EngineConfig::default();
    let policy = if pressure {
        // Fixed before measurement, not tuned to a failed pause or a workload result.
        config.default_runtime.gc.collection_threshold = Some(64);
        config.default_runtime.gc.retained_byte_threshold = 4096;
        "pressure_64_4096"
    } else {
        "default"
    };
    let gc = config.default_runtime.gc;
    println!(
        "auto_config,{policy},mode={},unit_threshold={},byte_threshold={},counting_allocator=false,observation_api=ordinary,observation_start_excluded=true,raw_pauses_include_indivisible=true",
        if check_only { "check" } else { "measurement" },
        gc.collection_threshold.unwrap(),
        gc.retained_byte_threshold,
    );
    let engine = native_layouts::configured_engine(config);
    let program = families::prepare(&engine, "storage_families.kgr", &families::source());
    for case in families::CASES {
        report::run(policy, case.name, check_only, pressure, || {
            lifecycle(&engine, &program, &case)
        });
    }
    report::run(
        policy,
        "sparse_cycles_4096_keep64",
        check_only,
        pressure,
        || sparse_cycles(&engine, &program),
    );
    let programs = retained_reload::programs(&engine);
    report::run(
        policy,
        "retained_reload_4x256",
        check_only,
        pressure,
        || reload(&engine, &programs),
    );
    let program = families::prepare(
        &engine,
        "storage_script_keys.kgr",
        include_str!("script_keys.kgr"),
    );
    report::run(
        policy,
        families::SCRIPT_KEYS.name,
        check_only,
        pressure,
        || lifecycle(&engine, &program, &families::SCRIPT_KEYS),
    );
    report::run(policy, pressure::NAME, check_only, true, || {
        pressure::lifecycle(&engine)
    });
}
