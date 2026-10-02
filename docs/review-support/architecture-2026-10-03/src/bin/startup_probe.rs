//! Read-only baseline probe: default runtime construction and program linking.
//! Source compilation/preparation, handle drops, execution, and engine creation
//! are outside both timing intervals. A fresh runtime is used for every sample.
use kagari_common::source::SourceFile;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;
use std::{hint::black_box, time::Instant};

const SOURCE: &str = "fn main() -> i32 { 40 + 2 }";
const WARMUPS: usize = 1;
const SAMPLES: usize = 11;

fn report(label: &str, times: &mut [u128]) {
    times.sort_unstable();
    println!(
        "{label}: median_ns={} min_ns={} max_ns={} samples={}",
        times[times.len() / 2],
        times[0],
        times[times.len() - 1],
        times.len()
    );
}

fn main() {
    println!(
        "profile=dev_O1,features=embed_source_native,allocator=system,warmups={WARMUPS},samples={SAMPLES}"
    );
    println!(
        "scope=engine.runtime(default_context)_then_runtime.load_program(shared_prepared_program),fresh_runtime_per_sample=true,construction_and_link_timed_separately=true,engine_source_prepare_execute_and_all_drops_excluded=true"
    );
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(SourceFile::new("startup.kgr", SOURCE), Default::default())
        .unwrap();
    println!(
        "artifact_bytes={} portable_mir_bytes={} modules={}",
        artifact.to_bytes().unwrap().len(),
        artifact
            .portable_mir
            .as_ref()
            .map_or(0, |mir| mir.bytes.len()),
        artifact.program.modules.len()
    );
    for (index, module) in artifact.program.modules.iter().enumerate() {
        println!(
            "module_index={index} root={} identity={} functions={} instructions={} native_imports={} native_declarations={} public_items={} trait_contracts={} interfaces={} structures={} enumerations={} dependencies={}",
            index == artifact.program.root.index(),
            module.identity,
            module.functions.len(),
            module
                .functions
                .iter()
                .map(|function| function.instructions.len())
                .sum::<usize>(),
            module.native_imports.len(),
            module.native_declarations.len(),
            module.public_items.len(),
            module.trait_contracts.len(),
            module.interface_tables.len(),
            module.structures.len(),
            module.enumerations.len(),
            module.dependencies.len()
        );
    }
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut construction_times = Vec::with_capacity(SAMPLES);
    let mut link_times = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let context = ExecutionContext::default();
        let start = Instant::now();
        let mut runtime = black_box(engine.runtime(context.clone()));
        let construction_ns = start.elapsed().as_nanos();
        let start = Instant::now();
        let loaded = black_box(runtime.load_program(&program, Default::default()).unwrap());
        let link_ns = start.elapsed().as_nanos();
        assert!(program.bytecode().same_version(loaded.verified_program()));
        let result = runtime.execute(&loaded, "main", &[], &context).unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        if sample >= WARMUPS {
            construction_times.push(construction_ns);
            link_times.push(link_ns);
        }
        // Every result, loaded handle and runtime drop occurs after both timers.
        drop(result);
        drop(loaded);
        drop(runtime);
    }
    report("runtime_construction", &mut construction_times);
    report("shared_program_link", &mut link_times);
}
