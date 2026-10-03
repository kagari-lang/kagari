//! Isolated architecture review probe; production sources remain untouched.
//! Warm interpreter execution only; every sample includes the SDK root envelope,
//! one interface construction, and 1,000 calls to m0 through that interface.
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::source::SourceFile;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fs,
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant},
};

const CALLS: usize = 1_000;
const WARMUPS: usize = 5;
const SAMPLES: usize = 21;

#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    allocs: usize,
    reallocs: usize,
    frees: usize,
    bytes: usize,
}

thread_local! {
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
}

struct Allocator;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn record(update: impl FnOnce(&mut Counts)) {
    let _ = ACTIVE.try_with(|active| {
        if let Some(mut counts) = active.get() {
            update(&mut counts);
            active.set(Some(counts));
        }
    });
}

// SAFETY: every pointer/layout is forwarded unchanged to System. Const-initialized
// thread-local counters do not allocate. Measurements run on this thread only.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|counts| {
            counts.allocs += 1;
            counts.bytes += layout.size();
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(|counts| {
            counts.allocs += 1;
            counts.bytes += layout.size();
        });
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(|counts| {
            counts.reallocs += 1;
            counts.bytes += size;
        });
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(|counts| counts.frees += 1);
        unsafe { System.dealloc(ptr, layout) };
    }
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(None));
    }
}

fn measured<T>(body: impl FnOnce() -> T) -> (T, Counts, Duration) {
    let reset = Reset;
    ACTIVE.with(|active| {
        assert!(active.get().is_none());
        active.set(Some(Counts::default()));
    });
    let start = Instant::now();
    let result = body();
    let duration = start.elapsed();
    let counts = ACTIVE.with(|active| active.get().unwrap());
    drop(reset);
    (result, counts, duration)
}

fn source(methods: usize) -> String {
    let mut source = String::from("trait Bench {\n");
    for method in 0..methods {
        source.push_str(&format!("fn m{method}(self) -> i32;\n"));
    }
    source.push_str("}\nstruct Worker {}\nimpl Bench for Worker {\n");
    for method in 0..methods {
        source.push_str(&format!("fn m{method}(self) -> i32 {{ 1 }}\n"));
    }
    source.push_str(&format!(
        "}}\nfn main() -> i32 {{\n  val worker: Bench = Worker {{}};\n  var i = 0; var sum = 0;\n  while i < {CALLS} {{ sum += worker.m0(); i += 1; }}\n  sum\n}}\n"
    ));
    source
}

fn main() {
    let (_, counter, _) = measured(|| {
        let mut items = vec![black_box(1u64)];
        black_box(&mut items).push(black_box(2u64));
        black_box(&items);
    });
    assert_eq!((counter.allocs, counter.reallocs, counter.frees), (1, 1, 1));
    assert!(counter.bytes >= 24);

    println!(
        "profile=dev_O1,features=embed_source_native,mode=interpreter,calls={CALLS},warmups={WARMUPS},samples={SAMPLES},allocator=thread_local_counting_system"
    );
    println!(
        "scope=execute_including_session_interface_construction_and_return_report,compilation_preparation_linking_and_report_drop_excluded=true,requested_bytes_are_cumulative=true"
    );
    for methods in [1, 8, 32] {
        let source = source(methods);
        // Persist the complete workload before compilation/timing.
        let path = format!("interface_{methods}.kgr");
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/architecture-review/workloads");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join(&path), &source).unwrap();
        let preparation = Instant::now();
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(SourceFile::new(&path, source), Default::default())
            .unwrap();
        let root_module = &artifact.program.modules[artifact.program.root.index()];
        let main = root_module
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap();
        assert_eq!(
            main.instructions
                .iter()
                .filter(|instruction| matches!(
                    instruction,
                    BytecodeInstruction::Call {
                        callee: CallTarget::InterfaceMethod { .. },
                        ..
                    }
                ))
                .count(),
            1,
            "the loop must retain one dynamic interface call site"
        );
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        println!(
            "methods={methods},compile_prepare_link_ns={}",
            preparation.elapsed().as_nanos()
        );
        for _ in 0..WARMUPS {
            let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
            assert_eq!(report.return_value, Value::I32(CALLS as i32));
            assert!(report.jit.is_none());
            black_box(report);
        }
        let mut samples = Vec::with_capacity(SAMPLES);
        for sample in 0..SAMPLES {
            let (report, counts, elapsed) = measured(|| {
                runtime
                    .execute(black_box(&loaded), "main", &[], &context)
                    .unwrap()
            });
            assert_eq!(report.return_value, Value::I32(CALLS as i32));
            assert!(report.jit.is_none());
            black_box(&report);
            let nanos = elapsed.as_nanos();
            println!(
                "methods={methods},sample={sample},ns={nanos},allocs={},reallocs={},frees={},requested_bytes={}",
                counts.allocs, counts.reallocs, counts.frees, counts.bytes
            );
            samples.push((nanos, counts.allocs, counts.bytes));
        }
        let mut times: Vec<_> = samples.iter().map(|sample| sample.0).collect();
        let mut allocs: Vec<_> = samples.iter().map(|sample| sample.1).collect();
        let mut bytes: Vec<_> = samples.iter().map(|sample| sample.2).collect();
        times.sort_unstable();
        allocs.sort_unstable();
        bytes.sort_unstable();
        println!(
            "summary,methods={methods},median_ns={},min_ns={},max_ns={},median_allocs={},median_requested_bytes={}",
            times[SAMPLES / 2],
            times[0],
            times[SAMPLES - 1],
            allocs[SAMPLES / 2],
            bytes[SAMPLES / 2]
        );
    }
}
