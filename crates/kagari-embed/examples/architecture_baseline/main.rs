//! O1 architecture baseline; run with default source/native features.
//! `cargo run -p kagari-embed --example architecture_baseline`
//! Timings include the counting allocator's atomic bookkeeping overhead.
mod memory;

use std::{hint::black_box, time::Instant};

use kagari_bytecode::artifact::{ArtifactBuildOptions, KbcArtifact};
use kagari_codegen::{BackendFunctionInput, CodegenBackend};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_compiler::{native_links::build_native_links, source::lower::lower_to_mir};
use kagari_contract::native::NativeCompilationProduct;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_hir::analyze_source;
use kagari_mir::{ids::InstanceId, verify::verify_mir};
use kagari_runtime::{jit_abi::native_helper_symbols, value::Value};
use kagari_source::source::SourceFile;
use kagari_vm::vm::JitExecutionStatus;

use memory::{CountingAllocator, live_bytes};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
const SOURCE: &str = "fn main() -> i32 { 40 + 2 }";

fn sample(mut action: impl FnMut(), label: &str, count: usize) {
    action();
    let mut times = Vec::with_capacity(count);
    for _ in 0..count {
        let start = Instant::now();
        action();
        times.push(start.elapsed().as_nanos());
    }
    times.sort_unstable();
    println!(
        "{label}: median_ns={} min_ns={} max_ns={} samples={count}",
        times[count / 2],
        times[0],
        times[count - 1]
    );
}

fn prepare(bytes: &[u8]) -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn context() -> ExecutionContext {
    ExecutionContext::default()
}

fn sharing(bytes: &[u8], count: usize, shared: bool) {
    let engine = KagariEngine::default();
    let mut runtimes = Vec::with_capacity(count);
    let mut programs = Vec::with_capacity(count);
    let before = live_bytes();
    if shared {
        programs.push(prepare(bytes));
    }
    for _ in 0..count {
        if !shared {
            programs.push(prepare(bytes));
        }
        let program = programs.last().unwrap();
        let mut runtime = engine.runtime(context());
        let loaded = runtime.load_program(program, Default::default()).unwrap();
        runtimes.push((runtime, loaded));
    }
    let bytes = live_bytes() - before;
    for (index, (_, loaded)) in runtimes.iter().enumerate() {
        assert_eq!(
            loaded
                .verified_program()
                .same_version(runtimes[0].1.verified_program()),
            shared || index == 0
        );
    }
    println!("retained_rust_heap_bytes={bytes} runtimes={count} shared_preparation={shared}");
    black_box((&programs, &runtimes));
}

fn main() {
    let engine = KagariEngine::default();
    sample(
        || {
            black_box(
                KagariEngine::default()
                    .compile_to_artifact(
                        SourceFile::new("baseline.kgr", SOURCE),
                        Default::default(),
                    )
                    .unwrap(),
            );
        },
        "cold_engine_source_to_artifact",
        21,
    );
    let checked = analyze_source(&SourceFile::new("baseline.kgr", SOURCE))
        .into_codegen()
        .unwrap();
    let mir = lower_to_mir(&checked, &Default::default()).unwrap();
    let mut analysis_times = Vec::new();
    for _ in 0..101 {
        let raw = mir.clone().into_unverified();
        let start = Instant::now();
        let verified = verify_mir(raw, &Default::default()).unwrap();
        analysis_times.push(start.elapsed().as_nanos());
        black_box(verified);
    }
    analysis_times.sort_unstable();
    println!(
        "mir_verify_analysis_median_ns={} samples=101 clone_excluded=true",
        analysis_times[50]
    );
    let artifact = engine
        .compile_to_artifact(SourceFile::new("baseline.kgr", SOURCE), Default::default())
        .unwrap();
    let bytes = artifact.to_bytes().unwrap();
    let bytecode_only =
        KbcArtifact::from_program(artifact.program.clone(), ArtifactBuildOptions::default())
            .unwrap()
            .to_bytes()
            .unwrap();
    println!(
        "artifact_bytes={} bytecode_only_bytes={} portable_mir_payload_bytes={}",
        bytes.len(),
        bytecode_only.len(),
        artifact.portable_mir.as_ref().unwrap().bytes.len()
    );
    sample(
        || {
            black_box(prepare(&bytes));
        },
        "decode_and_native_verify",
        101,
    );
    let program = prepare(&bytes);
    sample(
        || {
            let mut runtime = engine.runtime(context());
            black_box(runtime.load_program(&program, Default::default()).unwrap());
        },
        "new_runtime_and_shared_program_link",
        101,
    );
    let links = build_native_links(&native_helper_symbols()).unwrap();
    let input = BackendFunctionInput::new(&mir, InstanceId::new(0), &links).unwrap();
    let mut backend = CraneliftBackend::for_host().unwrap();
    // Retain products outside timed calls so page freeing is not compile time.
    let mut products: Vec<NativeCompilationProduct> = Vec::with_capacity(22);
    sample(
        || {
            products.push(backend.compile_function(input).unwrap());
        },
        "native_compile",
        21,
    );
    drop(products);
    let context = context();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let before_native = live_bytes();
    let native = runtime
        .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
        .unwrap();
    let cache_and_install_bytes = live_bytes() - before_native;
    drop(native);
    let cache_bytes = live_bytes() - before_native;
    println!(
        "native_cache_rust_heap_bytes={cache_bytes} cache_plus_install_bytes={cache_and_install_bytes} executable_mappings_excluded=true"
    );
    sample(
        || {
            black_box(
                runtime
                    .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
                    .unwrap(),
            );
        },
        "cached_preparation_and_install",
        101,
    );
    let native = runtime
        .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
        .unwrap();
    for use_native in [false, true] {
        let mut checksum = 0;
        sample(
            || {
                let report = if use_native {
                    runtime
                        .execute_prepared(&loaded, "main", &[], &context, &native)
                        .unwrap()
                } else {
                    runtime.execute(&loaded, "main", &[], &context).unwrap()
                };
                if use_native {
                    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
                }
                assert_eq!(report.return_value, Value::I32(42));
                checksum += 42;
                black_box(report.return_value);
            },
            if use_native {
                "sdk_native_call"
            } else {
                "sdk_interpreter_call"
            },
            10_001,
        );
        assert_eq!(checksum, 42 * 10_002);
    }
    for count in [1, 8, 32] {
        sharing(&bytes, count, true);
        sharing(&bytes, count, false);
    }
}
