//! Bounded same-product installation scaling; allocation and timing builds differ.
use kagari::{
    context::ExecutionContext,
    engine::{KagariEngine, source::PreparationOptions},
    program::Program,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use std::{collections::HashSet, env, fmt::Write, sync::Arc, time::Instant};

#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::allocations::{self, Counts};

struct Options {
    runtimes: usize,
    extra_functions: usize,
    fresh_domain: bool,
    mir: bool,
}

impl Options {
    fn parse() -> Self {
        let args: Vec<_> = env::args().skip(2).collect();
        assert_eq!(
            args.len(),
            4,
            "--sharing RUNTIMES EXTRA_FUNCTIONS shared|fresh bytecode|mir"
        );
        let runtimes = args[0].parse().expect("runtime count");
        let extra_functions = args[1].parse().expect("extra function count");
        assert!((1..=128).contains(&runtimes));
        assert!([0, 32, 128].contains(&extra_functions));
        let fresh_domain = match args[2].as_str() {
            "shared" => false,
            "fresh" => true,
            _ => panic!("expected shared|fresh"),
        };
        let mir = match args[3].as_str() {
            "bytecode" => false,
            "mir" => true,
            _ => panic!("expected bytecode|mir"),
        };
        Self {
            runtimes,
            extra_functions,
            fresh_domain,
            mir,
        }
    }

    fn source(&self) -> String {
        if self.extra_functions == 0 {
            return include_str!("sharing_control.kgr").into();
        }
        let mut source = include_str!("sharing.kgr").to_owned();
        // Generated scale witnesses are intentionally separate public functions.
        for index in 0..self.extra_functions {
            writeln!(
                source,
                "pub fn extra_{index}(value: i32) -> i32 {{ value + {index} }}"
            )
            .unwrap();
        }
        source
    }
}

#[cfg(feature = "diagnostics")]
type Snapshot = Counts;
#[cfg(not(feature = "diagnostics"))]
type Snapshot = u128;

fn snapshot(start: Instant) -> Snapshot {
    #[cfg(feature = "diagnostics")]
    {
        let _ = start;
        allocations::current().expect("allocation measurement")
    }
    #[cfg(not(feature = "diagnostics"))]
    {
        start.elapsed().as_nanos()
    }
}

struct Measurement {
    phases: [Snapshot; 5],
    domains: usize,
    root_backings: usize,
}

fn lifecycle(engine: &KagariEngine, program: &Program, options: &Options) -> Measurement {
    let context = ExecutionContext::default();
    let mut runtimes = Vec::with_capacity(options.runtimes);
    let mut installed = Vec::with_capacity(options.runtimes);
    let mut domains = HashSet::with_capacity(options.runtimes);
    let mut backings = HashSet::with_capacity(options.runtimes);
    let start = Instant::now();
    let initial = snapshot(start);
    for _ in 0..options.runtimes {
        let runtime = if options.fresh_domain {
            // The temporary engine is dropped before the empty-runtime snapshot.
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap()
                .runtime(context.clone())
        } else {
            engine.runtime(context.clone())
        };
        runtimes.push(runtime);
    }
    let empty = snapshot(start);
    for runtime in &mut runtimes {
        let loaded = runtime.load_program(program, Default::default()).unwrap();
        let selected = runtime.runtime().select_program(&loaded).unwrap();
        // Code keeps product IDs; measure the recipient identity domain independently.
        domains.insert(runtime.runtime().host().definitions().id());
        backings.insert(Arc::as_ptr(&selected.bytecode) as usize);
        installed.push(loaded);
    }
    let loaded = snapshot(start);
    for (runtime, installed) in runtimes.iter().zip(&installed) {
        let result: i32 = runtime
            .execute_typed(installed, "main", (), &context)
            .unwrap();
        assert_eq!(result, 42);
        if options.extra_functions != 0 {
            let calls: i32 = runtime
                .execute_typed(installed, "calls", (), &context)
                .unwrap();
            assert_eq!(calls, 1, "static cells must be installation-local");
        }
    }
    let initialized = snapshot(start);
    let domains = domains.len();
    let root_backings = backings.len();
    drop(installed);
    drop(runtimes);
    let released = snapshot(start);
    Measurement {
        phases: [initial, empty, loaded, initialized, released],
        domains,
        root_backings,
    }
}

fn print_sample(index: usize, value: &Measurement) {
    for (phase, pair) in ["empty", "load", "initialize", "release"]
        .into_iter()
        .zip(value.phases.windows(2))
    {
        #[cfg(feature = "diagnostics")]
        println!(
            "sharing,sample={index},phase={phase},requests={},requested_bytes={},net_bytes={}",
            pair[1].requests - pair[0].requests,
            pair[1].requested_bytes - pair[0].requested_bytes,
            pair[1].net_bytes - pair[0].net_bytes
        );
        #[cfg(not(feature = "diagnostics"))]
        println!(
            "sharing,sample={index},phase={phase},ns={}",
            pair[1] - pair[0]
        );
    }
    println!(
        "sharing_identity,sample={index},domains={},root_backings={}",
        value.domains, value.root_backings
    );
}

pub(super) fn run() {
    let options = Options::parse();
    let preparation = Instant::now();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let engine_ns = preparation.elapsed().as_nanos();
    let preparation = Instant::now();
    let checked = engine
        .compile_source(SourceFile::new("sharing.kgr", options.source()))
        .unwrap();
    let analysis_ns = preparation.elapsed().as_nanos();
    let preparation = Instant::now();
    let program = engine
        .prepare(
            &checked,
            PreparationOptions {
                retain_native_input: options.mir,
                ..Default::default()
            },
        )
        .unwrap();
    let product_ns = preparation.elapsed().as_nanos();
    assert_eq!(program.has_native_input(), options.mir);
    println!(
        "sharing_preparation,engine_ns={engine_ns},analysis_ns={analysis_ns},product_ns={product_ns}"
    );
    println!(
        "sharing_setup,runtimes={},extra_functions={},fresh_domain={},mir={}",
        options.runtimes, options.extra_functions, options.fresh_domain, options.mir
    );
    // Warm immutable/shared caches separately; output is outside phase snapshots.
    let mut samples = Vec::with_capacity(11);
    for index in 0..14 {
        #[cfg(feature = "diagnostics")]
        let (sample, total) = allocations::measure(|| lifecycle(&engine, &program, &options));
        #[cfg(not(feature = "diagnostics"))]
        let sample = lifecycle(&engine, &program, &options);
        if index >= 3 {
            #[cfg(feature = "diagnostics")]
            assert_eq!(total.net_bytes, 0, "warm runtime lifecycle must balance");
            samples.push(sample);
        }
    }
    for (index, sample) in samples.iter().enumerate() {
        print_sample(index, sample);
    }
}
