//! Reproducible standard-library compilation, execution and logical-charge baseline.
//! Build with the workspace dev profile, then run target/debug/examples/stdlib_baseline.
use kagari_common::source::SourceFile;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;
use std::{hint::black_box, time::Instant};

const WORKLOADS: &[(&str, &str)] = &[
    (
        "direct",
        r#"fn main() -> i32 {
            val text = " Kagari ".trim().to_ascii_lowercase();
            std::debug::assert(text == "kagari", "text");
            std::math::abs(-42)
        }"#,
    ),
    (
        "option_callback",
        "fn main() -> i32 { Some(20).map(|n| n + 22).unwrap_or(0) }",
    ),
    (
        "lazy_pipeline",
        "fn main() -> i32 { [1, 10, 11, 99].iter().map(|n| n * 2).filter(|n| n > 2).take(2usize).fold(0, |a, b| a + b) }",
    ),
    (
        "prepared_sort",
        "fn main() -> i32 { val a = [22, 1, 20]; a.sort_by(|a, b| a.cmp(b)); a.retain(|n| n > 1); a[0usize] + a[1usize] }",
    ),
];

fn main() {
    for (name, text) in WORKLOADS {
        let mut compile_ns = Vec::new();
        for sample in 0..22 {
            let start = Instant::now();
            let artifact = KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new(format!("memory://stdlib-baseline/{name}.kgr"), *text),
                    Default::default(),
                    Default::default(),
                )
                .unwrap();
            black_box(artifact);
            if sample != 0 {
                compile_ns.push(start.elapsed().as_nanos());
            }
        }
        compile_ns.sort_unstable();
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(format!("memory://stdlib-baseline/{name}.kgr"), *text),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let encoded_bytes = artifact.to_bytes().unwrap().len();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let mut execution_ns = Vec::new();
        let mut expected_steps = None;
        for sample in 0..102 {
            let before = runtime.runtime().resources().counters().instruction_steps;
            let start = Instant::now();
            let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(report.return_value, Value::I32(42));
            let steps = runtime.runtime().resources().counters().instruction_steps - before;
            assert_eq!(*expected_steps.get_or_insert(steps), steps);
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            if sample != 0 {
                execution_ns.push(elapsed);
            }
        }
        execution_ns.sort_unstable();
        println!(
            "{name}: compile_median_ns={} execution_median_ns={} logical_steps={} artifact_bytes={encoded_bytes}",
            compile_ns[10],
            execution_ns[50],
            expected_steps.unwrap(),
        );
    }
}
