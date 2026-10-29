//! Nonmoving managed allocations own headers, payloads and physical page marks.
mod access;
#[cfg(feature = "execution-diagnostics")]
mod accounting;
pub(super) mod descriptors;
pub(super) mod maintenance;
mod pages;
mod retirement;
#[cfg(test)]
mod tests;

use self::{
    pages::{CheckedPayload, Pages, Retired, SLAB_BYTES},
    retirement::Sweep,
};
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap, fields::StructViewMut, graph::marks::MarkRef, object_header::ObjectHeader,
        reference::GcRef, storage::HeapObject,
    },
    module::ModuleKey,
    native::{binding::NativeResult, payload::take::NativeTake, storage::NativeObject},
};
use std::{collections::HashMap, slice, sync::Arc};

#[cfg(test)]
use std::cell::Cell;

/// Scoped view of one retained direct allocation. Payload, descriptor and physical mark
/// share the storage borrow; none may survive a yield or retirement. Provider
/// callbacks keep their existing payload loan but reacquire completion marks.
pub(in crate::gc) struct ResolvedDirect<'a> {
    payload: CheckedPayload<'a>,
    pub(in crate::gc) mark: MarkRef<'a>,
}

impl Drop for GcHeap {
    fn drop(&mut self) {
        self.objects.get_mut().teardown(|| {
            self.resources
                .quarantine("native payload destruction panicked during teardown");
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(super) enum PayloadKind {
    String,
    Tuple,
    Range,
    HostRoot,
    HostPath,
    Ephemeral,
    Native,
    Taken,
    Enum,
    Struct,
    Interface,
    Closure,
    Cell,
}

impl PayloadKind {
    pub(in crate::gc) fn observation_owner(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Tuple => "Tuple",
            Self::Range => "Range",
            Self::HostRoot => "HostRoot",
            Self::HostPath => "HostPath",
            Self::Ephemeral => "Ephemeral",
            Self::Native => "NativeObject",
            Self::Taken => "TakenNative",
            Self::Enum => "Enum",
            Self::Struct => "Struct",
            Self::Interface => "Interface",
            Self::Closure => "Closure",
            Self::Cell => "Cell",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PayloadLocation {
    pub(super) kind: PayloadKind,
    page: u32,
    // At most 128 slots in a slab; large allocations have exactly one.
    slot: u16,
}

#[derive(Debug, Default)]
pub(super) struct ObjectStorage {
    pub(super) initialization_owners: HashMap<GcRef, ModuleKey>,
    pages: Pages,
    // Current occupancy, unlike generation history or a sweep's visited survivors.
    occupied: usize,
    sweep: Option<Sweep>,
    #[cfg(test)]
    pub(super) discovery_visits: Cell<usize>,
}

impl ObjectStorage {
    pub(super) fn direct_reference(&self, location: PayloadLocation) -> Option<GcRef> {
        self.pages.direct_reference(location)
    }

    pub(super) fn publish_direct(&mut self) {
        self.occupied = self
            .occupied
            .checked_add(1)
            .expect("bounded heap occupancy");
    }

    pub(in crate::gc) fn next_gray_direct(
        &self,
        block: usize,
        epoch: u64,
    ) -> (Option<GcRef>, bool) {
        self.pages.next_gray_direct(block, epoch)
    }

    pub(super) fn direct_location(&self, reference: GcRef) -> Option<PayloadLocation> {
        self.pages.direct_location(reference)
    }

    pub(in crate::gc) fn mark(&self, location: PayloadLocation) -> Option<MarkRef<'_>> {
        self.pages.direct_mark(location)
    }

    /// Upper bound used to preflight per-admission byte arithmetic before placement.
    pub(super) fn backing_bound(object: &HeapObject) -> NativeResult<usize> {
        Ok(match object {
            HeapObject::Native(object) => ObjectHeader::layout(object.physical_layout()?.0)
                .ok_or_else(|| RuntimeError::resource_limit("native object header layout"))?
                .0
                .pad_to_align()
                .size()
                .max(SLAB_BYTES),
            HeapObject::Struct { storage } => storage.descriptor.layout.size().max(SLAB_BYTES),
            _ => SLAB_BYTES,
        })
    }

    pub(super) fn backing_bytes(&self) -> usize {
        self.pages.backing_bytes
    }

    pub(super) fn insert(&mut self, object: HeapObject) -> NativeResult<PayloadLocation> {
        match object {
            HeapObject::Taken => unreachable!("tombstones replace admitted Native owners"),
            HeapObject::String(value) => self.place(PayloadKind::String, false, value),
            HeapObject::Tuple(value) => self.place(PayloadKind::Tuple, true, value),
            HeapObject::Range(value) => self.place(PayloadKind::Range, false, value),
            HeapObject::HostRoot(value) => self.place(PayloadKind::HostRoot, false, value),
            HeapObject::HostPath(value) => self.place(PayloadKind::HostPath, true, value),
            HeapObject::Ephemeral(value) => self.place(PayloadKind::Ephemeral, false, value),
            HeapObject::Enum(value, trace) => self.place(PayloadKind::Enum, true, (value, trace)),
            HeapObject::Closure { snapshot } => self.place(PayloadKind::Closure, true, snapshot),
            HeapObject::Cell { ty, value } => self.place(PayloadKind::Cell, true, (ty, value)),
            HeapObject::Interface { snapshot } => self.place(
                PayloadKind::Interface,
                true,
                snapshot
                    .directory_index()
                    .expect("admitted interface locator"),
            ),
            HeapObject::Native(value) => {
                let descriptor = self.pages.descriptors.native(&value)?;
                let (_, offset) = value.physical_layout()?;
                self.pages.insert(descriptor, |pointer| {
                    // SAFETY: the descriptor reserves the checked header+T Layout. T is
                    // initialized first, then its owning header; no fallible step follows.
                    unsafe {
                        let value = value.place_payload(pointer.add(offset));
                        pointer.cast::<NativeObject>().as_ptr().write(value);
                    }
                    Ok(())
                })
            }
            HeapObject::Struct { storage } => {
                let descriptor = storage.descriptor;
                let description = descriptor.structure_layout();
                self.pages.insert(descriptor.clone(), |pointer| {
                    // SAFETY: encoded bytes have alignment one, with the descriptor's
                    // exact length. Zero initialization also covers any layout padding.
                    let bytes = unsafe {
                        pointer.as_ptr().write_bytes(0, description.size);
                        slice::from_raw_parts_mut(pointer.as_ptr(), description.size)
                    };
                    let mut view = StructViewMut { description, bytes };
                    for (slot, value) in storage.values.into_iter().enumerate() {
                        view.set(slot, value).ok_or_else(|| {
                            RuntimeError::module_validation("invalid struct placement")
                        })?;
                    }
                    Ok(())
                })
            }
        }
    }

    fn place<T: Send + 'static>(
        &mut self,
        kind: PayloadKind,
        edges: bool,
        value: T,
    ) -> NativeResult<PayloadLocation> {
        let descriptor = self.pages.descriptors.builtin::<T>(kind, edges)?;
        self.pages.insert(descriptor, |pointer| {
            // SAFETY: sealed kind/T pairing, descriptor layout and exclusive fresh slot.
            unsafe { pointer.cast::<T>().as_ptr().write(value) };
            Ok(())
        })
    }

    /// Replace a validated Native allocation without retiring its identity or mark.
    /// The caller checks runtime/liveness, T's take capability and borrow state.
    /// Move T before releasing this borrow, then destroy returned metadata outside it.
    pub(super) fn take_native<T: NativeTake>(
        &mut self,
        id: GcRef,
    ) -> NativeResult<(T, NativeObject, Arc<descriptors::Descriptor>)> {
        let location = self
            .direct_location(id)
            .expect("retained Native allocation");
        let (mut native, descriptor) = self.pages.take_native(location)?;
        self.initialization_owners.remove(&id);
        let payload = native.take_payload::<T>();
        Ok((payload, native, descriptor))
    }

    /// Unlink disposed storage, returning final owners for drop outside table borrows.
    pub(super) fn finish(&mut self, retired: Retired) -> (usize, Retired) {
        self.pages.finish(retired)
    }

    /// Teardown cannot allocate a dead list. Each token keeps both descriptor and
    /// backing alive while all initialized values are destroyed, including panics.
    pub(super) fn teardown(&mut self, mut panic: impl FnMut()) {
        loop {
            let location = self.pages.occupied().next();
            let Some(location) = location else {
                break;
            };
            let mut retired = self.pages.detach(location).expect("occupied teardown slot");
            if retired.dispose() {
                panic();
            }
            let (_, retired) = self.finish(retired);
            if retired.release() {
                panic();
            }
        }
    }

    #[cfg(test)]
    pub(super) fn corrupt_structure_for_test(&mut self, location: PayloadLocation) {
        self.pages.corrupt_structure(location);
    }
}
