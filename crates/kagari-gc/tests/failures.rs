mod support;

use kagari_gc::{
    error::GcError,
    heap::{Heap, HeapConfig, Phase},
    object::{GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, WorkBudget},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use support::{Node, ObjectVec};

#[test]
fn initialization_error_and_panic_release_the_reserved_page_and_root() -> Result<(), GcError> {
    let mut heap = Heap::default();
    {
        let mut access = heap.access()?;
        let descriptor = access.register::<u64>()?;
        let failed = unsafe { access.alloc_with(&descriptor, |_| Err(GcError::Index)) };
        assert!(matches!(failed, Err(GcError::Index)));
        let panicked = catch_unwind(AssertUnwindSafe(|| unsafe {
            access.alloc_with(&descriptor, |_| -> Result<u64, GcError> {
                panic!("initializer failure")
            })
        }));
        assert!(panicked.is_err());
    }
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    assert_eq!(heap.stats().allocation_growth_bytes, 0);
    let root = heap.access()?.alloc_leaf(42u64)?;
    assert_eq!(
        heap.access()?.with_object(&root, |view| *view.object())?,
        42
    );
    Ok(())
}

#[test]
fn failed_growth_preserves_contents_and_committed_charge() -> Result<(), GcError> {
    let mut heap = Heap::new(HeapConfig {
        max_indirect_bytes: 2 * size_of::<usize>(),
        ..HeapConfig::default()
    })?;
    let drops = Arc::new(AtomicUsize::new(0));
    let vector;
    let node;
    {
        let mut access = heap.access()?;
        node = Node::alloc(&mut access, 42, &drops)?;
        vector = ObjectVec::alloc(&mut access, &[node.clone(), node.clone()])?;
    }
    let bytes = heap.stats().indirect_bytes;
    let growth = heap.stats().allocation_growth_bytes;
    assert!(matches!(
        ObjectVec::push(&mut heap.access()?, &vector, &node),
        Err(GcError::Limit("indirect bytes"))
    ));
    assert_eq!(heap.stats().indirect_bytes, bytes);
    assert_eq!(heap.stats().allocation_growth_bytes, growth);
    assert_eq!(ObjectVec::values(&mut heap.access()?, &vector)?.len(), 2);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn a_pending_restart_survives_later_cursor_access_and_continue() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let vector = {
        let mut access = heap.access()?;
        let nodes = (0..96)
            .map(|value| Node::alloc(&mut access, value, &drops))
            .collect::<Result<Vec<_>, _>>()?;
        ObjectVec::alloc(&mut access, &nodes)?
    };
    let mut active = false;
    for _ in 0..1000 {
        heap.step(WorkBudget::new(16))?;
        if ObjectVec::cursor(&mut heap.access()?, &vector)?.is_some() {
            active = true;
            break;
        }
    }
    assert!(active);
    let cursor = ObjectVec::cursor(&mut heap.access()?, &vector)?;
    let scalar_unwound = catch_unwind(AssertUnwindSafe(|| {
        heap.scope(|cx| -> Result<(), GcError> {
            let vector = cx.get(&vector)?;
            let label = ObjectVec::LABEL.borrow_mut(cx, vector);
            *label = 41;
            panic!("application scalar edit failure");
        })
    }));
    assert!(scalar_unwound.is_err());
    // A stable scalar never invalidates a resumable scanner, even during unwind.
    assert_eq!(ObjectVec::cursor(&mut heap.access()?, &vector)?, cursor);
    let unwound = catch_unwind(AssertUnwindSafe(|| unsafe {
        heap.access()?
            .with_object_mut(&vector, |_, _| -> Result<(), GcError> {
                panic!("provider operation failure")
            })
    }));
    assert!(unwound.is_err());
    heap.scope(|cx| {
        let vector = cx.get(&vector)?;
        assert_eq!(ObjectVec::LABEL.get(cx, vector), 41);
        ObjectVec::LABEL.set(cx, vector, 42);
        Ok(())
    })?;
    // A diagnostic continuation must not make invalidated state reusable again.
    assert_eq!(ObjectVec::cursor(&mut heap.access()?, &vector)?, None);
    assert_eq!(ObjectVec::cursor(&mut heap.access()?, &vector)?, None);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(vector);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 96);
    Ok(())
}

#[test]
fn forgetting_a_mutable_loan_keeps_the_scanner_conservatively_invalidated() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let vector = {
        let mut access = heap.access()?;
        let nodes = (0..96)
            .map(|value| Node::alloc(&mut access, value, &drops))
            .collect::<Result<Vec<_>, _>>()?;
        ObjectVec::alloc(&mut access, &nodes)?
    };
    let mut active = false;
    for _ in 0..1000 {
        heap.step(WorkBudget::new(16))?;
        if ObjectVec::cursor(&mut heap.access()?, &vector)?.is_some() {
            active = true;
            break;
        }
    }
    assert!(active);
    heap.scope(|cx| {
        let vector = cx.get(&vector)?;
        // SAFETY: dropping edges changes no owned capacity; it needs a restart,
        // installed before the loan is exposed. No unrooted result escapes.
        let mut object = unsafe { cx.borrow_object_mut(vector) }?;
        unsafe {
            object.parts().0.clear();
        }
        std::mem::forget(object);
        Ok(())
    })?;
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 96);
    Ok(())
}

struct ScanFault {
    fail: Arc<AtomicBool>,
    panic: bool,
    drops: Arc<AtomicUsize>,
}

// SAFETY: edge-free contents use a resumable scanner solely to exercise its
// explicit failure boundary; no stale reference or invalid storage is constructed.
unsafe impl GcObject for ScanFault {
    type ScanState = ();

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        budget: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !budget.take(1) {
            return Ok(ScanStatus::Pending);
        }
        if self.fail.load(Ordering::Relaxed) {
            if self.panic {
                panic!("scan failure");
            }
            return Err(GcError::Provider("intentional scan failure"));
        }
        Ok(ScanStatus::Complete)
    }
}

impl Drop for ScanFault {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn scan_errors_and_panics_fault_without_sweeping_a_partial_mark() -> Result<(), GcError> {
    for panic in [false, true] {
        let drops = Arc::new(AtomicUsize::new(0));
        let fail = Arc::new(AtomicBool::new(false));
        let mut heap = Heap::default();
        let root = {
            let mut access = heap.access()?;
            let descriptor = access.register::<ScanFault>()?;
            unsafe {
                access.alloc_with(&descriptor, |_| {
                    Ok(ScanFault {
                        fail: fail.clone(),
                        panic,
                        drops: drops.clone(),
                    })
                })?
            }
        };
        // Fault injection changes no edges or charge. It deliberately violates the
        // no-panic scanner rule to test containment, not to assert provider soundness.
        fail.store(true, Ordering::Relaxed);
        assert!(heap.collect().is_err());
        assert_eq!(heap.stats().phase, Phase::Faulted);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        assert!(matches!(heap.access(), Err(GcError::Faulted)));
        let report = heap.shutdown();
        assert_eq!(report.disposed_objects, 1);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        drop(root);
    }
    Ok(())
}

struct Destructor {
    panic: bool,
    drops: Arc<AtomicUsize>,
}

// SAFETY: no edges, no mutable backing; the panic is an intentional destructor
// failure and the collector must consume initialized ownership before dispatch.
unsafe impl GcObject for Destructor {
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

impl Drop for Destructor {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
        assert!(!self.panic, "destructor failure");
    }
}

#[test]
fn destructor_panic_consumes_each_owner_once_and_cleanup_can_finish() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    {
        let mut access = heap.access()?;
        for index in 0..80 {
            drop(access.alloc_leaf(Destructor {
                panic: index == 12,
                drops: drops.clone(),
            })?);
        }
    }
    let mut fault = None;
    for _ in 0..1000 {
        let before = drops.load(Ordering::Relaxed);
        let result = heap.step(WorkBudget::NORMAL);
        assert!(drops.load(Ordering::Relaxed) - before <= 32);
        if let Err(error) = result {
            fault = Some(error);
            break;
        }
    }
    assert!(matches!(
        fault,
        Some(GcError::Provider("object destructor panicked"))
    ));
    assert_eq!(heap.stats().phase, Phase::Faulted);
    heap.shutdown();
    assert_eq!(drops.load(Ordering::Relaxed), 80);
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 80);
    Ok(())
}

struct ChargeFault {
    _destructor: Destructor,
    enabled: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

// SAFETY: no edges or indirect storage. The injected charge panic deliberately
// violates the provider contract solely to exercise quarantine and cleanup.
unsafe impl GcObject for ChargeFault {
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

    fn charge(&self) -> ObjectCharge {
        if self.enabled.load(Ordering::Relaxed) {
            assert_ne!(
                self.calls.fetch_add(1, Ordering::Relaxed),
                5,
                "charge fault after partial detach"
            );
        }
        ObjectCharge::default()
    }
}

#[test]
fn charge_panic_disposes_partial_retirement_before_releasing_its_pins() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let enabled = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    heap.scope(|cx| {
        for _ in 0..80 {
            cx.alloc_leaf(ChargeFault {
                _destructor: Destructor {
                    panic: false,
                    drops: drops.clone(),
                },
                enabled: enabled.clone(),
                calls: calls.clone(),
            })?;
        }
        Ok(())
    })?;
    enabled.store(true, Ordering::Relaxed);
    assert!(matches!(
        heap.collect(),
        Err(GcError::Provider("scanner or charge panicked"))
    ));
    assert_eq!(heap.stats().phase, Phase::Faulted);
    assert_eq!(drops.load(Ordering::Relaxed), 5);
    assert_eq!(heap.shutdown().disposed_objects, 75);
    assert_eq!(drops.load(Ordering::Relaxed), 80);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}
