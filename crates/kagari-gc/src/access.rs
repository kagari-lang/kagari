//! Scoped object loans, protected construction and provider mutation services.

mod fields;
pub mod inspection;
mod mutation;
mod scope;

use crate::{
    allocation::{PhysicalHeap, Reservation},
    descriptor::{Descriptor, ScanWorkspace, TypedDescriptor},
    error::GcError,
    heap::{Heap, Phase, accounting::MemoryLedger},
    marking::{ActiveScan, Marking},
    object::{Edge, Gc, GcObject, ObjectCharge, ObjectRef},
    root::{Root, RootSet, Roots},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, VisitTarget},
};
use std::{
    any::TypeId,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

/// Low-level rooted/provider services in an exclusive heap interval. Ordinary
/// object operations use Heap::scope; both share allocation and mutation kernels.
/// Release the interval before collecting or invoking reentrant host callbacks.
pub struct Access<'heap> {
    pub(crate) heap: &'heap mut Heap,
    pub(crate) non_transferable: PhantomData<Rc<()>>,
}

/// Read-only object data and services to retain a child before its source expires.
pub struct View<'read, T: GcObject> {
    object: &'read T,
    retention: Retention<'read>,
}

impl<'read, T: GcObject> View<'read, T> {
    pub fn object(&self) -> &'read T {
        self.object
    }

    /// Read a stored target within this collection-excluded callback, without
    /// allocating a Root or publishing marks. The edge's source remains borrowed
    /// for the read lifetime; admission validates the owning heap and target type.
    /// Returned references cannot escape with_object or overlap mutable heap access.
    ///
    /// # Panics
    /// Panics before access on foreign edges or inconsistent representation/storage.
    ///
    /// ```compile_fail
    /// use kagari_gc::{access::Access, object::{Edge, GcObject}, root::Root};
    /// fn escape<'a, T: GcObject, U: GcObject>(
    ///     access: &'a mut Access<'_>, root: &Root<T>,
    ///     select: for<'s> fn(&'s T) -> &'s Edge<U>,
    /// ) -> &'a U {
    ///     access.with_object(root, |view| {
    ///         view.follow(select(view.object()))
    ///     }).unwrap()
    /// }
    /// ```
    pub fn follow<U: GcObject>(&self, edge: &'read Edge<U>) -> &'read U {
        let object = self
            .retention
            .physical
            .resolve(edge.reference)
            .expect("selected edge must name live storage in this heap");
        assert!(
            object.descriptor.matches::<U>(),
            "stored edge type mismatch"
        );
        // SAFETY: the borrowed edge's admitted source proves a live target. Its
        // original page and exact type were checked before forming U. The exclusive
        // read interval excludes collection/mutation for the entire returned loan.
        unsafe { object.pointer.cast::<U>().as_ref() }
    }

    /// Create an owning child root for use after this callback or across mutation.
    /// For read-only traversal inside the callback, use follow instead.
    pub fn retain<U: 'static>(&mut self, edge: &Edge<U>) -> Result<Root<U>, GcError> {
        self.retention.retain(edge.reference)
    }
}

/// Raw initialization services available only inside unsafe provider construction.
pub struct Init<'init> {
    physical: &'init PhysicalHeap,
    roots: &'init Roots,
}

impl Init<'_> {
    /// Import a live scoped handle into this initializer's traced result.
    ///
    /// # Safety
    /// As with edge, do not let the resulting owned carrier escape initialization.
    /// The handle's scope keeps its target allocated while this import is checked.
    pub unsafe fn edge_scoped<T: 'static>(
        &mut self,
        source: Gc<'_, T>,
    ) -> Result<Edge<T>, GcError> {
        self.physical.resolve(source.reference)?;
        Ok(Edge {
            reference: source.reference,
            marker: PhantomData,
        })
    }

    /// Import a protected source without cloning its payload or registering a root.
    ///
    /// # Safety
    /// Store the returned carrier only in this initializer's traced result; do not
    /// expose owned edges or keep escaped carriers for later unprotected admission.
    pub unsafe fn edge<T: 'static>(&mut self, source: &Root<T>) -> Result<Edge<T>, GcError> {
        let reference = typed_root(self.physical, self.roots, source)?;
        Ok(Edge {
            reference,
            marker: PhantomData,
        })
    }
}

struct Retention<'a> {
    physical: &'a PhysicalHeap,
    roots: &'a mut Roots,
    marking: &'a mut Marking,
    max_roots: usize,
}

impl Retention<'_> {
    fn retain<T: 'static>(&mut self, reference: ObjectRef) -> Result<Root<T>, GcError> {
        let object = self.physical.resolve(reference)?;
        if !object.descriptor.matches::<T>() {
            return Err(GcError::Representation);
        }
        let prepared = self
            .roots
            .prepare(1, Some(TypeId::of::<T>()), self.max_roots)?;
        self.marking.publish(self.physical, reference, None)?;
        Ok(Root {
            lease: self.roots.commit(prepared, Some(reference)),
            marker: PhantomData,
        })
    }
}

/// Receiver-bound provider services; creating one requires unsafe object admission.
pub struct Mutation<'op, T: GcObject> {
    retention: Retention<'op>,
    owner: ObjectRef,
    active: &'op mut Option<ActiveScan>,
    workspace: &'op mut ScanWorkspace,
    ledger: &'op mut MemoryLedger,
    old_charge: ObjectCharge,
    admitted_bytes: usize,
    max_indirect: usize,
    continuation: bool,
    scan_reusable: bool,
    force_restart: bool,
    marker: PhantomData<fn() -> T>,
}

/// A lending exclusive object loan. Providers must restrict edits to their checked
/// field/container API. Drop finalizes accounting; safe wrappers must remain valid
/// when forgotten and cannot rely on Drop to repair memory-safety invariants.
pub struct ObjectMut<'op, T: GcObject> {
    object: &'op mut T,
    mutation: Mutation<'op, T>,
    finished: bool,
    charge_state: ChargeState,
    panicking_on_entry: bool,
}

enum ChargeState {
    Dirty,
    Committed,
    Failed,
}

/// A generative, noncollecting object-access interval created by Heap::scope.
/// Handles may coexist and survive edits. Short shared reads may nest; mutable
/// payload loans require exclusive access. Retain a handle or store it in a root
/// pack before leaving the scope.
pub struct Scope<'gc> {
    pub(crate) access: Access<'gc>,
    pub(crate) invariant: PhantomData<fn(&'gc ()) -> &'gc ()>,
}

/// A provider-verified scalar field with stable tracing and accounting.
pub struct ScalarField<T: GcObject, V: Copy> {
    read: fn(&T) -> &V,
    write: fn(&mut T) -> &mut V,
}

/// A provider-verified optional managed-reference field at a fixed scan position.
pub struct EdgeField<T: GcObject, U: 'static> {
    read: fn(&T) -> &Option<Edge<U>>,
    write: fn(&mut T) -> &mut Option<Edge<U>>,
}

impl<T: GcObject> Mutation<'_, T> {
    /// Publish a scoped source for this operation's traced destination.
    ///
    /// # Safety
    /// Store the returned carrier only in valid traced storage for this operation.
    /// No owned unrooted carrier may escape to a later access interval.
    pub unsafe fn edge_scoped<U: 'static>(
        &mut self,
        source: Gc<'_, U>,
    ) -> Result<Edge<U>, GcError> {
        self.retention
            .marking
            .publish(self.retention.physical, source.reference, None)?;
        Ok(Edge {
            reference: source.reference,
            marker: PhantomData,
        })
    }

    /// Import a Root for this operation's traced destination.
    ///
    /// # Safety
    /// Do not let the owned carrier escape to untraced storage or a future operation.
    pub unsafe fn edge<U: 'static>(&mut self, source: &Root<U>) -> Result<Edge<U>, GcError> {
        let reference = typed_root(self.retention.physical, self.retention.roots, source)?;
        self.retention
            .marking
            .publish(self.retention.physical, reference, None)?;
        Ok(Edge {
            reference,
            marker: PhantomData,
        })
    }

    /// Retain before destructive removal.
    ///
    /// # Safety
    /// The edge is still protected by the admitted same-heap object or traced scratch.
    pub unsafe fn retain_existing<U: 'static>(
        &mut self,
        edge: &Edge<U>,
    ) -> Result<Root<U>, GcError> {
        self.retention.retain(edge.reference)
    }

    /// Protect a relocated target when the provider cannot repair scan coordinates.
    ///
    /// # Safety
    /// Keep the edge's admitted traced source alive until publication completes.
    pub unsafe fn publish_existing<U: 'static>(&mut self, edge: &Edge<U>) -> Result<(), GcError> {
        self.retention
            .marking
            .publish(self.retention.physical, edge.reference, None)?;
        Ok(())
    }

    /// Check the actual planned owned capacity before installing grown storage.
    /// Prepare staged capacity while old edges remain traced; account allocator
    /// rounding using the staged allocation's real capacity, then commit without failure.
    pub fn prepare_charge(&mut self, charge: ObjectCharge) -> Result<(), GcError> {
        self.ledger
            .check(self.old_charge.bytes, charge.bytes, self.max_indirect)?;
        self.ledger
            .check_units(self.old_charge.units, charge.units)?;
        self.admitted_bytes = charge.bytes;
        Ok(())
    }

    /// Lend this receiver's reusable active state for a closed provider commit.
    /// None covers absent/other-owner state and already pending Restart.
    ///
    /// # Safety
    /// Preserve initialized pointer-free state and object storage on every exit.
    /// Repair and mutation must remain non-reentrant and preserve complete future
    /// visitation or already-published coverage. Do not run allocating callbacks.
    ///
    /// ```compile_fail
    /// use kagari_gc::{access::Access, root::Root};
    /// unsafe fn leak<'a>(access: &'a mut Access<'_>, root: &Root<u64>) -> &'a mut () {
    ///     unsafe {
    ///         access.with_object_mut(root, |_, mutation| {
    ///             Ok(mutation.with_scan_state(|state| state.unwrap()))
    ///         }).unwrap()
    ///     }
    /// }
    /// ```
    pub unsafe fn with_scan_state<R>(
        &mut self,
        operation: impl for<'scan> FnOnce(Option<&'scan mut T::ScanState>) -> R,
    ) -> R {
        let reusable = self
            .active
            .as_ref()
            .is_some_and(|active| active.owner == self.owner)
            && self.scan_reusable
            && !self.force_restart;
        let state = if reusable {
            // SAFETY: with_object_mut checked owner, descriptor and registered state
            // before lending T. Workspace is disjoint and contains this initialized state.
            Some(unsafe { &mut *self.workspace.pointer().cast::<T::ScanState>().as_ptr() })
        } else {
            None
        };
        operation(state)
    }

    /// Declare that all edits preserve scan continuation; never clears prior Restart.
    ///
    /// # Safety
    /// Every later edit must preserve the same cursor-validity or coverage proof.
    pub unsafe fn continue_scan(&mut self) {
        self.continuation = true;
    }

    fn invalidate(&mut self) {
        self.force_restart = true;
        if let Some(active) = self.active.as_mut()
            && active.owner == self.owner
        {
            active.restart = true;
        }
    }
}

impl Access<'_> {
    /// Register the physical Rust type and reserve reusable scanner state capacity.
    pub fn register<T: GcObject>(&mut self) -> Result<TypedDescriptor<T>, GcError> {
        self.heap.marking.check()?;
        let descriptor = self.heap.physical.descriptors.register::<T>()?;
        self.heap.workspace.reserve(descriptor.state_layout)?;
        Ok(TypedDescriptor {
            heap: self.heap.id,
            descriptor,
            marker: PhantomData,
        })
    }

    /// Safely admit a value whose unsafe GcObject contract declares no managed edges.
    /// Reference-bearing preassembled values are rejected; use a reviewed constructor.
    pub fn alloc_leaf<T: GcObject>(&mut self, value: T) -> Result<Root<T>, GcError> {
        if T::SCAN_KIND != ScanKind::EdgeFree {
            return Err(GcError::Provider("alloc_leaf requires EdgeFree"));
        }
        let descriptor = self.register::<T>()?;
        // SAFETY: the representation contract excludes managed edges in every T value.
        unsafe { self.alloc_with(&descriptor, |_| Ok(value)) }
    }

    /// Construct through protected sources and return the first owning root.
    ///
    /// # Safety
    /// The initializer returns a fully scan-valid T. All stored edges come from
    /// live sources in this heap and remain protected until publication. Neither
    /// Init nor owned edges may escape into an ordinary safe construction path.
    /// Initialization and destruction must honor GcObject's validity contract.
    ///
    /// ```compile_fail
    /// use kagari_gc::heap::Heap;
    /// let mut heap = Heap::default();
    /// let mut access = heap.access().unwrap();
    /// let descriptor = access.register::<u64>().unwrap();
    /// // Raw construction requires an explicit unsafe provider boundary.
    /// let root = access.alloc_with(&descriptor, |_| Ok(42u64)).unwrap();
    /// ```
    pub unsafe fn alloc_with<T: GcObject>(
        &mut self,
        descriptor: &TypedDescriptor<T>,
        initializer: impl for<'init> FnOnce(Init<'init>) -> Result<T, GcError>,
    ) -> Result<Root<T>, GcError> {
        self.check_allocation(descriptor.heap, &descriptor.descriptor)?;
        let prepared =
            self.heap
                .roots
                .prepare(1, Some(TypeId::of::<T>()), self.heap.config.max_roots)?;
        let reference = self.allocate(descriptor, initializer)?;
        Ok(Root {
            lease: self.heap.roots.commit(prepared, Some(reference)),
            marker: PhantomData,
        })
    }

    pub(crate) fn check_allocation(
        &self,
        heap: u64,
        descriptor: &Arc<Descriptor>,
    ) -> Result<(), GcError> {
        self.heap.marking.check()?;
        if heap != self.heap.id {
            return Err(GcError::ForeignHeap);
        }
        if !Arc::ptr_eq(
            self.heap.physical.descriptors.get(descriptor.id)?,
            descriptor,
        ) {
            return Err(GcError::Representation);
        }
        if self.heap.physical.objects >= self.heap.config.max_objects {
            return Err(GcError::Limit("objects"));
        }
        Ok(())
    }

    // Both destinations use the same reservation, initialization, publication and
    // accounting protocol. Callers admit the descriptor and protect the result by
    // a prepared host root or the enclosing noncollecting scope.
    fn allocate<T: GcObject>(
        &mut self,
        descriptor: &TypedDescriptor<T>,
        initializer: impl for<'init> FnOnce(Init<'init>) -> Result<T, GcError>,
    ) -> Result<ObjectRef, GcError> {
        // SAFETY: the initializer writes one complete T only after its fallible
        // constructor succeeds. Validation scans/charges that same initialized T.
        unsafe {
            self.allocate_initialized(
                &descriptor.descriptor,
                |pointer, physical, roots| {
                    let value = initializer(Init { physical, roots })?;
                    pointer.cast::<T>().as_ptr().write(value);
                    Ok(())
                },
                |pointer, physical, marking| {
                    let value = pointer.cast::<T>().as_ref();
                    inspect_initial(value, physical, marking)?;
                    Ok(())
                },
            )
        }
    }

    /// Shared physical transaction. The admitted descriptor must match storage;
    /// initialize writes a complete object before Ok and owns rollback of any
    /// partial writes on error/panic. Validate cannot mutate initialized storage.
    pub(crate) unsafe fn allocate_initialized(
        &mut self,
        descriptor: &Arc<Descriptor>,
        initialize: impl FnOnce(NonNull<u8>, &PhysicalHeap, &Roots) -> Result<(), GcError>,
        validate: impl FnOnce(NonNull<u8>, &PhysicalHeap, &mut Marking) -> Result<(), GcError>,
    ) -> Result<ObjectRef, GcError> {
        let Heap {
            physical,
            roots,
            marking,
            ledger,
            config,
            ..
        } = self.heap;
        marking.reserve_pages(
            physical
                .page_count()
                .checked_add(1)
                .ok_or(GcError::Limit("page jobs"))?,
        )?;
        let reservation = physical.reserve(descriptor, config.max_backing_bytes)?;
        let mut guard = AllocationGuard {
            physical,
            reservation,
            descriptor: descriptor.clone(),
            initialized: false,
            committed: false,
        };
        initialize(guard.pointer(), guard.physical, roots)?;
        guard.initialized = true;
        let publication = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: initialize established the descriptor's complete payload.
            let charge = unsafe { descriptor.charge(guard.pointer()) };
            let total = ledger.check(0, charge.bytes, config.max_indirect_bytes)?;
            let units = ledger.check_units(0, charge.units)?;
            validate(guard.pointer(), guard.physical, marking)?;
            Ok::<_, GcError>((total, units))
        }));
        let (total, units) = match publication {
            Ok(result) => result?,
            Err(_) => {
                marking.phase = Phase::Faulted;
                return Err(GcError::Provider("initial scan or charge panicked"));
            }
        };
        marking.admit(guard.physical, reservation)?;
        guard.physical.commit(reservation, descriptor.id)?;
        guard.committed = true;
        ledger.grew(
            descriptor
                .layout
                .size()
                .saturating_add(total - ledger.indirect),
        );
        ledger.indirect = total;
        ledger.units = units;
        ledger.observe(guard.physical);
        Ok(reservation.reference)
    }

    /// Borrow object data for a callback whose result cannot contain that loan.
    ///
    /// ```compile_fail
    /// use kagari_gc::heap::Heap;
    /// let mut heap = Heap::default();
    /// let mut access = heap.access().unwrap();
    /// let root = access.alloc_leaf(42u64).unwrap();
    /// let escaped = access.with_object(&root, |view| view.object()).unwrap();
    /// println!("{escaped}");
    /// ```
    pub fn with_object<T: GcObject, R>(
        &mut self,
        root: &Root<T>,
        reader: impl for<'read> FnOnce(View<'read, T>) -> R,
    ) -> Result<R, GcError> {
        self.heap.marking.check()?;
        let reference = self.heap.roots.target(root)?;
        self.with_reference::<T, R>(reference, reader)
    }

    // Callers establish a live same-heap T through an admitted Root<T> or Gc<T>.
    // Root construction validates/fixes the target type; scoped handles carry
    // the same proof. No current operation changes that representation in place.
    fn with_reference<T: GcObject, R>(
        &mut self,
        reference: ObjectRef,
        reader: impl for<'read> FnOnce(View<'read, T>) -> R,
    ) -> Result<R, GcError> {
        let object = self.heap.physical.resolve(reference)?;
        debug_assert!(object.descriptor.matches::<T>());
        let pointer = object.pointer;
        let Heap {
            physical,
            roots,
            marking,
            config,
            ..
        } = self.heap;
        // SAFETY: admitted live typed source and collection-excluded shared loan.
        let object = unsafe { pointer.cast::<T>().as_ref() };
        Ok(reader(View {
            object,
            retention: Retention {
                physical,
                roots,
                marking,
                max_roots: config.max_roots,
            },
        }))
    }

    /// Mutate only through a reviewed provider that owns tracing/accounting invariants.
    ///
    /// # Safety
    /// Keep T valid, publish incoming references before exposing them, retain removed
    /// results before release, preflight capacity growth and prove cursor continuation
    /// or allow Restart. Do not expose arbitrary mutable T to safe clients. Every
    /// unwind leaves valid object storage, even if logical changes already occurred.
    pub unsafe fn with_object_mut<T: GcObject, R>(
        &mut self,
        owner: &Root<T>,
        operation: impl for<'op> FnOnce(&'op mut T, &mut Mutation<'op, T>) -> Result<R, GcError>,
    ) -> Result<R, GcError> {
        self.heap.marking.check()?;
        let reference = self.heap.roots.target(owner)?;
        self.with_reference_mut(reference, operation)
    }

    fn with_reference_mut<T: GcObject, R>(
        &mut self,
        reference: ObjectRef,
        operation: impl for<'op> FnOnce(&'op mut T, &mut Mutation<'op, T>) -> Result<R, GcError>,
    ) -> Result<R, GcError> {
        let mut object = self.borrow_object_mut::<T>(reference)?;
        let result = object.apply(operation);
        if !matches!(&result, Ok(Ok(_))) {
            object.mutation.invalidate();
        }
        let final_error = object.finish().err();
        match result {
            Err(panic) => resume_unwind(panic),
            Ok(result) => match final_error {
                Some(error) => Err(error),
                None => result,
            },
        }
    }

    /// Register a fixed-cardinality empty root pack.
    pub fn create_roots(&mut self, slots: usize) -> Result<RootSet, GcError> {
        self.heap.marking.check()?;
        let prepared = self
            .heap
            .roots
            .prepare(slots, None, self.heap.config.max_roots)?;
        Ok(RootSet {
            lease: self.heap.roots.commit(prepared, None),
            len: slots,
        })
    }

    /// Publish before replacing a root slot, including slots behind a yielded cursor.
    pub fn replace_root<T: 'static>(
        &mut self,
        pack: &RootSet,
        index: usize,
        source: &Root<T>,
    ) -> Result<(), GcError> {
        self.heap.marking.check()?;
        self.heap.roots.replace_with(pack, index, |roots| {
            let target = typed_root(&self.heap.physical, roots, source)?;
            self.heap
                .marking
                .publish(&self.heap.physical, target, None)?;
            Ok(Some(target))
        })
    }

    pub fn clear_root(&mut self, pack: &RootSet, index: usize) -> Result<(), GcError> {
        self.heap.marking.check()?;
        self.heap.roots.replace_with(pack, index, |_| Ok(None))
    }

    /// Retain a typed pack slot as an independent immutable owning root.
    pub fn root_at<T: 'static>(
        &mut self,
        pack: &RootSet,
        index: usize,
    ) -> Result<Root<T>, GcError> {
        self.heap.marking.check()?;
        let entry = self.heap.roots.admit(&pack.lease)?;
        if entry.typed.is_some() {
            return Err(GcError::Representation);
        }
        let reference = entry
            .targets
            .get(index)
            .copied()
            .flatten()
            .ok_or(GcError::Index)?;
        Retention {
            physical: &self.heap.physical,
            roots: &mut self.heap.roots,
            marking: &mut self.heap.marking,
            max_roots: self.heap.config.max_roots,
        }
        .retain(reference)
    }
}

fn typed_root<T: 'static>(
    physical: &PhysicalHeap,
    roots: &Roots,
    root: &Root<T>,
) -> Result<ObjectRef, GcError> {
    let reference = roots.target(root)?;
    if !physical.resolve(reference)?.descriptor.matches::<T>() {
        return Err(GcError::Representation);
    }
    Ok(reference)
}

fn inspect_initial<T: GcObject>(
    value: &T,
    physical: &PhysicalHeap,
    marking: &mut Marking,
) -> Result<(), GcError> {
    let mut state = value.begin_scan();
    loop {
        let mut budget = ScanBudget::new(1024);
        let mut visitor = EdgeVisitor {
            physical,
            target: VisitTarget::Mark(marking),
        };
        let status = value.scan(&mut state, &mut budget, &mut visitor)?;
        if T::SCAN_KIND != ScanKind::Indivisible && budget.spent() > 1024 {
            return Err(GcError::Provider("initial scan exceeded allowance"));
        }
        if matches!(T::SCAN_KIND, ScanKind::Bounded { max_work } if status != ScanStatus::Complete || budget.spent() > max_work * 5)
        {
            return Err(GcError::Provider("initial bounded scan violated contract"));
        }
        if status == ScanStatus::Complete {
            return Ok(());
        }
        if budget.spent() == 0 {
            return Err(GcError::Provider("initial scan made no progress"));
        }
    }
}

struct AllocationGuard<'a> {
    physical: &'a mut PhysicalHeap,
    reservation: Reservation,
    descriptor: Arc<Descriptor>,
    initialized: bool,
    committed: bool,
}

impl AllocationGuard<'_> {
    fn pointer(&self) -> NonNull<u8> {
        // SAFETY: checked descriptor offset inside the reserved object's layout.
        unsafe {
            self.reservation
                .reference
                .0
                .cast::<u8>()
                .add(self.descriptor.payload_offset)
        }
    }
}

impl Drop for AllocationGuard<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if self.initialized {
            self.initialized = false;
            // SAFETY: this ticket uniquely owns initialized but unpublished T.
            let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
                self.descriptor.destroy(self.pointer())
            }));
        }
        self.physical
            .rollback(self.reservation)
            .expect("linear allocation rollback");
    }
}
