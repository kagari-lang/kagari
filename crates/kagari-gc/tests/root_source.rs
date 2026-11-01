// Share the actual standalone adapter so its safe facade, cursor and publication
// behavior are tested rather than a second implementation of the same protocol.
#[path = "../examples/root_access/frames.rs"]
mod frames;
mod support;

use frames::Frames;
use kagari_gc::{
    dynamic::FieldType,
    error::GcError,
    heap::{HeapConfig, Phase},
    object::AnyObject,
    root_source::{RootDriver, RootSource},
    scan::{EdgeVisitor, ScanBudget, ScanStatus, WorkBudget},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::Node;

fn finish<R: RootSource>(driver: &mut RootDriver<R>) -> Result<(), GcError> {
    for _ in 0..10000 {
        if driver.stats().phase == Phase::Idle {
            return Ok(());
        }
        let report = driver.step(WorkBudget::new(8))?;
        assert!(report.work <= 8);
        assert!(report.retired_owners <= 32);
    }
    panic!("root source failed to finish");
}

#[test]
fn moving_a_root_behind_the_cursor_keeps_its_transitive_cycle_alive() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut driver = RootDriver::<Frames<Node>>::new(HeapConfig {
        max_roots: 0,
        ..HeapConfig::default()
    })?;
    let frame = driver.scope(|cx, roots| {
        let a = Node::alloc_scoped(cx, 1, &drops)?;
        let b = Node::alloc_scoped(cx, 2, &drops)?;
        Node::NEXT.set(cx, a, Some(b));
        Node::NEXT.set(cx, b, Some(a));
        let id = Frames::enter(roots, cx, 64)?;
        Frames::with_frame(roots, cx, id, |frame| frame.set(63, Some(a)))?;
        Ok(id)
    })?;
    assert_eq!(driver.stats().root_registrations, 0);
    driver.step(WorkBudget::new(8))?;
    assert_eq!(driver.stats().phase, Phase::Roots);
    driver.scope(|cx, roots| {
        Frames::with_frame(roots, cx, frame, |frame| {
            frame.copy(0, 63)?;
            frame.set(63, None)?;
            // Reallocate the bank between slices; the cursor must reacquire it.
            frame.grow(4096);
            Ok(())
        })
    })?;
    finish(&mut driver)?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    driver.collect()?;
    assert_eq!(driver.stats().objects, 2);
    driver.scope(|cx, roots| Frames::leave(roots, cx, frame))?;
    driver.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(driver.stats().backing_bytes, 0);
    Ok(())
}

#[test]
fn frame_reuse_new_frontiers_and_reentry_preserve_all_live_roots() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut driver = RootDriver::<Frames<Node>>::new(HeapConfig::default())?;
    let outer = driver.scope(|cx, roots| {
        let value = Node::alloc_scoped(cx, 1, &drops)?;
        let transfer = Node::alloc_scoped(cx, 2, &drops)?;
        let id = Frames::enter(roots, cx, 64)?;
        Frames::with_frame(roots, cx, id, |frame| {
            frame.set(62, Some(transfer))?;
            frame.set(63, Some(value))
        })?;
        Ok(id)
    })?;
    driver.step(WorkBudget::new(8))?;
    // Model a host callback: the outer scope ended but its frame stays visible.
    let inner = driver.scope(|cx, roots| {
        // Transfer an OLD, not-yet-scanned target beyond the captured frontier;
        // newly allocated black objects would not exercise this write barrier.
        let value = Frames::with_frame(roots, cx, outer, |frame| frame.get(62))?;
        let id = Frames::enter(roots, cx, 1)?;
        Frames::with_frame(roots, cx, id, |frame| frame.set(0, value))?;
        Frames::with_frame(roots, cx, outer, |frame| frame.set(62, None))?;
        Ok(id)
    })?;
    // Full collect must include the source in both the carried and fresh cycles.
    assert_eq!(driver.collect()?.completed_cycles, 2);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    driver.step(WorkBudget::new(8))?;
    let reused = driver.scope(|cx, roots| {
        Frames::leave(roots, cx, outer)?;
        let reused = Frames::enter(roots, cx, 1)?;
        assert_ne!(reused, outer);
        let value = Frames::with_frame(roots, cx, inner, |frame| frame.get(0))?;
        Frames::with_frame(roots, cx, reused, |frame| frame.set(0, value))?;
        Frames::leave(roots, cx, inner)?;
        assert!(Frames::with_frame(roots, cx, outer, |_| Ok(())).is_err());
        Ok(reused)
    })?;
    finish(&mut driver)?;
    driver.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    driver.scope(|cx, roots| {
        let value = Frames::with_frame(roots, cx, reused, |frame| frame.get(0))?.unwrap();
        assert_eq!(Node::VALUE.get(cx, value), 2);
        Frames::leave(roots, cx, reused)
    })?;
    driver.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    Ok(())
}

#[test]
fn host_roots_scope_unwind_foreign_access_and_shutdown_share_the_same_owner() -> Result<(), GcError>
{
    let mut driver = RootDriver::<Frames<u64>>::new(HeapConfig::default())?;
    let mut other = RootDriver::<Frames<u64>>::new(HeapConfig::default())?;
    let (id, host) = driver.scope(|cx, roots| {
        let value = cx.alloc_leaf(42u64)?;
        let id = Frames::enter(roots, cx, 1)?;
        Frames::with_frame(roots, cx, id, |frame| frame.set(0, Some(value)))?;
        Ok((id, cx.retain(value)?))
    })?;
    driver.scope(|_, roots| {
        other.scope(|cx, _| {
            assert!(matches!(
                Frames::with_frame(roots, cx, id, |_| Ok(())),
                Err(GcError::ForeignHeap)
            ));
            Ok(())
        })
    })?;
    assert!(
        catch_unwind(AssertUnwindSafe(|| driver.scope::<()>(|cx, roots| {
            let value = cx.alloc_leaf(7u64)?;
            Frames::with_frame(roots, cx, id, |frame| frame.set(0, Some(value)))?;
            panic!("user operation after committed root write");
        })))
        .is_err()
    );
    driver.collect()?;
    assert_eq!(driver.stats().objects, 2);
    drop(host);
    driver.collect()?;
    assert_eq!(driver.stats().objects, 1);
    let host = driver.scope(|cx, roots| {
        let value = Frames::with_frame(roots, cx, id, |frame| frame.get(0))?.unwrap();
        cx.retain(value)
    })?;
    assert_eq!(driver.shutdown().disposed_objects, 1);
    assert!(matches!(driver.collect(), Err(GcError::Closed)));
    assert!(matches!(driver.scope(|_, _| Ok(())), Err(GcError::Closed)));
    drop(host);
    Ok(())
}

#[test]
fn erased_dynamic_cycles_are_retained_by_the_direct_source() -> Result<(), GcError> {
    let mut driver = RootDriver::<Frames<AnyObject>>::new(HeapConfig::default())?;
    let layout = driver.access()?.register_dynamic(&[FieldType::Reference])?;
    let next = layout.reference(0)?;
    let id = driver.scope(|cx, roots| {
        let a = cx.alloc_dynamic(&layout)?;
        let b = cx.alloc_dynamic(&layout)?;
        next.set(cx, a, Some(b))?;
        next.set(cx, b, Some(a))?;
        let id = Frames::enter(roots, cx, 1)?;
        Frames::with_frame(roots, cx, id, |frame| frame.set(0, Some(a)))?;
        Ok(id)
    })?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 2);
    driver.scope(|cx, roots| Frames::leave(roots, cx, id))?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 0);
    Ok(())
}

struct Broken<const MODE: u8>;

// SAFETY: no managed storage is exposed. Deliberate progress/failure violations
// below exercise fault containment, never publish or dereference invalid pointers.
unsafe impl<const MODE: u8> RootSource for Broken<MODE> {
    type Cursor = ();

    fn empty() -> Self {
        Self
    }

    fn begin_scan(&self) -> Result<(), GcError> {
        if MODE == 0 {
            panic!("begin failed");
        }
        Ok(())
    }

    fn scan(
        &self,
        _: &mut (),
        budget: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        match MODE {
            1 => Err(GcError::Provider("source failed")),
            2 => panic!("source panicked"),
            3 => Ok(ScanStatus::Pending),
            _ => {
                budget.charge(budget.remaining() + 1)?;
                Ok(ScanStatus::Complete)
            }
        }
    }
}

fn rejects_source_failure<const MODE: u8>(automatic: bool) -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut driver = RootDriver::<Broken<MODE>>::new(HeapConfig {
        max_objects: 1,
        ..HeapConfig::default()
    })?;
    driver.scope(|cx, _| {
        Node::alloc_scoped(cx, 1, &drops)?;
        Ok(())
    })?;
    if automatic {
        assert!(driver.safepoint().is_err());
    } else {
        assert!(driver.step(WorkBudget::NORMAL).is_err());
    }
    assert_eq!(driver.stats().phase, Phase::Faulted);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert!(matches!(driver.collect(), Err(GcError::Faulted)));
    assert_eq!(driver.shutdown().disposed_objects, 1);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn incomplete_or_failed_sources_cannot_transition_to_sweep() -> Result<(), GcError> {
    for automatic in [false, true] {
        rejects_source_failure::<0>(automatic)?;
        rejects_source_failure::<1>(automatic)?;
        rejects_source_failure::<2>(automatic)?;
        rejects_source_failure::<3>(automatic)?;
        rejects_source_failure::<4>(automatic)?;
    }
    Ok(())
}
