//! Checked generation identities point into nonmoving, descriptor-owned page slots.
mod access;
#[cfg(feature = "execution-diagnostics")]
mod accounting;
pub(super) mod descriptors;
mod pages;
mod retirement;
#[cfg(test)]
mod tests;

use self::pages::{Pages, Retired, SLAB_BYTES};
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, fields::StructViewMut, storage::HeapObject},
    module::ModuleKey,
    native::{binding::NativeResult, storage::NativeObject},
    resource::HeapCharge,
};
use std::{collections::HashMap, slice};

#[cfg(test)]
use std::cell::Cell;

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
    pub(super) fn location(&self) -> Option<PayloadLocation> {
        (self.location.kind != PayloadKind::Vacant).then_some(self.location)
    }
}

#[derive(Debug, Default)]
pub(super) struct ObjectStorage {
    pub(super) slots: Vec<ObjectSlot>,
    pub(super) initialization_owners: HashMap<HeapObjectId, ModuleKey>,
    pages: Pages,
    free_identity: Option<u32>,
    // Occupancy index of stable wrapper identities, not another payload store.
    interface_directories: Vec<u32>,
    #[cfg(test)]
    pub(super) discovery_visits: Cell<usize>,
}

impl ObjectStorage {
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
    }

    fn retire_identity(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        slot.location = PayloadLocation {
            kind: PayloadKind::Vacant,
            page: self.free_identity.unwrap_or(0),
            slot: u16::from(self.free_identity.is_some()),
        };
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            self.free_identity = Some(index as u32);
        }
    }

    /// Upper bound used to preflight per-admission byte arithmetic before placement.
    pub(super) fn backing_bound(object: &HeapObject) -> NativeResult<usize> {
        Ok(match object {
            HeapObject::Native(object) => object.physical_layout()?.0.size().max(SLAB_BYTES),
            HeapObject::Struct { storage } => {
                storage.descriptor.structure_layout().size.max(SLAB_BYTES)
            }
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
            HeapObject::Interface { snapshot } => {
                self.interface_directories
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("interface wrapper occupancy"))?;
                self.interface_directories.push(directory);
                Ok(PayloadLocation {
                    kind: PayloadKind::Interface,
                    page: snapshot
                        .directory_index()
                        .expect("admitted interface locator"),
                    slot: 0,
                })
            }
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

    /// Enumerate occupied slots, not permanent generation tombstones. Wrapper
    /// occupancy is independent of the separately owned interface metadata.
    pub(super) fn occupied_directories(&self) -> impl Iterator<Item = usize> + '_ {
        self.pages
            .occupied_directories()
            .chain(self.interface_directories.iter().copied())
            .map(|index| {
                #[cfg(test)]
                self.discovery_visits.set(self.discovery_visits.get() + 1);
                index as usize
            })
    }

    /// Detach without running payload destructors under the directory borrow.
    pub(super) fn detach(&mut self, id: HeapObjectId) -> Option<Retired> {
        let slot = self.slots.get_mut(id.index())?;
        if slot.generation != id.generation {
            return None;
        }
        let location = slot.location()?;
        let retired = if location.kind == PayloadKind::Interface {
            Retired::Interface
        } else {
            self.pages.detach(location)?
        };
        self.retire_identity(id.index());
        self.initialization_owners.remove(&id);
        Some(retired)
    }

    /// Unlink disposed storage, returning final owners for drop outside table borrows.
    pub(super) fn finish(&mut self, retired: Retired) -> (usize, Retired) {
        self.pages.finish(retired)
    }

    // The synchronous sweep removes wrapper occupancy before metadata retirement.
    // PG05 replaces this batch compaction with the bounded wrapper sweep driver.
    pub(super) fn retire_wrappers(&mut self) {
        self.interface_directories
            .retain(|&index| self.slots[index as usize].location().is_some());
        if self.interface_directories.capacity()
            > self.interface_directories.len().saturating_mul(4).max(16)
        {
            self.interface_directories.shrink_to_fit();
        }
    }

    pub(super) fn prune(&mut self, live_objects: usize) {
        self.pages.descriptors.prune();
        // Preserve every generation tombstone, but not geometric Vec spare capacity
        // after a sparse sweep. Dense growth keeps its amortized admission headroom.
        // This may move identity records, never the payloads their IDs locate.
        if self.slots.len() > live_objects.saturating_mul(4).max(16) {
            self.slots.shrink_to_fit();
        }
        if self.initialization_owners.capacity()
            > self.initialization_owners.len().saturating_mul(4).max(16)
        {
            self.initialization_owners.shrink_to_fit();
        }
    }

    /// Teardown cannot allocate a dead list. Each token keeps both descriptor and
    /// backing alive while all initialized values are destroyed, including panics.
    pub(super) fn teardown(&mut self, mut panic: impl FnMut()) {
        while let Some(index) = self.interface_directories.pop() {
            self.retire_identity(index as usize);
        }
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
    pub(super) fn corrupt_structure_for_test(&mut self, id: HeapObjectId) {
        self.pages
            .corrupt_structure(self.slots[id.index()].location().unwrap());
    }
}
