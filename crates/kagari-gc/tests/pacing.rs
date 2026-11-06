// Reuse the reviewed execution-root adapter and provider contracts.
#[allow(dead_code)]
#[path = "../examples/root_access/frames.rs"]
mod frames;
mod support;

use frames::Frames;
use kagari_gc::{
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{
        Heap, HeapConfig, Phase,
        pacing::{AutomaticKind, PacingConfig},
    },
    object::AnyObject,
    root_source::RootDriver,
    scan::WorkBudget,
};
use std::sync::{Arc, atomic::AtomicUsize};
use support::{Node, ObjectVec};

#[test]
fn automatic_service_sleeps_advances_without_allocations_and_preserves_limits()
-> Result<(), GcError> {
    let mut heap = Heap::default();
    assert!(heap.safepoint()?.is_none());
    let retained = heap.access()?.alloc_leaf(42u64)?;
    heap.scope(|cx| {
        for i in 0..1023 {
            cx.alloc_leaf(i as u64)?;
        }
        Ok(())
    })?;
    let first = heap.safepoint()?.expect("threshold reached");
    assert_eq!(first.kind, AutomaticKind::Step);
    assert_eq!(heap.stats().allocation_growth_bytes, 0);
    for _ in 0..1000 {
        if heap.stats().phase == Phase::Idle {
            break;
        }
        let report = heap
            .safepoint()?
            .expect("active progress without new allocation");
        assert!(report.work <= 1024 && report.retired_owners <= 32);
    }
    assert_eq!(heap.stats().objects, 1);
    assert_eq!(heap.stats().phase, Phase::Idle);
    assert!(heap.safepoint()?.is_none());
    assert_eq!(
        heap.scope(|cx| {
            let value = cx.get(&retained)?;
            Ok(cx.read(value, |v| *v))
        })?,
        42
    );
    // An explicit partial cycle also progresses through the automatic entrypoint.
    heap.step(WorkBudget::new(8))?;
    while heap.stats().phase != Phase::Idle {
        heap.safepoint()?;
    }
    drop(retained);
    heap.collect()?;
    heap.shutdown();
    assert!(matches!(heap.safepoint(), Err(GcError::Closed)));

    let mut limited = Heap::new(HeapConfig {
        max_objects: 4,
        ..HeapConfig::default()
    })?;
    limited.scope(|cx| {
        for i in 0..4 {
            cx.alloc_leaf(i as u64)?;
        }
        assert!(matches!(
            cx.alloc_leaf(5u64),
            Err(GcError::Limit("objects"))
        ));
        Ok(())
    })?;
    let assist = limited.safepoint()?.unwrap();
    assert_eq!(assist.kind, AutomaticKind::Assist);
    assert_eq!(assist.completed_cycles, 1);
    assert_eq!(limited.stats().objects, 0);
    assert!(limited.safepoint()?.is_none());
    Ok(())
}

#[test]
fn pacing_counts_reused_storage_and_committed_indirect_growth_only() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let node = Node::alloc(&mut heap.access()?, 1, &drops)?;
    let vector = ObjectVec::alloc(&mut heap.access()?, &[])?;
    let _anchor = heap.access()?.alloc_leaf(0u64)?;
    heap.collect()?;
    let backing = heap.stats().backing_bytes;
    heap.scope(|cx| cx.alloc_leaf(1u64).map(|_| ()))?;
    assert_eq!(heap.stats().backing_bytes, backing);
    let reused_growth = heap.stats().allocation_growth_bytes;
    assert!(reused_growth > 0);
    let before = heap.stats().indirect_bytes;
    ObjectVec::push(&mut heap.access()?, &vector, &node)?;
    assert_eq!(
        heap.stats().allocation_growth_bytes,
        reused_growth + heap.stats().indirect_bytes - before
    );
    let growth = heap.stats().allocation_growth_bytes;
    let descriptor = heap.access()?.register::<u64>()?;
    // SAFETY: initialization fails before producing any storage or edge.
    let result =
        heap.scope(|cx| unsafe { descriptor.alloc_with(cx, |_| Err(GcError::Index)) }.map(|_| ()));
    assert!(matches!(result, Err(GcError::Index)));
    assert_eq!(heap.stats().allocation_growth_bytes, growth);
    Ok(())
}

#[test]
fn sustained_default_policy_bounds_a_fixed_live_ring_and_retains_cycles() -> Result<(), GcError> {
    for cyclic in [false, true] {
        let mut driver = RootDriver::<Frames<AnyObject>>::new(HeapConfig::default())?;
        let descriptor = driver.access()?.register::<u64>()?;
        let mut builder = DynamicLayoutBuilder::new();
        let field = builder.scalar::<u64>();
        let next = builder.reference();
        let layout = driver.access()?.register_dynamic(builder)?;
        let field = layout.scalar(field)?;
        let next = layout.reference(next)?;
        let id = driver.scope(|cx, roots| Frames::enter(roots, cx, 512))?;
        let mut sequence = 0usize;
        let mut assists = 0;
        for batch in 0..1024 {
            // Periodic larger bursts exercise assists, not only regular steps.
            let allocations = if batch % 16 == 0 { 4096 } else { 256 };
            driver.scope(|cx, roots| {
                for _ in 0..allocations {
                    let object = if cyclic {
                        let object = cx.alloc_dynamic(&layout)?;
                        field.set(cx, object, sequence as u64)?;
                        next.set(cx, object, Some(object))?;
                        object
                    } else {
                        descriptor.alloc_leaf(cx, sequence as u64)?.erase()
                    };
                    Frames::with_frame(roots, cx, id, |frame| {
                        frame.set(sequence % 512, Some(object))
                    })?;
                    sequence += 1;
                }
                Ok(())
            })?;
            assert!(
                driver.stats().objects <= 16384,
                "fixed live set must not grow with total allocations"
            );
            if let Some(report) = driver.safepoint()? {
                if report.kind == AutomaticKind::Step {
                    assert!(report.work <= 1024 && report.retired_owners <= 32);
                } else {
                    assists += 1;
                    assert_eq!(report.completed_cycles, 1);
                }
            }
        }
        assert!(assists > 0 && driver.stats().completed_cycles > 0);
        driver.scope(|cx, roots| {
            for slot in 0..512 {
                let object = Frames::with_frame(roots, cx, id, |frame| frame.get(slot))?.unwrap();
                let value = if cyclic {
                    field.get(cx, object)?
                } else {
                    let value = cx.downcast::<u64>(object)?;
                    cx.read(value, |v| *v)
                };
                assert_eq!(value as usize, sequence - 512 + slot);
            }
            Frames::leave(roots, cx, id)
        })?;
        driver.collect()?;
        assert_eq!(driver.stats().objects, 0);
        assert_eq!(driver.stats().backing_bytes, 0);
    }
    Ok(())
}

#[test]
fn pacing_configuration_rejects_zero_thresholds() {
    for pacing in [
        PacingConfig {
            min_growth_bytes: 0,
            ..PacingConfig::default()
        },
        PacingConfig {
            growth_percent: 0,
            ..PacingConfig::default()
        },
        PacingConfig {
            assist_multiplier: 0,
            ..PacingConfig::default()
        },
    ] {
        assert!(matches!(
            Heap::new(HeapConfig {
                pacing,
                ..HeapConfig::default()
            }),
            Err(GcError::Limit(_))
        ));
    }
}
