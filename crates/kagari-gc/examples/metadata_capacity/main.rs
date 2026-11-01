//! High-low collector metadata observations; requested bytes are not allocator RSS.

use kagari_gc::{dynamic::DynamicLayoutBuilder, error::GcError, heap::Heap};
use std::time::Duration;

fn emit(case: &str, stage: &str, heap: &Heap, collect: Duration, maintenance: Duration) {
    let stats = heap.metadata_stats();
    println!(
        "{case},{stage},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        heap.stats().backing_bytes,
        stats.pages.entries,
        stats.pages.slots,
        stats.pages.bytes,
        stats.roots.entries,
        stats.roots.slots,
        stats.roots.bytes,
        stats.descriptors.entries,
        stats.descriptors.slots,
        stats.descriptors.bytes,
        stats.mark_queue_bytes,
        stats.release_queue_bytes,
        stats.root_active_bytes + stats.descriptor_active_bytes,
        collect.as_nanos(),
        maintenance.as_nanos(),
    );
}

fn collect(case: &str, stage: &str, heap: &mut Heap) -> Result<(), GcError> {
    let report = heap.collect()?;
    emit(
        case,
        stage,
        heap,
        report.elapsed,
        report.maintenance_elapsed,
    );
    Ok(())
}

fn main() -> Result<(), GcError> {
    println!(
        "case,stage,backing_bytes,pages,page_slots,page_table_bytes,roots,root_slots,root_table_bytes,descriptors,descriptor_slots,descriptor_table_bytes,mark_queue_bytes,release_queue_bytes,active_bytes,collect_ns,maintenance_ns"
    );
    // One host root; every large dynamic record owns one physical allocation.
    let mut heap = Heap::default();
    let mut builder = DynamicLayoutBuilder::new();
    let value = builder.scalar::<u64>();
    for _ in 1..256 {
        builder.scalar::<u64>();
    }
    let layout = heap.access()?.register_dynamic(builder)?;
    let value = layout.scalar(value)?;
    let root = heap.scope(|cx| {
        let mut last = None;
        for i in 0..8192 {
            let object = cx.alloc_dynamic(&layout)?;
            value.set(cx, object, i)?;
            last = Some(object);
        }
        cx.retain(last.unwrap())
    })?;
    emit("pages", "peak", &heap, Duration::ZERO, Duration::ZERO);
    collect("pages", "one_high_id", &mut heap)?;
    heap.scope(|cx| {
        let object = cx.get(&root)?;
        assert_eq!(value.get(cx, object)?, 8191);
        Ok(())
    })?;
    drop((root, value, layout));
    collect("pages", "empty", &mut heap)?;

    let mut roots = (0..8192)
        .map(|_| heap.access()?.create_roots(0))
        .collect::<Result<Vec<_>, GcError>>()?;
    let high = roots.pop().unwrap();
    emit("roots", "peak", &heap, Duration::ZERO, Duration::ZERO);
    drop(roots);
    collect("roots", "one_high_id", &mut heap)?;
    drop(high);
    collect("roots", "empty", &mut heap)?;

    let mut layouts = (0..8192)
        .map(|_| {
            let mut builder = DynamicLayoutBuilder::new();
            builder.scalar::<u64>();
            heap.access()?.register_dynamic(builder)
        })
        .collect::<Result<Vec<_>, GcError>>()?;
    let high = layouts.pop().unwrap();
    emit("descriptors", "peak", &heap, Duration::ZERO, Duration::ZERO);
    drop(layouts);
    collect("descriptors", "one_high_id", &mut heap)?;
    drop(high);
    collect("descriptors", "empty", &mut heap)?;
    Ok(())
}
