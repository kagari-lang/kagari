//! Checked generation identities point into nonmoving, descriptor-owned page slots.
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
    pages::{Pages, Retired, SLAB_BYTES},
    retirement::Sweep,
};
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap, HeapObjectId,
        fields::{StructView, StructViewMut},
        graph::marks::{Domain, MarkBlocks, MarkRef},
        reference::{GcRef, HeapObjectIdentity},
        storage::HeapObject,
    },
    module::ModuleKey,
    native::{binding::NativeResult, storage::NativeObject},
    resource::HeapCharge,
};
use std::{collections::HashMap, slice};

#[cfg(test)]
use std::cell::Cell;

/// Collector view of one retained direct allocation. Payload, descriptor and physical mark
/// share the storage borrow; none may survive a yield, callback or retirement.
pub(in crate::gc) struct ResolvedDirect<'a> {
    pub(in crate::gc) fields: Option<StructView<'a>>,
    pub(in crate::gc) mark: MarkRef<'a>,
    pub(in crate::gc) edges: bool,
}

impl GcHeap {
    pub(super) fn dispose_payload(&self, mut retired: Retired) -> usize {
        if retired.dispose() {
            self.resources
                .quarantine("native payload destruction panicked");
        }
        let (released, retired) = self.objects.borrow_mut().finish(retired);
        if retired.release() {
            self.resources
                .quarantine("representation owner destruction panicked");
        }
        self.resources
            .release_heap_charge(HeapCharge::new(0, released));
        released
    }
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
    Enum,
    Struct,
    Interface,
    Closure,
    Cell,
    Vacant,
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
            Self::Enum => "Enum",
            Self::Struct => "Struct",
            Self::Interface => "Interface",
            Self::Closure => "Closure",
            Self::Cell => "Cell",
            Self::Vacant => "Vacant",
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

#[derive(Debug)]
pub(super) struct ObjectSlot {
    pub(super) generation: u32,
    location: PayloadLocation,
}

impl ObjectSlot {
    pub(super) fn kind(&self) -> PayloadKind {
        self.location.kind
    }

    pub(super) fn location(&self) -> Option<PayloadLocation> {
        (self.location.kind != PayloadKind::Vacant).then_some(self.location)
    }

    /// The caller has validated occupancy and owns this identity's retirement.
    /// Exhausted generations stay vacant but never rejoin the reusable chain.
    fn retire(&mut self, index: u32, free: &mut Option<u32>) {
        self.location = PayloadLocation {
            kind: PayloadKind::Vacant,
            page: free.unwrap_or(0),
            slot: u16::from(free.is_some()),
        };
        if let Some(generation) = self.generation.checked_add(1) {
            self.generation = generation;
            *free = Some(index);
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct ObjectStorage {
    pub(super) slots: Vec<ObjectSlot>,
    pub(super) initialization_owners: HashMap<HeapObjectIdentity, ModuleKey>,
    pages: Pages,
    marks: MarkBlocks,
    free_identity: Option<u32>,
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

    /// The caller has resolved this occupied directory under the same storage
    /// borrow. Marks belong to directory slots, so no physical-page reverse lookup
    /// is needed after checking the full object identity.
    pub(in crate::gc) fn mark_for_directory(&self, index: usize) -> MarkRef<'_> {
        self.marks.slot(Domain::Object, index)
    }

    pub(in crate::gc) fn mark(&self, location: PayloadLocation) -> Option<MarkRef<'_>> {
        if let Some(mark) = self.pages.direct_mark(location) {
            return Some(mark);
        }
        let directory = self.pages.directory(location)?;
        Some(self.marks.slot(Domain::Object, directory as usize))
    }

    pub(in crate::gc) fn next_gray(
        &self,
        owner: u64,
        block: usize,
        epoch: u64,
    ) -> (Option<HeapObjectId>, bool) {
        let (index, more) = self.marks.next_gray(block, epoch);
        (
            index.map(|index| {
                let slot = &self.slots[index];
                assert!(slot.location().is_some(), "gray heap directory occupied");
                HeapObjectId::new(owner, index, slot.generation)
            }),
            more,
        )
    }

    pub(super) fn next_directory(&self) -> usize {
        self.free_identity
            .map_or(self.slots.len(), |index| index as usize)
    }

    pub(super) fn publish(&mut self, index: usize, location: PayloadLocation) {
        if index == self.slots.len() {
            self.slots.push(ObjectSlot {
                generation: 0,
                location,
            });
        } else {
            let vacant = &mut self.slots[index];
            assert_eq!(vacant.location.kind, PayloadKind::Vacant);
            self.free_identity = (vacant.location.slot != 0).then_some(vacant.location.page);
            vacant.location = location;
        }
        self.occupied = self
            .occupied
            .checked_add(1)
            .expect("bounded directory occupancy");
    }

    fn retire_identity(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        assert!(slot.location().is_some(), "retire occupied identity");
        self.occupied = self
            .occupied
            .checked_sub(1)
            .expect("occupied directory count");
        slot.retire(index as u32, &mut self.free_identity);
    }

    /// Upper bound used to preflight per-admission byte arithmetic before placement.
    pub(super) fn backing_bound(object: &HeapObject) -> NativeResult<usize> {
        Ok(match object {
            HeapObject::Native(object) => object.physical_layout()?.0.size().max(SLAB_BYTES),
            HeapObject::Struct { storage } => storage.descriptor.layout.size().max(SLAB_BYTES),
            _ => SLAB_BYTES,
        })
    }

    pub(super) fn backing_bytes(&self) -> usize {
        self.pages.backing_bytes
    }

    pub(super) fn insert(
        &mut self,
        directory: u32,
        object: HeapObject,
    ) -> NativeResult<PayloadLocation> {
        if directory != u32::MAX {
            self.marks
                .prepare(directory as usize)
                .map_err(|_| RuntimeError::resource_limit("heap block marks"))?;
        }
        match object {
            HeapObject::String(value) => self.place(directory, PayloadKind::String, false, value),
            HeapObject::Tuple(value) => self.place(directory, PayloadKind::Tuple, true, value),
            HeapObject::Range(value) => self.place(directory, PayloadKind::Range, false, value),
            HeapObject::HostRoot(value) => {
                self.place(directory, PayloadKind::HostRoot, false, value)
            }
            HeapObject::HostPath(value) => {
                self.place(directory, PayloadKind::HostPath, true, value)
            }
            HeapObject::Ephemeral(value) => {
                self.place(directory, PayloadKind::Ephemeral, false, value)
            }
            HeapObject::Enum(value, trace) => {
                self.place(directory, PayloadKind::Enum, true, (value, trace))
            }
            HeapObject::Closure { snapshot } => {
                self.place(directory, PayloadKind::Closure, true, snapshot)
            }
            HeapObject::Cell { ty, value } => {
                self.place(directory, PayloadKind::Cell, true, (ty, value))
            }
            HeapObject::Interface { snapshot } => self.place(
                directory,
                PayloadKind::Interface,
                true,
                snapshot
                    .directory_index()
                    .expect("admitted interface locator"),
            ),
            HeapObject::Native(value) => {
                let descriptor = self.pages.descriptors.native(&value)?;
                let (_, offset) = value.physical_layout()?;
                self.pages.insert(directory, descriptor, |pointer| {
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
                self.pages.insert(directory, descriptor.clone(), |pointer| {
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
        directory: u32,
        kind: PayloadKind,
        edges: bool,
        value: T,
    ) -> NativeResult<PayloadLocation> {
        let descriptor = self.pages.descriptors.builtin::<T>(kind, edges)?;
        self.pages.insert(directory, descriptor, |pointer| {
            // SAFETY: sealed kind/T pairing, descriptor layout and exclusive fresh slot.
            unsafe { pointer.cast::<T>().as_ptr().write(value) };
            Ok(())
        })
    }

    /// Detach without running payload destructors under the directory borrow.
    pub(super) fn detach(&mut self, id: HeapObjectId) -> Option<Retired> {
        let slot = self.slots.get_mut(id.index())?;
        if slot.generation != id.generation {
            return None;
        }
        let location = slot.location()?;
        let retired = self.pages.detach(location)?;
        self.retire_identity(id.index());
        self.initialization_owners.remove(&id.into());
        Some(retired)
    }

    /// Unlink disposed storage, returning final owners for drop outside table borrows.
    pub(super) fn finish(&mut self, retired: Retired) -> (usize, Retired) {
        self.pages.finish(retired)
    }

    /// Teardown cannot allocate a dead list. Each token keeps both descriptor and
    /// backing alive while all initialized values are destroyed, including panics.
    pub(super) fn teardown(&mut self, mut panic: impl FnMut()) {
        loop {
            let location = self.pages.occupied().next().map(|(_, location)| location);
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
