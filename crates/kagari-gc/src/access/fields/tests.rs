//! Exercise contract rejection and fault quarantine without invalid addresses.

use crate::{
    access::{EdgeField, ScalarField},
    error::GcError,
    heap::{Heap, Phase},
    object::{Edge, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

struct Link(Option<Edge<u64>>);

// SAFETY: one fixed optional edge, changed only through NEXT, no indirect charge
// or custom destructor. State owns no references and the scan takes one unit.
unsafe impl GcObject for Link {
    type ScanState = ();

    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !budget.take(1) {
            return Ok(ScanStatus::Pending);
        }
        if let Some(edge) = &self.0 {
            // SAFETY: this object's live stored field is the protected source.
            return unsafe { visitor.try_visit_stored(edge, budget) };
        }
        Ok(ScanStatus::Complete)
    }
}

// SAFETY: matching projections of a scalar and a fixed, completely traced edge.
const VALUE: ScalarField<u64, u64> = unsafe { ScalarField::new(|v| v, |v| v) };
const NEXT: EdgeField<Link, u64> = unsafe { EdgeField::new(|v| &v.0, |v| &mut v.0) };

#[test]
fn application_panic_preserves_committed_scalar_write_and_heap_usability() -> Result<(), GcError> {
    let mut heap = Heap::default();
    heap.scope(|cx| {
        let local = cx.alloc_leaf(7u64)?;
        // An application panic propagates, but a scalar edit already committed
        // before it remains valid; no collector mutation needs quarantine.
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                *VALUE.borrow_mut(cx, local) = 8;
                cx.read(local, |_| panic!("application read panic"));
            }))
            .is_err()
        );
        assert_eq!(VALUE.get(cx, local), 8);
        assert_eq!(cx.access.heap.stats().phase, Phase::Idle);
        Ok(())
    })?;
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    Ok(())
}

#[test]
fn publication_failure_faults_before_panicking_and_preserves_the_old_edge() -> Result<(), GcError> {
    let mut heap = Heap::default();
    heap.scope(|cx| {
        let old = cx.alloc_leaf(1u64)?;
        let incoming = cx.alloc_leaf(2u64)?;
        // SAFETY: initialized None contains no edges to import or trace.
        let owner = unsafe { cx.alloc_with(|_| Ok(Link(None)))? };
        NEXT.set(cx, owner, Some(old));
        // Inject exhaustion of non-admission accounting, using real allocated
        // white objects and the actual publication path. No address is retired.
        cx.access.heap.marking.start()?;
        cx.access.heap.marking.publication_work = usize::MAX;
        assert!(catch_unwind(AssertUnwindSafe(|| NEXT.set(cx, owner, Some(incoming)))).is_err());
        assert_eq!(cx.access.heap.stats().phase, Phase::Faulted);
        assert!(catch_unwind(AssertUnwindSafe(|| NEXT.get(cx, owner))).is_err());
        // Public access correctly rejects the faulted heap. Inspect the still-live
        // allocation internally to verify that the rejected store did not commit.
        let object = cx.access.heap.physical.resolve(owner.reference)?;
        // SAFETY: exact Link allocation above, no collection or mutable loan here.
        let link = unsafe { object.pointer.cast::<Link>().as_ref() };
        assert_eq!(link.0.as_ref().unwrap().reference, old.reference);
        Ok(())
    })?;
    assert!(matches!(heap.collect(), Err(GcError::Faulted)));
    assert_eq!(heap.shutdown().disposed_objects, 3);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}
