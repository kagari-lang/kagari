//! Exercise contract rejection and fault quarantine without invalid addresses.

use crate::{
    access::{EdgeField, ScalarField},
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{Heap, Phase},
    object::{Edge, GcObject, ObjectRef},
    root_source::{RootDriver, RootSource},
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

struct EmptyRoots;

// SAFETY: the source never installs any edges; its scan is immediately complete.
unsafe impl RootSource for EmptyRoots {
    type Cursor = ();

    fn empty() -> Self {
        Self
    }

    fn begin_scan(&self) -> Result<(), GcError> {
        Ok(())
    }

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }
}

#[derive(Clone, Copy)]
enum PublicationPath {
    Typed,
    Dynamic,
    Source,
}

#[test]
fn publication_failure_quarantines_all_write_entries_and_preserves_old_edges() -> Result<(), GcError>
{
    for path in [
        PublicationPath::Typed,
        PublicationPath::Dynamic,
        PublicationPath::Source,
    ] {
        let mut driver = RootDriver::<EmptyRoots>::new(Default::default())?;
        let mut builder = DynamicLayoutBuilder::new();
        let next = builder.reference();
        let layout = driver.access()?.register_dynamic(builder)?;
        let next = layout.reference(next)?;
        driver.scope(|cx, _| {
            let old = cx.alloc_leaf(1u64)?;
            let incoming = cx.alloc_leaf(2u64)?;
            // SAFETY: initialized None contains no edges to import or trace.
            let owner = unsafe { cx.alloc_with(|_| Ok(Link(None)))? };
            let record = cx.alloc_dynamic(&layout)?;
            let rooted_owner = cx.retain(owner)?;
            NEXT.set(cx, owner, Some(old));
            next.set(cx, record, Some(old.erase()))?;
            // Exhaust the real marking account with allocated white targets.
            cx.access.heap.marking.start()?;
            cx.access.heap.marking.publication_work = usize::MAX;
            match path {
                PublicationPath::Typed => {
                    assert!(
                        catch_unwind(AssertUnwindSafe(|| NEXT.set(cx, owner, Some(incoming))))
                            .is_err()
                    );
                }
                PublicationPath::Dynamic => {
                    assert_eq!(
                        next.set(cx, record, Some(incoming.erase())),
                        Err(GcError::Limit("publication work"))
                    );
                }
                PublicationPath::Source => {
                    // SAFETY: current driver's paired scope; no carrier is returned
                    // on failure and therefore no source slot can be committed.
                    assert!(matches!(
                        unsafe { cx.source_edge(incoming) },
                        Err(GcError::Limit("publication work"))
                    ));
                }
            }
            assert_eq!(cx.access.heap.stats().phase, Phase::Faulted);
            assert!(catch_unwind(AssertUnwindSafe(|| NEXT.get(cx, owner))).is_err());
            assert!(
                catch_unwind(AssertUnwindSafe(|| NEXT.set(cx, owner, Some(incoming)))).is_err()
            );
            assert!(catch_unwind(AssertUnwindSafe(|| NEXT.set(cx, owner, None))).is_err());
            assert_eq!(
                next.set(cx, record, Some(incoming.erase())),
                Err(GcError::Faulted)
            );
            assert_eq!(next.set(cx, record, None), Err(GcError::Faulted));
            assert!(matches!(next.get(cx, record), Err(GcError::Faulted)));
            // SAFETY: paired driver scope; rejection returns no carrier to install.
            assert!(matches!(
                unsafe { cx.source_edge(incoming) },
                Err(GcError::Faulted)
            ));
            // These public entrypoints share the private mutable-loan kernel.
            // Reject before charging the provider or entering an edit callback.
            let mut entered = false;
            // SAFETY: exact scoped Link; any admitted loan would be used only
            // through its provider contract, with no escaping reference/carrier.
            assert!(matches!(
                unsafe { cx.borrow_object_mut(owner) },
                Err(GcError::Faulted)
            ));
            // SAFETY: callbacks only set an unrelated test flag, preserving T.
            assert!(matches!(
                unsafe {
                    cx.with_object_mut(owner, |_, _| {
                        entered = true;
                        Ok(())
                    })
                },
                Err(GcError::Faulted)
            ));
            // SAFETY: same read-only provider callback through an existing Access.
            assert!(matches!(
                unsafe {
                    cx.access.with_object_mut(&rooted_owner, |_, _| {
                        entered = true;
                        Ok(())
                    })
                },
                Err(GcError::Faulted)
            ));
            assert!(!entered);
            let heap = &mut cx.access.heap;
            assert_eq!(
                heap.marking
                    .publish(&heap.physical, incoming.reference, None),
                Err(GcError::Faulted)
            );
            // Inspect only still-live storage internally after public rejection.
            let object = heap.physical.resolve(owner.reference)?;
            // SAFETY: exact Link allocation, no collection or mutable loan here.
            let link = unsafe { object.pointer.cast::<Link>().as_ref() };
            assert_eq!(link.0.as_ref().unwrap().reference, old.reference);
            let object = heap.physical.resolve(record.reference)?;
            let offset = object.descriptor.shape()?.references[0];
            // SAFETY: this exact initialized layout has one optional reference;
            // the fault does not retire storage or change representation.
            let stored = unsafe {
                object
                    .pointer
                    .as_ptr()
                    .add(offset)
                    .cast::<Option<ObjectRef>>()
                    .read()
            };
            assert_eq!(stored, Some(old.reference));
            Ok(())
        })?;
        assert!(matches!(driver.collect(), Err(GcError::Faulted)));
        assert_eq!(driver.shutdown().disposed_objects, 4);
        assert_eq!(driver.stats().backing_bytes, 0);
    }
    Ok(())
}
