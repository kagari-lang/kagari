//! Managed representations and pointer-sized stored edges.

use crate::{
    error::GcError,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::{
    alloc::{Layout, LayoutError},
    any::Any,
    fmt,
    fmt::Pointer,
    hash::{Hash, Hasher},
    marker::PhantomData,
    ptr::NonNull,
};

/// Complete tracing and constant-time owned-capacity accounting for one object.
///
/// # Safety
/// Report every managed edge through the same scanner, including algorithm scratch.
/// Stored edges must be imported from protected same-heap sources. Scanning and
/// charging cannot allocate, panic, call scripts, mutate the object or inspect peers.
/// All edge, structure and charge changes require the provider mutation capability;
/// shared access must not hide such changes behind interior mutability. ScanState
/// is pointer/reference/root-free, owned and finite. Pending must spend work or
/// leave an insufficient budget unchanged. Destruction cannot dereference managed
/// peers or reenter the heap. Every error/unwind leaves valid owned storage.
pub unsafe trait GcObject: Any + Send {
    type ScanState: Copy + Send + 'static;

    const SCAN_KIND: ScanKind;

    fn begin_scan(&self) -> Self::ScanState;

    fn scan(
        &self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError>;

    /// Describe owned indirect bytes and provider logical units, excluding this
    /// object's shallow bytes, child objects and shared host backing. Charge
    /// changes require admitted provider mutation and committed accounting before
    /// exposing a result whose loan can be forgotten.
    fn charge(&self) -> ObjectCharge {
        ObjectCharge::default()
    }
}

/// Provider-owned indirect capacity and logical units, sampled without allocation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ObjectCharge {
    pub bytes: usize,
    /// Provider-defined logical cost, aggregated in HeapStats::provider_units.
    /// Physical object count is observed separately; units do not drive pacing.
    pub units: usize,
}

/// A traced internal reference with no independent retention authority.
///
/// This is neither Copy nor Clone. Ordinary code cannot create or extract an owned
/// carrier. Providers must keep its traced source live when inspecting or moving it.
pub struct Edge<T: 'static> {
    pub(crate) reference: ObjectRef,
    pub(crate) marker: PhantomData<fn() -> T>,
}

/// A copyable object identity valid only inside its creating Heap::scope callback.
/// Its invariant lifetime proves heap membership; the handle stores one pointer.
/// Equality/hashing use allocation identity within this invariant scope. Copying
/// registers no root and borrows no payload. Use Scope or a checked field
/// to access data; explicit retention is required to cross a collection boundary.
pub struct Gc<'gc, T: 'static> {
    pub(crate) reference: ObjectRef,
    pub(crate) invariant: PhantomData<fn(&'gc ()) -> &'gc ()>,
    pub(crate) marker: PhantomData<fn() -> T>,
}

/// Erased object identity, including runtime-defined layouts. This marker is not
/// an allocated Rust payload and deliberately does not implement GcObject.
///
/// ```compile_fail
/// use kagari_gc::{dynamic::DynamicLayoutBuilder, heap::Heap};
/// let mut heap = Heap::default();
/// let mut builder = DynamicLayoutBuilder::new();
/// let value = builder.scalar::<u64>();
/// let layout = heap.access().unwrap().register_dynamic(builder).unwrap();
/// heap.scope(|cx| {
///     let object = cx.alloc_dynamic(&layout)?;
///     Ok(cx.read(object, |_| ())) // erased bytes cannot be borrowed as a Rust T
/// }).unwrap();
/// ```
pub enum AnyObject {}

impl<'gc, T: 'static> Gc<'gc, T> {
    /// Compare allocation identity inside the same invariant heap scope. This
    /// observes no payload and grants no retention or address transport authority.
    pub fn ptr_eq<U: 'static>(self, other: Gc<'gc, U>) -> bool {
        self.reference == other.reference
    }

    /// Forget the static Rust representation without changing retention or scope.
    pub fn erase(self) -> Gc<'gc, AnyObject> {
        Gc {
            reference: self.reference,
            invariant: PhantomData,
            marker: PhantomData,
        }
    }
}

impl<T: 'static> PartialEq for Gc<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.reference == other.reference
    }
}

impl<T: 'static> Eq for Gc<'_, T> {}

impl<T: 'static> Hash for Gc<'_, T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.reference.hash(state);
    }
}

impl<T: 'static> Copy for Gc<'_, T> {}

impl<T: 'static> Clone for Gc<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> fmt::Debug for Gc<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Gc").field(&self.reference).finish()
    }
}

/// An opaque diagnostic identity within the protected allocation's lifetime.
/// Formatting exposes no reference or retention authority; the allocation may
/// be recycled after reclamation, so this text is not a persistent object ID.
impl<T: 'static> fmt::Pointer for Gc<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Pointer::fmt(&self.reference.0, f)
    }
}

impl<T: 'static> fmt::Debug for Edge<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Edge").field(&self.reference).finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ObjectRef(pub(crate) NonNull<ObjectHeader>);

// SAFETY: inert carriers expose no dereference; admitted payloads are Send and
// physical access always requires the owning heap's exclusive loan.
unsafe impl Send for ObjectRef {}

// SAFETY: copying an opaque address grants no access to its referent.
unsafe impl Sync for ObjectRef {}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(crate) struct ObjectHeader {
    pub(crate) descriptor: u32,
    pub(crate) allocation: u32,
}

const _: () = assert!(size_of::<ObjectHeader>() == 8);

impl ObjectHeader {
    /// The same Rust allocation layout owns registration and admitted typed access.
    pub(crate) const fn layout<T>() -> Result<(Layout, usize), LayoutError> {
        Layout::new::<Self>().extend(Layout::new::<T>())
    }
}

macro_rules! scalar_objects {
    ($($ty:ty),+ $(,)?) => {$(
        // SAFETY: these primitives contain no edges, owned backing or callbacks.
        unsafe impl GcObject for $ty {
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
    )+};
}

scalar_objects!(
    (),
    bool,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64
);
