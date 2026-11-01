//! Live-ring allocation with cooperative automatic collection; emits every batch.

// Reuse the safe root adapter; unused methods are tested at its original owner.
#[allow(dead_code)]
#[path = "../root_access/frames.rs"]
mod frames;

use frames::Frames;
use kagari_gc::{
    dynamic::FieldType,
    error::GcError,
    heap::{HeapConfig, Phase, pacing::AutomaticKind},
    object::AnyObject,
    root_source::RootDriver,
};
use std::{env, time::Instant};

const RING: usize = 512;
const BATCHES: usize = 128;
const ALLOCATIONS: usize = 256;

fn run(sample: usize, cyclic: bool, burst: bool) -> Result<(), GcError> {
    let mut driver = RootDriver::<Frames<AnyObject>>::new(HeapConfig::default())?;
    let leaf = driver.access()?.register::<u64>()?;
    let layout = driver
        .access()?
        .register_dynamic(&[FieldType::U64, FieldType::Reference])?;
    let field = layout.scalar::<u64>(0)?;
    let next = layout.reference(1)?;
    let id = driver.scope(|cx, roots| Frames::enter(roots, cx, RING))?;
    let mut total = 0;
    let mut cycles = 0;
    for batch in 0..BATCHES {
        let count = if burst && batch % 16 == 0 {
            4096
        } else {
            ALLOCATIONS
        };
        let start = Instant::now();
        driver.scope(|cx, roots| {
            for _ in 0..count {
                let value = total as u64;
                let object = if cyclic {
                    let object = cx.alloc_dynamic(&layout)?;
                    field.set(cx, object, value)?;
                    next.set(cx, object, Some(object))?;
                    object
                } else {
                    leaf.alloc_leaf(cx, value)?.erase()
                };
                Frames::with_frame(roots, cx, id, |frame| frame.set(total % RING, Some(object)))?;
                total += 1;
            }
            Ok(())
        })?;
        let allocation = start.elapsed().as_nanos();
        let before = driver.stats();
        let report = driver.safepoint()?;
        let elapsed = start.elapsed().as_nanos();
        let stats = driver.stats();
        let (kind, work, retired, service_ns) = match report {
            Some(report) => {
                cycles += report.completed_cycles;
                if report.kind == AutomaticKind::Step {
                    assert!(report.work <= 1024 && report.retired_owners <= 32);
                }
                (
                    if report.kind == AutomaticKind::Step {
                        "step"
                    } else {
                        "assist"
                    },
                    report.work,
                    report.retired_owners,
                    report.elapsed.as_nanos(),
                )
            }
            None => ("idle", 0, 0, 0),
        };
        println!(
            "{sample},{},{batch},{count},{allocation},{elapsed},{kind},{service_ns},{work},{retired},{:?},{},{},{},{cycles}",
            if cyclic { "cycle" } else { "leaf" },
            stats.phase,
            before.objects,
            stats.objects,
            stats.peak_managed_bytes
        );
    }
    assert!(cycles > 0);
    // Final live-ring values are checked before explicit cleanup can hide a miss.
    driver.scope(|cx, roots| {
        for slot in 0..RING {
            let object = Frames::with_frame(roots, cx, id, |frame| frame.get(slot))?.unwrap();
            let value = if cyclic {
                field.get(cx, object)?
            } else {
                let object = cx.downcast::<u64>(object)?;
                cx.read(object, |v| *v)
            };
            let expected = total - RING + (slot + RING - total % RING) % RING;
            assert_eq!(value, expected as u64);
        }
        Frames::leave(roots, cx, id)
    })?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 0);
    assert_eq!(driver.stats().backing_bytes, 0);
    assert_eq!(driver.stats().phase, Phase::Idle);
    Ok(())
}

fn main() -> Result<(), GcError> {
    let burst = env::args().any(|arg| arg == "--burst");
    println!(
        "sample,case,batch,allocations,allocation_ns,total_ns,service,service_ns,work,retired,phase,before_objects,objects,peak_managed_bytes,cycles"
    );
    for sample in 0..14 {
        run(sample, false, burst)?;
        run(sample, true, burst)?;
    }
    Ok(())
}
