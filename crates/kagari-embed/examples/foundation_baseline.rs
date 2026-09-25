//! Reproducible foundation throughput and code-sharing baseline.
//! Run with `cargo run --release -p kagari-embed --example foundation_baseline`.
use std::{hint::black_box, sync::Arc, time::Instant};

use kagari_common::{SourceFile, source_database::SourceLayer};
use kagari_embed::{ArtifactOptions, CompileOptions, KagariEngine};
use kagari_runtime::{Runtime, VerifiedProgram, value::Value};
use kagari_vm::Vm;

fn main() {
    let mut source = String::new();
    for index in 0..32 {
        source.push_str(&format!("fn f{index}() -> i32 {{ {index} }}\n"));
    }
    source.push_str("fn main() -> i32 { f31() }\n");

    let mut compile_samples = Vec::new();
    for _ in 0..5 {
        let engine = KagariEngine::default();
        let start = Instant::now();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("baseline.kgr", source.clone()),
                CompileOptions::default(),
                ArtifactOptions::default(),
            )
            .unwrap();
        black_box(artifact);
        compile_samples.push(start.elapsed().as_micros());
    }
    compile_samples.sort_unstable();
    println!(
        "compile_us={compile_samples:?} median_us={}",
        compile_samples[2]
    );

    let engine = KagariEngine::default();
    let file = engine
        .set_source("baseline.kgr", source.clone(), SourceLayer::Base)
        .unwrap();
    let first = engine
        .analyze(
            engine.source_snapshot(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let initial = &first.file(file).unwrap().result().facts().typed;
    assert_eq!(initial.checked_bodies, 33);
    let edited = source.replacen("fn f0() -> i32 { 0 }", "fn f0() -> i32 { 100 }", 1);
    engine
        .set_source("baseline.kgr", edited, SourceLayer::Overlay)
        .unwrap();
    let start = Instant::now();
    let second = engine
        .analyze(
            engine.source_snapshot(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let edit_us = start.elapsed().as_micros();
    let changed = &second.file(file).unwrap().result().facts().typed;
    assert_eq!((changed.checked_bodies, changed.reused_bodies), (1, 32));
    println!(
        "edit_analysis_us={edit_us} checked={} reused={}",
        changed.checked_bodies, changed.reused_bodies
    );

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("baseline.kgr", source),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .unwrap();
    let code_image_bytes = artifact.to_bytes().unwrap().len();
    let verified = VerifiedProgram::new(artifact.program).unwrap();
    let mut first_runtime = Runtime::default();
    let mut second_runtime = Runtime::default();
    let first_loaded = first_runtime
        .load_verified_program("baseline", verified.clone())
        .unwrap();
    let second_loaded = second_runtime
        .load_verified_program("baseline", verified.clone())
        .unwrap();
    assert!(Arc::ptr_eq(&first_loaded.bytecode, &second_loaded.bytecode));
    println!(
        "shared_code_image_bytes={code_image_bytes} module_arc_refs={}",
        Arc::strong_count(&verified.modules()[0])
    );

    let mut vm = Vm::new(first_runtime);
    const CALLS: u32 = 10_000;
    for _ in 0..100 {
        black_box(vm.execute(&first_loaded, "main").unwrap().return_value);
    }
    let start = Instant::now();
    let mut checksum = 0_i64;
    for _ in 0..CALLS {
        let Value::I32(value) = vm.execute(&first_loaded, "main").unwrap().return_value else {
            unreachable!("benchmark entry returns i32")
        };
        checksum += i64::from(value);
    }
    println!(
        "root_plus_one_internal_call_ns={} calls={CALLS} checksum={checksum}",
        start.elapsed().as_nanos() / u128::from(CALLS)
    );
    assert_eq!(checksum, i64::from(CALLS) * 31);
}
