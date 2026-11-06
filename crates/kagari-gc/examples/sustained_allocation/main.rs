//! Live-ring allocation with cooperative automatic collection; buffers every batch.
//! Cleanup measures post-root-release collect, not verification or full teardown.

// Reuse the safe root adapter; unused methods are tested at its original owner.
#[allow(dead_code)]
#[path = "../root_access/frames.rs"]
mod frames;

use frames::Frames;
use kagari_gc::{
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{HeapConfig, HeapStats, Phase, pacing::AutomaticKind},
    object::AnyObject,
    root_source::RootDriver,
};
use std::{env, time::Instant};

const RING: usize = 512;
const BATCHES: usize = 128;
const ALLOCATIONS: usize = 256;

struct Row {
    sample: usize,
    case: &'static str,
    batch: usize,
    allocations: usize,
    allocation_ns: u128,
    total_ns: u128,
    service: &'static str,
    service_ns: u128,
    work: usize,
    retired: usize,
    before_objects: usize,
    stats: HeapStats,
    cycles: u64,
    maintenance_ns: u128,
}

fn run(sample: usize, cyclic: bool, burst: bool, rows: &mut Vec<Row>) -> Result<(), GcError> {
    let mut driver = RootDriver::<Frames<AnyObject>>::new(HeapConfig::default())?;
    let leaf = driver.access()?.register::<u64>()?;
    let mut builder = DynamicLayoutBuilder::new();
    let field = builder.scalar::<u64>();
    let next = builder.reference();
    let layout = driver.access()?.register_dynamic(builder)?;
    let field = layout.scalar(field)?;
    let next = layout.reference(next)?;
    let id = driver.scope(|cx, roots| Frames::enter(roots, cx, RING))?;
    let mut total = 0;
    let mut cycles = 0;
    let case = if cyclic { "cycle" } else { "leaf" };
    for batch in 0..BATCHES {
        let count = if burst && batch % 16 == 0 {
            4096
        } else {
            ALLOCATIONS
        };
        // Snapshot before the batch so diagnostics stay outside both timers.
        let before = driver.stats();
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
        let report = driver.safepoint()?;
        let elapsed = start.elapsed().as_nanos();
        let stats = driver.stats();
        let (kind, work, retired, service_ns, maintenance_ns) = match report {
            Some(report) => {
                cycles += report.completed_cycles;
                assert!(
                    report.work > 0 || report.completed_cycles > 0 || stats.phase != before.phase
                );
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
                    report.maintenance_elapsed.as_nanos(),
                )
            }
            None => ("idle", 0, 0, 0, 0),
        };
        assert_eq!(stats.completed_cycles, cycles);
        rows.push(Row {
            sample,
            case,
            batch,
            allocations: count,
            allocation_ns: allocation,
            total_ns: elapsed,
            service: kind,
            service_ns,
            work,
            retired,
            before_objects: before.objects,
            stats,
            cycles,
            maintenance_ns,
        });
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
    // Verification and frame release above are excluded; this is not full teardown.
    let before = driver.stats();
    let start = Instant::now();
    let report = driver.collect()?;
    let total_ns = start.elapsed().as_nanos();
    cycles += report.completed_cycles;
    assert_eq!(
        report.completed_cycles,
        if before.phase == Phase::Idle { 1 } else { 2 }
    );
    assert!(report.work > 0 && report.retired_owners > 0);
    let stats = driver.stats();
    assert_eq!(stats.completed_cycles, cycles);
    assert_eq!(stats.objects, 0);
    assert_eq!(stats.backing_bytes, 0);
    assert_eq!(stats.indirect_bytes, 0);
    assert_eq!(stats.phase, Phase::Idle);
    rows.push(Row {
        sample,
        case,
        batch: BATCHES,
        allocations: 0,
        allocation_ns: 0,
        total_ns,
        service: "collect",
        service_ns: report.elapsed.as_nanos(),
        work: report.work,
        retired: report.retired_owners,
        before_objects: before.objects,
        stats,
        cycles,
        maintenance_ns: report.maintenance_elapsed.as_nanos(),
    });
    Ok(())
}

fn main() -> Result<(), GcError> {
    let burst = env::args().any(|arg| arg == "--burst");
    let mut rows = Vec::with_capacity(14 * 2 * (BATCHES + 1));
    for sample in 0..14 {
        run(sample, false, burst, &mut rows)?;
        run(sample, true, burst, &mut rows)?;
    }
    println!(
        "sample,case,batch,allocations,allocation_ns,total_ns,service,service_ns,work,retired,phase,before_objects,objects,peak_managed_bytes,cycles,maintenance_ns,backing_bytes,indirect_bytes,allocation_growth_bytes"
    );
    for row in rows {
        println!(
            "{},{},{},{},{},{},{},{},{},{},{:?},{},{},{},{},{},{},{},{}",
            row.sample,
            row.case,
            row.batch,
            row.allocations,
            row.allocation_ns,
            row.total_ns,
            row.service,
            row.service_ns,
            row.work,
            row.retired,
            row.stats.phase,
            row.before_objects,
            row.stats.objects,
            row.stats.peak_managed_bytes,
            row.cycles,
            row.maintenance_ns,
            row.stats.backing_bytes,
            row.stats.indirect_bytes,
            row.stats.allocation_growth_bytes
        );
    }
    Ok(())
}
