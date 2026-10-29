//! Runtime-owned field facts for admitted, noncollecting interpreter regions.
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap, directory::descriptors::structures::StructIdentity, fields::codec,
        object_header::ObjectHeader, reference::GcRef,
    },
    module::{StructLayoutRef, layout_storage::StoredField},
    value::Value,
};
use kagari_types::scalar::BuiltinType;
use std::{slice, sync::Arc};

/// Exact runtime/environment preparation retains compact identity against ID reuse.
/// It owns immutable layout facts, never objects or an additional root registry.
#[derive(Debug)]
pub(crate) struct PreparedStructField {
    identity: Arc<StructIdentity>,
    field: StoredField,
    slot: usize,
}

impl GcHeap {
    pub(crate) fn prepare_struct_field(
        &self,
        layout: StructLayoutRef,
        slot: usize,
    ) -> Result<PreparedStructField, RuntimeError> {
        let descriptor = self.struct_storage_description(layout)?;
        let field = *descriptor
            .structure_layout()
            .fields
            .get(slot)
            .ok_or_else(|| RuntimeError::module_validation("invalid prepared field slot"))?;
        Ok(PreparedStructField {
            identity: descriptor.struct_identity().clone(),
            field,
            slot,
        })
    }
}

impl PreparedStructField {
    pub(crate) fn layout(&self) -> &StructLayoutRef {
        &self.identity.semantic
    }

    /// Caller proves this live reference and preparation belong to the same heap.
    /// The admitted frame roots the receiver; no callback, collection or mutation
    /// can intervene before the returned edge is published into its operand bank.
    /// Header IDs alone do not establish ownership for arbitrary host input.
    pub(crate) unsafe fn read(&self, heap: &GcHeap, reference: GcRef) -> Option<Value> {
        // SAFETY: inherited same-runtime, live-frame and noncollecting contract.
        match unsafe {
            self.with_bytes(heap, reference, |bytes| {
                codec::read_field(bytes, self.field)
            })
        } {
            Some(value) => value,
            // Retained compatible generations may use different descriptor IDs.
            None => heap.struct_get_slot(reference, self.layout(), self.slot),
        }
    }

    /// Same admission/lifetime contract as read, with a checked scalar domain.
    pub(crate) unsafe fn read_scalar(
        &self,
        heap: &GcHeap,
        reference: GcRef,
        ty: BuiltinType,
    ) -> Option<u64> {
        let StoredField::Scalar { offset, encoding } = self.field else {
            return None;
        };
        if encoding.ty != ty {
            return None;
        }
        // SAFETY: inherited same-runtime, live-frame and noncollecting contract.
        match unsafe {
            self.with_bytes(heap, reference, |bytes| {
                codec::read_scalar(bytes, offset, encoding)
            })
        } {
            Some(value) => value,
            None => heap.struct_scalar(reference, self.layout(), self.slot, ty),
        }
    }

    /// Same runtime/liveness contract as read. Inputs remain protected through
    /// admission and barrier publication; no callback can run during the commit.
    pub(crate) unsafe fn write(
        &self,
        heap: &GcHeap,
        reference: GcRef,
        value: Value,
    ) -> Result<(), RuntimeError> {
        // SAFETY: inherited live, same-runtime operand contract.
        if !unsafe { self.matches(reference) } {
            return heap.struct_set_slot(reference, self.layout(), self.slot, value);
        }
        heap.admit_struct_write(self.layout(), self.slot, &value)?;
        heap.publish_value(value)?;
        // SAFETY: descriptor matched above; publication cannot collect or reenter.
        unsafe {
            self.with_bytes_mut(heap, reference, |bytes| {
                codec::write_field(bytes, self.field, value)
            })
        }
    }

    /// Same contract as write, preserving scalar validation before mutation.
    pub(crate) unsafe fn write_scalar(
        &self,
        heap: &GcHeap,
        reference: GcRef,
        ty: BuiltinType,
        bits: u64,
    ) -> Result<(), RuntimeError> {
        // SAFETY: inherited live, same-runtime operand contract.
        if !unsafe { self.matches(reference) } {
            return heap.set_struct_scalar(reference, self.layout(), self.slot, ty, bits);
        }
        heap.admit_struct_scalar_write(self.layout(), self.slot, ty, bits)?;
        let StoredField::Scalar { offset, encoding } = self.field else {
            return Err(heap
                .resources
                .quarantine("prepared scalar field encoding mismatch"));
        };
        // SAFETY: descriptor matched above; admission cannot collect or reenter.
        unsafe {
            self.with_bytes_mut(heap, reference, |bytes| {
                codec::write_scalar(bytes, offset, encoding, bits)
            })
        }
    }

    unsafe fn matches(&self, reference: GcRef) -> bool {
        // SAFETY: caller retains a live reference in the preparation's runtime.
        unsafe { ObjectHeader::read(reference.pointer()) }.descriptor_id
            == self.identity.registered_id()
    }

    /// Caller already matched the immutable header and completed write admission.
    unsafe fn with_bytes_mut(
        &self,
        heap: &GcHeap,
        reference: GcRef,
        write: impl FnOnce(&mut [u8]) -> Option<()>,
    ) -> Result<(), RuntimeError> {
        let _storage = heap.objects_mut()?;
        // SAFETY: same descriptor proves initialized payload bounds. The exclusive
        // storage guard excludes all native/struct views; no reference escapes.
        let bytes = unsafe {
            slice::from_raw_parts_mut(
                reference.pointer().as_ptr().add(ObjectHeader::SIZE),
                self.identity.size,
            )
        };
        write(bytes).ok_or_else(|| {
            heap.resources
                .quarantine("struct storage does not match its layout")
        })
    }

    /// A mismatch never forms a payload slice using the expected object's size.
    unsafe fn with_bytes<R>(
        &self,
        heap: &GcHeap,
        reference: GcRef,
        read: impl FnOnce(&[u8]) -> R,
    ) -> Option<R> {
        let _storage = heap.objects.try_borrow().ok()?;
        // SAFETY: the admitted operand retains a live allocation of this heap.
        let header = unsafe { ObjectHeader::read(reference.pointer()) };
        if header.descriptor_id != self.identity.registered_id() {
            return None;
        }
        // SAFETY: same-runtime descriptor equality proves payload size/encoding;
        // the retained identity cannot recycle, and the storage loan excludes writes.
        let bytes = unsafe {
            slice::from_raw_parts(
                reference.pointer().as_ptr().add(ObjectHeader::SIZE),
                self.identity.size,
            )
        };
        Some(read(bytes))
    }
}
