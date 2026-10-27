//! Bounded PG00 families missing from the original storage probe.
use crate::{
    native_layouts,
    observations::{self, Record},
    retained_reload,
};
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::native::value_handle::ScriptValue;
use kagari_source::source::SourceFile;
use std::{fmt::Write, time::Instant};

const NOMINAL_TYPES: i32 = 32;

struct Case {
    name: &'static str,
    entry: &'static str,
    check: &'static str,
    count: i32,
    expected: i32,
}

pub(super) fn prepare(engine: &KagariEngine, name: &str, source: &str) -> PreparedProgram {
    let start = Instant::now();
    let artifact = engine
        .compile_to_artifact(SourceFile::new(name, source), Default::default())
        .unwrap();
    let compiled = start.elapsed().as_nanos();
    let start = Instant::now();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    println!(
        "family_setup,{name},compile_ns={compiled},prepare_ns={}",
        start.elapsed().as_nanos()
    );
    prepared
}

fn source() -> String {
    let mut source = String::from(include_str!("workload.kgr"));
    source.push_str(include_str!("families.kgr"));
    // Distinct declarations, not many instances of one generic type.
    for index in 0..NOMINAL_TYPES {
        writeln!(source, "struct Sparse{index} {{ val x: i32, val y: i32 }}").unwrap();
        writeln!(
            source,
            "impl Read for Sparse{index} {{ fn read(self) -> i32 {{ self.x + self.y }} }}"
        )
        .unwrap();
    }
    source.push_str("pub fn sparse_types(n: i32) -> Vec<Read> { val v: Vec<Read> = Vec::new(); var i = 0; while i < n {\n");
    for index in 0..NOMINAL_TYPES {
        writeln!(
            source,
            "val p{index}: Read = Sparse{index} {{ x: 1, y: 2 }}; v.push(p{index});"
        )
        .unwrap();
    }
    source.push_str("i += 1; } v }\n");
    source
}

fn lifecycle(engine: &KagariEngine, program: &PreparedProgram, case: &Case) -> Record {
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let mut record = Record::default();
    let start = Instant::now();
    let loaded = runtime.load_program(program, Default::default()).unwrap();
    let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
    record.capture("setup", &runtime, start);
    let start = Instant::now();
    let values: ScriptValue = runtime
        .execute_typed(&loaded, case.entry, (case.count,), &context)
        .unwrap();
    record.capture("construct", &runtime, start);
    let start = Instant::now();
    runtime.runtime().collect_garbage().unwrap();
    record.capture("live_gc", &runtime, start);
    let start = Instant::now();
    let checked: i32 = runtime
        .execute_typed(&loaded, case.check, (values.clone(),), &context)
        .unwrap();
    assert_eq!(checked, case.expected);
    record.capture("verified", &runtime, start);
    drop(values);
    let start = Instant::now();
    let empty = runtime.runtime().collect_garbage().unwrap();
    record.capture("release_gc", &runtime, start);
    assert_eq!(empty.live_objects, baseline);
    record
}

fn sparse_cycles(engine: &KagariEngine, program: &PreparedProgram) -> Record {
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let mut record = Record::default();
    let start = Instant::now();
    let loaded = runtime.load_program(program, Default::default()).unwrap();
    let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
    record.capture("setup", &runtime, start);
    let start = Instant::now();
    let dense: ScriptValue = runtime
        .execute_typed(&loaded, "nodes", (4096_i32,), &context)
        .unwrap();
    record.capture("construct", &runtime, start);
    let start = Instant::now();
    runtime.runtime().collect_garbage().unwrap();
    record.capture("dense_gc", &runtime, start);
    let kept: ScriptValue = runtime
        .execute_typed(&loaded, "sparse_nodes", (dense.clone(),), &context)
        .unwrap();
    drop(dense);
    let start = Instant::now();
    let sparse = runtime.runtime().collect_garbage().unwrap().live_objects;
    record.capture("sparsify_gc", &runtime, start);
    assert_eq!(
        sparse,
        baseline + 129,
        "64 Nodes, their 64 edges Vecs, and retained Vec"
    );
    for phase in ["churn_gc_0", "churn_gc_1", "churn_gc_2"] {
        let dead: ScriptValue = runtime
            .execute_typed(&loaded, "nodes", (4096_i32,), &context)
            .unwrap();
        drop(dead);
        let start = Instant::now();
        assert_eq!(
            runtime.runtime().collect_garbage().unwrap().live_objects,
            sparse
        );
        record.capture(phase, &runtime, start);
        let checked: i32 = runtime
            .execute_typed(&loaded, "check_nodes", (kept.clone(),), &context)
            .unwrap();
        assert_eq!(checked, 64);
    }
    let start = Instant::now();
    runtime.runtime().collect_garbage().unwrap();
    record.capture("sparse_live_gc", &runtime, start);
    drop(kept);
    let start = Instant::now();
    assert_eq!(
        runtime.runtime().collect_garbage().unwrap().live_objects,
        baseline
    );
    record.capture("release_gc", &runtime, start);
    record
}

pub(super) fn run() {
    let engine = native_layouts::engine();
    for (name, size, align) in native_layouts::layouts() {
        println!("native_layout,{name},size={size},align={align}");
    }
    let program = prepare(&engine, "storage_families.kgr", &source());
    for case in [
        Case {
            name: "sparse_types_32x4",
            entry: "sparse_types",
            check: "check_sparse_types",
            count: 4,
            expected: 128,
        },
        Case {
            name: "mixed_native_128x6",
            entry: "mixed_native",
            check: "check_mixed",
            count: 128,
            expected: 128,
        },
        Case {
            name: "scalar_65536",
            entry: "numbers",
            check: "check_numbers",
            count: 65_536,
            expected: 65_536,
        },
        Case {
            name: "references_8192",
            entry: "points",
            check: "check_points",
            count: 8192,
            expected: 8192,
        },
    ] {
        observations::run(case.name, || lifecycle(&engine, &program, &case));
    }
    observations::run("sparse_cycles_4096_keep64", || {
        sparse_cycles(&engine, &program)
    });
    retained_reload::run(&engine);
    script_keys(&engine);
}

fn script_keys(engine: &KagariEngine) {
    // Keep this product separate so the earlier families retain their setup contract.
    let program = prepare(
        engine,
        "storage_script_keys.kgr",
        include_str!("script_keys.kgr"),
    );
    let case = Case {
        name: "script_keys_256",
        entry: "script_keys",
        check: "check_script_keys",
        count: 256,
        expected: 256,
    };
    observations::run(case.name, || lifecycle(engine, &program, &case));
}

pub(super) fn script_keys_only() {
    script_keys(&native_layouts::engine());
}
