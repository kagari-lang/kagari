//! Scalar loans preserve tracing; reference writes own barriers and continuation.

#[cfg(test)]
mod tests;

use crate::{
    access::{EdgeField, ScalarField, Scope},
    object::{Edge, Gc, GcObject},
};
use std::marker::PhantomData;

impl<T: GcObject, V: Copy> ScalarField<T, V> {
    /// Define the matching read/write projections of one scalar field.
    ///
    /// # Safety
    /// Both projections are pure, infallible, non-reentrant and identify the same
    /// field. Every V bit-pattern obtainable through safe Rust must be valid there.
    /// Arbitrary edits to V cannot affect traced edges, scanner state validity,
    /// owned capacity/charge or other representation invariants. No owned managed
    /// carrier or interior-mutable tracing/accounting state may be exposed.
    pub const unsafe fn new(read: fn(&T) -> &V, write: fn(&mut T) -> &mut V) -> Self {
        Self { read, write }
    }

    /// Copy one scalar through shared Scope access without retaining a payload loan.
    ///
    /// # Panics
    /// Panics on an invalid scoped access, as described by Scope::read.
    pub fn get<'gc>(&self, cx: &Scope<'gc>, object: Gc<'gc, T>) -> V {
        cx.read(object, |object| *(self.read)(object))
    }

    /// Store a scalar; no reference barrier or caller scanner decision is needed.
    ///
    /// # Panics
    /// Panics on an invalid scoped access before storing the value.
    pub fn set<'gc>(&self, cx: &mut Scope<'gc>, object: Gc<'gc, T>, value: V) {
        *self.borrow_mut(cx, object) = value;
    }

    /// Lend the actual field under an exclusive Scope borrow. Access checks heap
    /// state; the scope brand proves ownership, and Gc<T> admission establishes live
    /// storage and the Rust layout. Debug builds also resolve that storage.
    /// No charge sampling, scanner changes or finalization are needed: new's unsafe
    /// contract guarantees every scalar edit preserves them, including on unwind.
    /// Other handles may exist, but data access must wait until the loan ends.
    ///
    /// # Panics
    /// Panics on an invalid scoped access before lending the field.
    ///
    /// ```compile_fail
    /// use kagari_gc::{access::ScalarField, heap::Heap};
    /// const VALUE: ScalarField<u64, u64> =
    ///     unsafe { ScalarField::new(|v| v, |v| v) };
    /// let mut heap = Heap::default();
    /// heap.scope(|cx| {
    ///     let object = cx.alloc_leaf(0u64)?;
    ///     let first = VALUE.borrow_mut(cx, object);
    ///     let second = VALUE.borrow_mut(cx, object);
    ///     *first += *second;
    ///     Ok(())
    /// }).unwrap();
    /// ```
    ///
    /// Reading through an alias also waits until the mutable loan ends.
    ///
    /// ```compile_fail,E0502
    /// use kagari_gc::{access::ScalarField, heap::Heap};
    /// const VALUE: ScalarField<u64, u64> =
    ///     unsafe { ScalarField::new(|v| v, |v| v) };
    /// let mut heap = Heap::default();
    /// heap.scope(|cx| {
    ///     let object = cx.alloc_leaf(0u64)?;
    ///     let alias = object;
    ///     let field = VALUE.borrow_mut(cx, object);
    ///     let value = VALUE.get(cx, alias);
    ///     *field += value;
    ///     Ok(())
    /// }).unwrap();
    /// ```
    ///
    /// ```compile_fail
    /// use kagari_gc::{access::ScalarField, heap::Heap};
    /// const VALUE: ScalarField<u64, u64> =
    ///     unsafe { ScalarField::new(|v| v, |v| v) };
    /// let mut heap = Heap::default();
    /// let escaped = heap.scope(|cx| {
    ///     let object = cx.alloc_leaf(0u64)?;
    ///     Ok(VALUE.borrow_mut(cx, object))
    /// }).unwrap();
    /// *escaped = 1;
    /// ```
    pub fn borrow_mut<'loan, 'gc>(
        &self,
        cx: &'loan mut Scope<'gc>,
        object: Gc<'gc, T>,
    ) -> &'loan mut V {
        let mut pointer = cx.typed_pointer(object);
        // SAFETY: typed admission establishes initialized live T with its fixed
        // layout; the exclusive Scope borrow covers the reference's lifetime.
        // The trusted projection exposes only this stable field; its mutations
        // cannot affect edges, capacity, or even an already-invalidated scanner.
        let object = unsafe { pointer.as_mut() };
        (self.write)(object)
    }
}

impl<T: GcObject, U: 'static> EdgeField<T, U> {
    /// Define matching projections of an optional, fixed-position traced edge.
    ///
    /// # Safety
    /// Projections must be pure, infallible and identify the same field. The scanner
    /// covers its target; replacing it preserves scan coordinates and charge. Any
    /// same-heap U or None must be a valid field value. Projections cannot mutate
    /// other state, execute callbacks, or expose a field with coupled invariants.
    pub const unsafe fn new(
        read: fn(&T) -> &Option<Edge<U>>,
        write: fn(&mut T) -> &mut Option<Edge<U>>,
    ) -> Self {
        Self { read, write }
    }

    /// Follow this field without registering a root or keeping its owner borrowed.
    ///
    /// # Panics
    /// Panics on invalid scoped access or a malformed edge; see Scope::follow.
    pub fn get<'gc>(&self, cx: &Scope<'gc>, object: Gc<'gc, T>) -> Option<Gc<'gc, U>> {
        cx.follow(object, |object| (self.read)(object).as_ref())
    }

    /// Publish the incoming target before replacing the field. Self/cyclic edges
    /// are valid; publication borrows metadata, never another object payload.
    ///
    /// # Panics
    /// Panics before replacing the field on invalid scoped access or publication
    /// failure. Publication failure faults the heap before panicking; catching
    /// that panic does not make the heap usable again. The invariant scope brand
    /// excludes foreign handles at compile time.
    pub fn set<'gc>(&self, cx: &mut Scope<'gc>, object: Gc<'gc, T>, value: Option<Gc<'gc, U>>) {
        if let Some(value) = value {
            cx.check(value).expect("invalid incoming scoped edge");
        }
        let mut pointer = cx.typed_pointer(object);
        let edge = value
            .map(|value| cx.publish(value).expect("fixed edge publication failed"))
            .map(|reference| Edge {
                reference,
                marker: PhantomData,
            });
        // SAFETY: the fixed-field contract preserves charge and scan coordinates.
        // Publication completes before the infallible store; exclusive Scope
        // access excludes collection/reentry. Preserve any pending restart.
        *(self.write)(unsafe { pointer.as_mut() }) = edge;
    }
}
