//! Single-thread identity representation probe, with separate allocation counting.
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
    table::{DefinitionId, DefinitionTableBuilder},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Instant,
};

static COUNT: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

// SAFETY: every operation forwards its original layout/pointer to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: caller supplies the original allocation and layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNT.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: caller supplies the original allocation and valid new size.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations(mut operation: impl FnMut()) -> usize {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNT.store(true, Ordering::Relaxed);
    operation();
    COUNT.store(false, Ordering::Relaxed);
    ALLOCATIONS.load(Ordering::Relaxed)
}

fn median_ns(mut operation: impl FnMut()) -> u128 {
    let mut samples = [0; 11];
    operation();
    for sample in &mut samples {
        let start = Instant::now();
        operation();
        *sample = start.elapsed().as_nanos();
    }
    samples.sort_unstable();
    samples[5]
}

fn main() {
    const COPIES: usize = 100_000;
    let path = DefinitionPath {
        module: ModuleIdentity::single_file("game.kgr"),
        path: vec![
            DefinitionPathSegment {
                kind: DefinitionKind::Struct,
                name: "Player".into(),
                occurrence: 0,
            },
            DefinitionPathSegment {
                kind: DefinitionKind::Field,
                name: "hp".into(),
                occurrence: 0,
            },
        ],
    };
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let id = builder.intern_path(&path).unwrap();
    let table = builder.freeze();
    assert_eq!(table.resolve(id).unwrap().to_path(), path);
    let owned = || {
        for _ in 0..COPIES {
            black_box(black_box(&path).clone());
        }
    };
    let scoped = || {
        for _ in 0..COPIES {
            black_box(*black_box(&id));
        }
    };
    let owned_allocations = allocations(owned);
    let scoped_allocations = allocations(scoped);
    let owned_ns = median_ns(owned);
    let scoped_ns = median_ns(scoped);
    println!(
        "{{\"copies\":{COPIES},\"owned_bytes\":{},\"scoped_bytes\":{},\"owned_allocations\":{owned_allocations},\"scoped_allocations\":{scoped_allocations},\"owned_median_ns\":{owned_ns},\"scoped_median_ns\":{scoped_ns}}}",
        size_of::<DefinitionPath>(),
        size_of::<DefinitionId>(),
    );
    assert_eq!(scoped_allocations, 0);
}
