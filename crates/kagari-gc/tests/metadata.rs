use kagari_gc::{
    dynamic::FieldType,
    error::GcError,
    heap::{Heap, Phase},
    object::GcObject,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, WorkBudget},
};
use std::{
    sync::{Arc, Barrier},
    thread,
};

struct Large([u64; 256]);

// SAFETY: initialized scalar storage with no references or indirect resources.
unsafe impl GcObject for Large {
    type ScanState = ();

    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }
}

#[test]
fn high_and_scattered_survivors_release_historical_pages_and_root_slots() -> Result<(), GcError> {
    let mut heap = Heap::default();
    for wave in 0..3 {
        let mut roots = (0..8192)
            .map(|i| heap.access()?.alloc_leaf(Large([i; 256])).map(Some))
            .collect::<Result<Vec<_>, GcError>>()?;
        let peak = heap.metadata_stats();
        assert_eq!(peak.pages.entries, 8192);
        assert_eq!(peak.roots.entries, 8192);
        for (i, root) in roots.iter_mut().enumerate() {
            if i != 8191 && (wave != 1 || i % 2048 != 0) {
                *root = None;
            }
        }
        heap.collect()?;
        let low = heap.metadata_stats();
        let survivors = roots.iter().flatten().count();
        assert_eq!(low.pages.entries, survivors);
        assert_eq!(low.roots.entries, survivors);
        assert!(low.pages.slots <= survivors * 256);
        assert!(low.roots.slots <= survivors * 256);
        assert!(low.pages.bytes < peak.pages.bytes / 4);
        assert!(low.roots.bytes < peak.roots.bytes / 4);
        assert!(low.mark_queue_bytes < peak.mark_queue_bytes / 4);
        assert!(low.release_queue_bytes < peak.release_queue_bytes / 4);
        assert_eq!(low.descriptors.entries, 1);
        for (i, root) in roots.iter().enumerate() {
            if let Some(root) = root {
                heap.scope(|cx| {
                    let object = cx.get(root)?;
                    cx.read(object, |value| assert_eq!(value.0, [i as u64; 256]));
                    Ok(())
                })?;
            }
        }
        // Allocate into freed IDs while an old high-ID source remains live.
        let replacement = heap.access()?.alloc_leaf(Large([99; 256]))?;
        heap.collect()?;
        assert_eq!(heap.stats().objects, survivors + 1);
        drop((replacement, roots));
        heap.collect()?;
        let empty = heap.metadata_stats();
        assert_eq!(
            empty.pages.bytes + empty.roots.bytes + empty.descriptors.bytes,
            0
        );
        assert_eq!(empty.root_active_bytes + empty.descriptor_active_bytes, 0);
        assert_eq!(empty.release_queue_bytes + empty.mark_queue_bytes, 0);
        assert_eq!(empty.type_map_capacity + empty.scan_workspace_bytes, 0);
        assert_eq!(heap.stats().backing_bytes, 0);
    }
    Ok(())
}

#[test]
fn descriptor_blocks_keep_external_layout_and_field_leases() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let mut layouts = (0..2048)
        .map(|_| heap.access()?.register_dynamic(&[FieldType::U64]))
        .collect::<Result<Vec<_>, GcError>>()?;
    let layout = layouts.pop().unwrap();
    let field = layout.scalar::<u64>(0)?;
    drop(layouts);
    heap.collect()?;
    let metadata = heap.metadata_stats();
    assert_eq!(metadata.descriptors.entries, 1);
    assert!(metadata.descriptors.slots <= 256);
    assert!(metadata.scan_workspace_bytes >= size_of::<usize>());
    let replacement = heap.access()?.register_dynamic(&[FieldType::U64])?;
    let root = heap.scope(|cx| {
        let object = cx.alloc_dynamic(&layout)?;
        field.set(cx, object, 42)?;
        let other = cx.alloc_dynamic(&replacement)?;
        assert_eq!(field.get(cx, other), Err(GcError::Representation));
        cx.retain(object)
    })?;
    drop((layout, replacement));
    heap.collect()?;
    assert_eq!(heap.metadata_stats().descriptors.entries, 1);
    heap.scope(|cx| {
        let object = cx.get(&root)?;
        assert_eq!(field.get(cx, object)?, 42);
        Ok(())
    })?;
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.metadata_stats().descriptors.entries, 1);
    drop(field);
    heap.collect()?;
    assert_eq!(heap.metadata_stats().descriptors.bytes, 0);
    assert_eq!(heap.metadata_stats().scan_workspace_bytes, 0);
    Ok(())
}

#[test]
fn release_queue_shrink_reserves_credits_for_concurrent_late_drops() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let mut roots = (0..8192)
        .map(|_| heap.access()?.create_roots(0))
        .collect::<Result<Vec<_>, GcError>>()?;
    let late = roots.split_off(8192 - 256);
    drop(roots);
    heap.collect()?;
    assert_eq!(heap.stats().root_registrations, 256);
    let barrier = Arc::new(Barrier::new(2));
    let release = barrier.clone();
    let thread = thread::spawn(move || {
        release.wait();
        for root in late {
            drop(root);
            thread::yield_now();
        }
    });
    barrier.wait();
    for _ in 0..256 {
        heap.trim_metadata()?;
        thread::yield_now();
    }
    thread.join().unwrap();
    heap.collect()?;
    assert_eq!(heap.metadata_stats().release_queue_bytes, 0);
    assert_eq!(heap.metadata_stats().roots.bytes, 0);
    Ok(())
}

#[repr(C, align(256))]
#[derive(Clone, Copy)]
struct WideCursor([u8; 4096]);

struct Wide;

// SAFETY: no edges or resources; the Copy cursor owns no pointers. Scan reads
// initialized bounded state and consumes one unit before completing.
unsafe impl GcObject for Wide {
    type ScanState = WideCursor;

    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> WideCursor {
        WideCursor([0xa5; 4096])
    }

    fn scan(
        &self,
        state: &mut WideCursor,
        budget: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        assert_eq!((state as *const WideCursor).addr() % 256, 0);
        assert_eq!(state.0[0], 0xa5);
        assert_eq!(state.0[4095], 0xa5);
        Ok(if budget.take(1) {
            ScanStatus::Complete
        } else {
            ScanStatus::Pending
        })
    }
}

#[test]
fn workspace_shrink_waits_for_idle_and_preserves_registered_requirements() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let wide = heap.access()?.register::<Wide>()?;
    let small = heap.access()?.register::<u64>()?;
    heap.collect()?;
    assert_eq!(heap.stats().scan_workspace_bytes, 4096);
    let root = heap.scope(|cx| {
        // SAFETY: Wide contains no edges or indirect charge.
        let object = unsafe { wide.alloc_with(cx, |_| Ok(Wide)) }?;
        cx.retain(object)
    })?;
    heap.step(WorkBudget::new(8))?;
    assert_ne!(heap.stats().phase, Phase::Idle);
    assert!(heap.trim_metadata()?.is_none());
    heap.collect()?;
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().scan_workspace_bytes, 4096);
    drop(wide);
    heap.collect()?;
    assert!(heap.stats().scan_workspace_bytes < 4096);
    assert_eq!(heap.metadata_stats().descriptors.entries, 1);
    drop(small);
    heap.collect()?;
    assert_eq!(heap.stats().scan_workspace_bytes, 0);
    heap.shutdown();
    assert_eq!(heap.trim_metadata(), Err(GcError::Closed));
    assert_eq!(heap.metadata_stats().mark_queue_bytes, 0);
    Ok(())
}

#[test]
fn completing_safepoints_maintain_buffers_but_steps_leave_timing_to_the_driver()
-> Result<(), GcError> {
    let mut heap = Heap::default();
    let roots = (0..4096)
        .map(|_| heap.access()?.create_roots(0))
        .collect::<Result<Vec<_>, GcError>>()?;
    drop(roots);
    loop {
        let report = heap.safepoint()?.expect("pending releases or active cycle");
        assert!(report.maintenance_elapsed <= report.elapsed);
        if heap.stats().phase == Phase::Idle {
            break;
        }
    }
    assert_eq!(heap.metadata_stats().release_queue_bytes, 0);
    let roots = (0..4096)
        .map(|_| heap.access()?.create_roots(0))
        .collect::<Result<Vec<_>, GcError>>()?;
    drop(roots);
    loop {
        let report = heap.step(WorkBudget::new(8))?;
        assert!(report.work <= 8);
        if report.phase == Phase::Idle {
            break;
        }
    }
    assert_eq!(heap.metadata_stats().roots.bytes, 0);
    assert!(heap.metadata_stats().release_queue_bytes > 0);
    assert!(heap.trim_metadata()?.is_some());
    assert_eq!(heap.metadata_stats().release_queue_bytes, 0);
    Ok(())
}
