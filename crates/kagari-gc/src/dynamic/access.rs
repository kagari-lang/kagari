//! Dynamic field operations lend no Rust T and share heap identity/publication.

use crate::{
    access::Scope,
    dynamic::{DynamicLayout, ReferenceField, Scalar, ScalarField},
    error::GcError,
    object::{AnyObject, Gc, ObjectRef},
};
use std::ptr::NonNull;
#[cfg(debug_assertions)]
use std::sync::Arc;

impl<'gc> Scope<'gc> {
    /// Allocate a zero/default initialized dynamic record without a host Root.
    /// All references start at None. The VM completes required-field semantics
    /// before exposing the record to script code. Allocation never collects here.
    pub fn alloc_dynamic(&mut self, layout: &DynamicLayout) -> Result<Gc<'gc, AnyObject>, GcError> {
        self.access
            .check_allocation(layout.heap, &layout.descriptor)?;
        let shape = layout.descriptor.shape()?;
        // SAFETY: validated layout and infallible zero/default initialization of
        // all fields. No outgoing references or owned indirect resources exist.
        let reference = unsafe {
            self.access.allocate_initialized(
                &layout.descriptor,
                |pointer, _, _| {
                    shape.initialize(pointer);
                    Ok(())
                },
                |_, _, _| Ok(()),
            )?
        };
        Ok(self.handle(reference))
    }
}

impl DynamicLayout {
    // Closed operations use the pointer under a shared Scope borrow for reads
    // or an exclusive borrow for writes. It never escapes a loan/collection boundary.
    #[inline]
    fn pointer<'gc>(
        &self,
        cx: &Scope<'gc>,
        object: Gc<'gc, AnyObject>,
    ) -> Result<NonNull<u8>, GcError> {
        let reference = cx.check(object)?;
        if self.heap != cx.access.heap.id {
            return Err(GcError::ForeignHeap);
        }
        // SAFETY: scoped admission retains this same-heap object's immutable
        // header. Its descriptor ID cannot be recycled while the object is live;
        // this layout's Arc also pins its registration, even without live objects.
        let header = unsafe { reference.0.as_ref() };
        if header.descriptor != self.descriptor.id {
            return Err(GcError::Representation);
        }
        #[cfg(debug_assertions)]
        {
            let resolved = cx.access.heap.physical.resolve(reference)?;
            debug_assert!(Arc::ptr_eq(resolved.descriptor, &self.descriptor));
        }
        // SAFETY: exact same-heap descriptor identity proves payload alignment
        // and bounds; collection and representation changes are excluded here.
        Ok(unsafe { reference.0.cast::<u8>().add(self.descriptor.payload_offset) })
    }
}

impl<V: Scalar> ScalarField<V> {
    /// Read a scalar after exact layout admission. Foreign layouts and differing
    /// registrations fail before any field bytes are read, even at equal sizes.
    pub fn get<'gc>(&self, cx: &Scope<'gc>, object: Gc<'gc, AnyObject>) -> Result<V, GcError> {
        let pointer = self.layout.pointer(cx, object)?;
        // SAFETY: exact layout proves initialized V at this aligned offset. The
        // sealed scalar set and safe writes exclude invalid bool/char bit patterns.
        Ok(unsafe { pointer.as_ptr().add(self.offset).cast::<V>().read() })
    }

    /// Store a scalar through a closed short loan. Fixed coordinates and zero
    /// indirect charge preserve any pending scanner without restarting it.
    pub fn set<'gc>(
        &self,
        cx: &mut Scope<'gc>,
        object: Gc<'gc, AnyObject>,
        value: V,
    ) -> Result<(), GcError> {
        *self.borrow_mut(cx, object)? = value;
        Ok(())
    }

    /// Check the exact layout once and lend the scalar under an exclusive Scope
    /// borrow. The closed scalar set preserves tracing and charge without cleanup.
    /// The reference cannot escape the loan or overlap another object access.
    ///
    /// ```compile_fail
    /// use kagari_gc::{dynamic::DynamicLayoutBuilder, heap::Heap};
    /// let mut heap = Heap::default();
    /// let mut builder = DynamicLayoutBuilder::new();
    /// let value = builder.scalar::<u64>();
    /// let layout = heap.access().unwrap().register_dynamic(builder).unwrap();
    /// let field = layout.scalar(value).unwrap();
    /// heap.scope(|cx| {
    ///     let object = cx.alloc_dynamic(&layout)?;
    ///     let value = field.borrow_mut(cx, object)?;
    ///     field.set(cx, object, 2)?;
    ///     *value = 1;
    ///     Ok(())
    /// }).unwrap();
    /// ```
    ///
    /// ```compile_fail,E0502
    /// use kagari_gc::{dynamic::DynamicLayoutBuilder, heap::Heap};
    /// let mut heap = Heap::default();
    /// let mut builder = DynamicLayoutBuilder::new();
    /// let value = builder.scalar::<u64>();
    /// let layout = heap.access().unwrap().register_dynamic(builder).unwrap();
    /// let field = layout.scalar(value).unwrap();
    /// heap.scope(|cx| {
    ///     let object = cx.alloc_dynamic(&layout)?;
    ///     let value = field.borrow_mut(cx, object)?;
    ///     let other = field.get(cx, object)?;
    ///     *value += other;
    ///     Ok(())
    /// }).unwrap();
    /// ```
    ///
    /// ```compile_fail
    /// use kagari_gc::{dynamic::DynamicLayoutBuilder, heap::Heap};
    /// let mut heap = Heap::default();
    /// let mut builder = DynamicLayoutBuilder::new();
    /// let value = builder.scalar::<u64>();
    /// let layout = heap.access().unwrap().register_dynamic(builder).unwrap();
    /// let field = layout.scalar(value).unwrap();
    /// let escaped = heap.scope(|cx| {
    ///     let object = cx.alloc_dynamic(&layout)?;
    ///     field.borrow_mut(cx, object)
    /// }).unwrap();
    /// *escaped = 1;
    /// ```
    pub fn borrow_mut<'loan, 'gc>(
        &self,
        cx: &'loan mut Scope<'gc>,
        object: Gc<'gc, AnyObject>,
    ) -> Result<&'loan mut V, GcError> {
        let pointer = self.layout.pointer(cx, object)?;
        // SAFETY: exact immutable layout proves initialized aligned V. The loan
        // excludes collection, reentry and conflicting data access until it ends.
        Ok(unsafe { &mut *pointer.as_ptr().add(self.offset).cast::<V>() })
    }
}

impl ReferenceField {
    /// Follow an initialized optional reference without registering a root. It may
    /// name a dynamic or Rust object; Scope::downcast checks a desired Rust type.
    pub fn get<'gc>(
        &self,
        cx: &Scope<'gc>,
        object: Gc<'gc, AnyObject>,
    ) -> Result<Option<Gc<'gc, AnyObject>>, GcError> {
        let pointer = self.layout.pointer(cx, object)?;
        // SAFETY: immutable descriptor proves aligned, initialized reference slot.
        let target = unsafe {
            pointer
                .as_ptr()
                .add(self.offset)
                .cast::<Option<ObjectRef>>()
                .read()
        };
        target
            .map(|target| {
                cx.access.heap.physical.resolve(target)?;
                Ok(cx.handle(target))
            })
            .transpose()
    }

    /// Publish a protected incoming target, then replace the fixed slot. Failure
    /// leaves the old field intact. Nulls, self-edges and mixed cycles are valid.
    pub fn set<'gc>(
        &self,
        cx: &mut Scope<'gc>,
        object: Gc<'gc, AnyObject>,
        value: Option<Gc<'gc, AnyObject>>,
    ) -> Result<(), GcError> {
        let pointer = self.layout.pointer(cx, object)?;
        // Exact-layout admission above also checked heap state. Nothing can
        // invalidate that proof before publication in this exclusive operation.
        let target = value.map(|value| cx.publish_admitted(value)).transpose()?;
        // SAFETY: the admitted same-heap source lives through this closed commit.
        // No coordinate/charge changes or callbacks occur; pointers retain their
        // provenance through typed stores rather than integer byte encodings.
        unsafe {
            pointer
                .as_ptr()
                .add(self.offset)
                .cast::<Option<ObjectRef>>()
                .write(target)
        };
        Ok(())
    }
}
