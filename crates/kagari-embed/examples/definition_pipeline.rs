//! Identity migration measurements; compile and allocator passes are separate.
use kagari_embed::engine::{KagariEngine, source::ArtifactOptions};
use kagari_runtime::{
    Runtime, module::VerifiedProgram, native::module::NativeModule, value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_stdlib as standard;
use kagari_vm::vm::Vm;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
    },
    time::Instant,
};

static COUNT: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

struct CountingAllocator;

// SAFETY: every operation forwards its original layout/pointer to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            let live =
                LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed) + layout.size() as isize;
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        // SAFETY: caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if COUNT.load(Ordering::Relaxed) {
            LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        }
        // SAFETY: caller supplies the original allocation and layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNT.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size, Ordering::Relaxed);
            let change = size as isize - layout.size() as isize;
            let live = LIVE.fetch_add(change, Ordering::Relaxed) + change;
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        // SAFETY: caller supplies the original allocation and valid new size.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measure<T>(name: &str, mut operation: impl FnMut() -> T) -> T {
    measure_setup(name, || (), |()| operation())
}

fn measure_setup<S, T>(
    name: &str,
    mut setup: impl FnMut() -> S,
    mut operation: impl FnMut(S) -> T,
) -> T {
    drop(operation(setup()));
    let mut samples = [0; 7];
    for sample in &mut samples {
        let input = setup();
        let start = Instant::now();
        let result = black_box(operation(input));
        *sample = start.elapsed().as_nanos();
        drop(result);
    }
    samples.sort_unstable();
    let input = setup();
    ALLOCATIONS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    LIVE.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
    COUNT.store(true, Ordering::Relaxed);
    let result = operation(input);
    COUNT.store(false, Ordering::Relaxed);
    println!(
        "{name}: median_ns={} allocations={} allocated_bytes={} retained_delta_bytes={} peak_delta_bytes={} samples_ns={samples:?}",
        samples[3],
        ALLOCATIONS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
        LIVE.load(Ordering::Relaxed),
        PEAK.load(Ordering::Relaxed)
    );
    result
}

fn main() {
    let mut source = "struct Player { var hp: i32 }\n".to_owned();
    for index in 0..32 {
        source.push_str(&format!("fn f{index}(p: Player) -> Player {{ p }}\n"));
    }
    source.push_str(
        "fn identity<T>(value: T) -> T { value }\nfn main() -> i32 { f31(Player { hp: 31 }).hp }\n",
    );
    let analyzed = measure("fresh_analysis", || {
        let engine = KagariEngine::default();
        let file = engine
            .set_source("identity-benchmark.kgr", source.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = engine
            .analyze(engine.source_snapshot(), &Default::default())
            .unwrap();
        assert!(
            snapshot
                .file(file)
                .unwrap()
                .result()
                .diagnostics()
                .is_empty()
        );
        (engine, snapshot)
    });
    drop(analyzed);
    let artifact = measure("compile_artifact", || {
        KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("identity-benchmark.kgr", source.clone()),
                ArtifactOptions::default(),
            )
            .unwrap()
    });
    println!("artifact_bytes={}", artifact.to_bytes().unwrap().len());
    let verified = measure("verified_program", || {
        VerifiedProgram::new(artifact.program.clone()).unwrap()
    });
    drop(measure("runtime_creation", Runtime::default));
    let (first_runtime, first) =
        measure_setup("runtime_import", standard_runtime, |mut runtime| {
            let loaded = runtime
                .load_verified_program("identity-benchmark", verified.clone())
                .unwrap();
            (runtime, loaded)
        });
    let mut second_runtime = standard_runtime();
    let second = second_runtime
        .load_verified_program("identity-benchmark", verified)
        .unwrap();
    println!(
        "independent_runtime_shared_module={}",
        Arc::ptr_eq(&first.bytecode, &second.bytecode)
    );
    let vm = Vm::new(first_runtime);
    assert!(matches!(
        vm.execute(&first, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(31)
    ));
    black_box((vm, second_runtime, first, second));
}

// Standard installation is explicit setup, outside the executable import measurement.
fn standard_runtime() -> Runtime {
    let mut runtime = Runtime::default();
    NativeModule::install_all(&standard::modules().unwrap(), &mut runtime).unwrap();
    runtime
}
