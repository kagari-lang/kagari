//! Manual construction attribution; frozen workload setup remains the acceptance owner.
//! Build with --release and run paired baseline/current binaries without concurrent work.
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{Runtime, native::installation::NativeInstallation};
use kagari_source::source::SourceFile;
use kagari_stdlib::modules;
use std::{env, hint::black_box, time::Instant};

const BATCH: usize = 128;
const SAMPLES: usize = 11;

fn measure_compiled<T>(name: &str, mut create: impl FnMut(&KagariEngine) -> T) {
    let source = include_str!("../../../benchmarks/lua-comparison/workloads/calls.kgr")
        .replace("__N__", "10000");
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("benchmark_calls.kgr", source),
            Default::default(),
        )
        .unwrap();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let start = Instant::now();
    let runtime = black_box(create(&engine));
    let elapsed = start.elapsed().as_nanos();
    println!("initialization,{name},cold,1,0,{elapsed}");
    // Keep the verified product and runtime alive through the measured interval.
    drop(black_box(prepared));
    drop(runtime);
}

fn measure<T>(name: &str, mut create: impl FnMut() -> T) {
    let start = Instant::now();
    let first = black_box(create());
    let elapsed = start.elapsed();
    println!("initialization,{name},cold,1,0,{}", elapsed.as_nanos());
    drop(first);
    for _ in 0..3 {
        drop(black_box(create()));
    }
    for sample in 0..SAMPLES {
        let mut elapsed = 0;
        for _ in 0..BATCH {
            let start = Instant::now();
            let runtime = black_box(create());
            elapsed += start.elapsed().as_nanos();
            // Destruction stays outside each timed construction.
            drop(runtime);
        }
        println!("initialization,{name},warm,{BATCH},{sample},{elapsed}");
    }
}

fn main() {
    let mut reverse = false;
    let mut compiled = false;
    for argument in env::args().skip(1) {
        match argument.as_str() {
            "--reverse" if !reverse => reverse = true,
            "--compiled" if !compiled => compiled = true,
            _ => panic!("expected unique --reverse or --compiled"),
        }
    }
    println!("phase,route,state,batch,sample,ns");
    if compiled {
        let mut routes = ["compiled_bare", "compiled_sdk"];
        if reverse {
            routes.reverse();
        }
        let context = ExecutionContext::default();
        for route in routes {
            match route {
                "compiled_bare" => measure_compiled(route, |_| Runtime::default()),
                "compiled_sdk" => measure_compiled(route, |engine| engine.runtime(context.clone())),
                _ => unreachable!("fixed construction routes"),
            }
        }
        return;
    }
    let modules = modules().unwrap();
    let installation = NativeInstallation::prepare(&modules).unwrap();
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let mut routes = ["bare", "installed", "sdk"];
    if reverse {
        routes.reverse();
    }
    for route in routes {
        match route {
            "bare" => measure(route, Runtime::default),
            "installed" => measure(route, || installation.runtime(Default::default())),
            "sdk" => measure(route, || engine.runtime(context.clone())),
            _ => unreachable!("fixed construction routes"),
        }
    }
}
