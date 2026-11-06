//! Scoped identities share the same read, mutation, publication and root owners.

use crate::{
    access::{Init, Mutation, ObjectMut, Retention, Scope, typed_root},
    descriptor::TypedDescriptor,
    error::GcError,
    heap::Phase,
    object::{AnyObject, Edge, Gc, GcObject, ObjectHeader, ObjectRef},
    root::{Root, RootSet},
    scan::ScanKind,
};
use std::{marker::PhantomData, ptr::NonNull};

impl<'gc> Scope<'gc> {
    /// Allocate an edge-free value without a host-root registration. The result
    /// lives through this scope; retain/store it explicitly to survive collection.
    pub fn alloc_leaf<T: GcObject>(&mut self, value: T) -> Result<Gc<'gc, T>, GcError> {
        if T::SCAN_KIND != ScanKind::EdgeFree {
            return Err(GcError::Provider("alloc_leaf requires EdgeFree"));
        }
        // SAFETY: the representation excludes managed edges in every value.
        unsafe { self.alloc_with(|_| Ok(value)) }
    }

    /// Construct a provider object protected by this scope instead of a new Root.
    /// Type registration and the allocation kernel are shared with rooted allocation.
    ///
    /// # Safety
    /// Honor Access::alloc_with's initialized storage, source protection, complete
    /// tracing and nonescaping carrier contract. Init::edge_scoped imports handles.
    pub unsafe fn alloc_with<T: GcObject>(
        &mut self,
        initializer: impl for<'init> FnOnce(Init<'init>) -> Result<T, GcError>,
    ) -> Result<Gc<'gc, T>, GcError> {
        let descriptor = self.access.register::<T>()?;
        self.access
            .check_allocation(descriptor.heap, &descriptor.descriptor)?;
        let reference = self.access.allocate(&descriptor, initializer)?;
        Ok(self.handle(reference))
    }

    /// Admit a host root once, without cloning its lease or registering a new root.
    pub fn get<T: 'static>(&self, root: &Root<T>) -> Result<Gc<'gc, T>, GcError> {
        self.access.heap.marking.check()?;
        let reference = typed_root(&self.access.heap.physical, &self.access.heap.roots, root)?;
        Ok(self.handle(reference))
    }

    /// Admit a frame/root-pack slot directly; no intermediate host Root is created.
    pub fn get_root<T: 'static>(
        &self,
        pack: &RootSet,
        index: usize,
    ) -> Result<Gc<'gc, T>, GcError> {
        self.access.heap.marking.check()?;
        let entry = self.access.heap.roots.admit(&pack.lease)?;
        let reference = entry
            .targets
            .get(index)
            .copied()
            .flatten()
            .ok_or(GcError::Index)?;
        if !self
            .access
            .heap
            .physical
            .resolve(reference)?
            .descriptor
            .matches::<T>()
        {
            return Err(GcError::Representation);
        }
        Ok(self.handle(reference))
    }

    /// Publish a scoped target into an existing root slot, or clear that slot.
    /// Previously acquired handles remain valid until this scope ends.
    pub fn set_root<T: 'static>(
        &mut self,
        pack: &RootSet,
        index: usize,
        value: Option<Gc<'gc, T>>,
    ) -> Result<(), GcError> {
        self.access.heap.marking.check()?;
        let heap = &mut self.access.heap;
        heap.roots.replace_with(pack, index, |_| {
            let reference = value.map(|value| value.reference);
            if let Some(reference) = reference {
                heap.marking.publish(&heap.physical, reference, None)?;
            }
            Ok(reference)
        })
    }

    /// Keep an object across the scope boundary by creating an owning host root.
    pub fn retain<T: 'static>(&mut self, object: Gc<'gc, T>) -> Result<Root<T>, GcError> {
        let reference = self.check(object)?;
        let heap = &mut self.access.heap;
        Retention {
            physical: &heap.physical,
            roots: &mut heap.roots,
            marking: &mut heap.marking,
            max_roots: heap.config.max_roots,
        }
        .retain(reference)
    }

    /// Read owned results through a shared Scope loan. Read callbacks may nest,
    /// including reads through aliases of the same object. No payload reference
    /// can escape the callback or overlap a mutable Scope operation.
    /// Returns the callback result directly, including any application error.
    ///
    /// # Panics
    /// Panics before access if the heap is unavailable or admitted storage violates
    /// its contract. The invariant scope brand excludes foreign handles at compile
    /// time. Callback panics propagate.
    ///
    /// ```
    /// use kagari_gc::{error::GcError, heap::Heap};
    /// let mut heap = Heap::default();
    /// heap.scope(|cx| {
    ///     let a = cx.alloc_leaf(20u64)?;
    ///     let b = cx.alloc_leaf(22u64)?;
    ///     let shared = &*cx;
    ///     let sum = shared.read(a, |a| shared.read(b, |b| *a + *b));
    ///     assert_eq!(sum, 42);
    ///     Ok::<_, GcError>(())
    /// }).unwrap();
    /// ```
    ///
    /// ```compile_fail
    /// use kagari_gc::heap::Heap;
    /// let mut heap = Heap::default();
    /// let root = heap.access().unwrap().alloc_leaf(1u64).unwrap();
    /// heap.scope(|cx| {
    ///     let object = cx.get(&root)?;
    ///     let escaped = cx.read(object, |value| value);
    ///     println!("{escaped}");
    ///     Ok(())
    /// }).unwrap();
    /// ```
    ///
    /// Shared callbacks cannot write through another handle to the object.
    ///
    /// ```compile_fail,E0500
    /// use kagari_gc::{access::ScalarField, heap::Heap};
    /// const VALUE: ScalarField<u64, u64> =
    ///     unsafe { ScalarField::new(|v| v, |v| v) };
    /// let mut heap = Heap::default();
    /// heap.scope(|cx| {
    ///     let object = cx.alloc_leaf(0u64)?;
    ///     cx.read(object, |value| VALUE.set(cx, object, *value + 1));
    ///     Ok(())
    /// }).unwrap();
    /// ```
    pub fn read<T: GcObject, R>(
        &self,
        object: Gc<'gc, T>,
        read: impl for<'read> FnOnce(&'read T) -> R,
    ) -> R {
        let pointer = self.typed_pointer(object);
        // SAFETY: typed admission establishes live initialized T. The callback
        // cannot retain this loan or obtain mutable Scope access. Other shared
        // loans may coexist, including aliases of this same object.
        read(unsafe { pointer.as_ref() })
    }

    /// Select a stored edge while its source is borrowed, then return a handle
    /// independent of that payload loan. The target survives source disconnection
    /// within this scope because collection is excluded, not because it is rooted.
    ///
    /// # Panics
    /// Panics on an unavailable heap, foreign source/selected edge or invalid
    /// stored representation, before dereferencing the rejected object. The
    /// selector must return an edge from this heap. Selector panics propagate
    /// without a heap write.
    pub fn follow<T: GcObject, U: 'static>(
        &self,
        object: Gc<'gc, T>,
        select: impl for<'source> FnOnce(&'source T) -> Option<&'source Edge<U>>,
    ) -> Option<Gc<'gc, U>> {
        self.read(object, |object| {
            select(object).map(|edge| {
                // The selector borrows a live source. Check original-heap
                // membership and target type before constructing a handle.
                let target = self
                    .access
                    .heap
                    .physical
                    .resolve(edge.reference)
                    .expect("selected edge must name live storage in this heap");
                assert!(
                    target.descriptor.matches::<U>(),
                    "stored edge type mismatch"
                );
                self.handle(edge.reference)
            })
        })
    }

    /// Execute a reviewed provider mutation with the same kernel as rooted access.
    /// Ordinary clients should use checked fields or provider methods.
    ///
    /// # Safety
    /// Honor Access::with_object_mut's tracing, publication, charge and unwind
    /// contract. Do not expose arbitrary mutable objects/edges to safe callers.
    pub unsafe fn with_object_mut<T: GcObject, R>(
        &mut self,
        object: Gc<'gc, T>,
        operation: impl for<'op> FnOnce(&'op mut T, &mut Mutation<'op, T>) -> Result<R, GcError>,
    ) -> Result<R, GcError> {
        let reference = self.check(object)?;
        self.access.with_reference_mut(reference, operation)
    }

    /// Lend a provider object while exclusively borrowing this access scope.
    ///
    /// # Safety
    /// Encapsulate edits behind the ObjectMut::parts contract, including forget
    /// safety. ScalarField::borrow_mut supplies the ordinary safe field operation.
    ///
    /// ```compile_fail
    /// use kagari_gc::heap::Heap;
    /// let mut heap = Heap::default();
    /// let root = heap.access().unwrap().alloc_leaf(1u64).unwrap();
    /// heap.scope(|cx| {
    ///     let object = cx.get(&root)?;
    ///     let first = unsafe { cx.borrow_object_mut(object)? };
    ///     let second = unsafe { cx.borrow_object_mut(object)? };
    ///     println!("{}", first.object());
    ///     Ok(())
    /// }).unwrap();
    /// ```
    pub unsafe fn borrow_object_mut<T: GcObject>(
        &mut self,
        object: Gc<'gc, T>,
    ) -> Result<ObjectMut<'_, T>, GcError> {
        let reference = self.check(object)?;
        self.access.borrow_object_mut(reference)
    }

    /// Recover a Rust representation only after checking its exact registered type.
    /// Dynamic layouts never satisfy this check, even if their byte sizes match T.
    pub fn downcast<T: GcObject>(&self, object: Gc<'gc, AnyObject>) -> Result<Gc<'gc, T>, GcError> {
        let reference = self.check(object)?;
        if !self
            .access
            .heap
            .physical
            .resolve(reference)?
            .descriptor
            .matches::<T>()
        {
            return Err(GcError::Representation);
        }
        Ok(self.handle(reference))
    }

    /// Resolve an admitted static representation without repeating physical lookup.
    /// The pointer remains internal. Read loans borrow Scope shared; mutable loans
    /// require an exclusive Scope borrow covering the reference's lifetime.
    pub(crate) fn typed_pointer<T: GcObject>(&self, object: Gc<'gc, T>) -> NonNull<T> {
        let reference = self.check(object).expect("invalid scoped object access");
        let (_, offset) = ObjectHeader::layout::<T>().expect("registered Rust layout");
        // SAFETY: Gc construction establishes a retained initialized T with this
        // allocation layout. No collection, relocation or representation change
        // can occur inside Scope. The invariant brand proves heap membership;
        // check still rejects faulted access, including after a caught panic.
        let pointer = unsafe { reference.0.cast::<u8>().add(offset).cast::<T>() };
        #[cfg(debug_assertions)]
        {
            let resolved = self
                .access
                .heap
                .physical
                .resolve(reference)
                .expect("admitted object must name live storage");
            debug_assert!(resolved.descriptor.matches::<T>());
            debug_assert_eq!(resolved.pointer.cast::<T>(), pointer);
        }
        pointer
    }

    // Matching invariant brands establish membership without a per-handle heap ID.
    // Constructors validate external roots/edges/layouts; source_get delegates that
    // proof to its unsafe paired-source contract. Faults can still arise mid-scope.
    pub(crate) fn check<T: 'static>(&self, object: Gc<'gc, T>) -> Result<ObjectRef, GcError> {
        self.access.heap.marking.check()?;
        Ok(object.reference)
    }

    /// Publish after this closed operation has admitted heap state. Typed field
    /// writes use typed_pointer, dynamic writes use exact-layout admission, and
    /// source_edge checks state directly. No callback or state-changing operation
    /// may intervene. A publication failure quarantines before returning.
    pub(crate) fn publish_admitted<T: 'static>(
        &mut self,
        object: Gc<'gc, T>,
    ) -> Result<ObjectRef, GcError> {
        debug_assert!(self.access.heap.marking.check().is_ok());
        let reference = object.reference;
        let heap = &mut self.access.heap;
        if heap.marking.phase != Phase::Idle
            && let Err(error) = heap
                .marking
                .publish_admitted(&heap.physical, reference, None)
        {
            // Publication is pre-reserved and has no recoverable admission
            // failure. A caught field-write panic must not resume a collector
            // whose marking invariants could not be established.
            heap.marking.phase = Phase::Faulted;
            return Err(error);
        }
        Ok(reference)
    }

    /// Callers establish a live same-heap target and exact T representation (or
    /// the erased AnyObject marker). The type must stay valid throughout the
    /// scope: typed field access relies on this admission invariant.
    pub(crate) fn handle<T: 'static>(&self, reference: ObjectRef) -> Gc<'gc, T> {
        Gc {
            reference,
            invariant: PhantomData,
            marker: PhantomData,
        }
    }
}

impl<T: GcObject> TypedDescriptor<T> {
    /// Allocate through this prepared registration without repeating type lookup.
    /// The result is protected by the scope; foreign registrations are rejected.
    pub fn alloc_leaf<'gc>(&self, cx: &mut Scope<'gc>, value: T) -> Result<Gc<'gc, T>, GcError> {
        if T::SCAN_KIND != ScanKind::EdgeFree {
            return Err(GcError::Provider("alloc_leaf requires EdgeFree"));
        }
        // SAFETY: every value of this representation excludes managed edges.
        unsafe { self.alloc_with(cx, |_| Ok(value)) }
    }

    /// Construct a scope-protected object using this heap-local registration.
    /// Registration retains the descriptor and reserves its scanner workspace.
    ///
    /// # Safety
    /// Honor Access::alloc_with's initialized storage, source protection, complete
    /// tracing and nonescaping carrier contract. Init::edge_scoped imports handles.
    pub unsafe fn alloc_with<'gc>(
        &self,
        cx: &mut Scope<'gc>,
        initializer: impl for<'init> FnOnce(Init<'init>) -> Result<T, GcError>,
    ) -> Result<Gc<'gc, T>, GcError> {
        cx.access.check_allocation(self.heap, &self.descriptor)?;
        let reference = cx.access.allocate(self, initializer)?;
        Ok(cx.handle(reference))
    }
}
