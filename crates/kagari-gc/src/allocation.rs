//! Stable object allocation, source-checked address resolution and physical lists.

pub(crate) mod page;

use self::page::{GROUPS, Links, Page};
use crate::{
    descriptor::{Descriptor, Descriptors},
    error::GcError,
    object::{ObjectHeader, ObjectRef},
    table::Table,
};
use std::{cell::Cell, marker::PhantomData, ptr::NonNull, sync::Arc};

#[derive(Debug, Default)]
pub(crate) struct PhysicalCursor {
    pub(crate) page: Option<u32>,
    pub(crate) slot: usize,
}

#[derive(Debug)]
pub(crate) struct PhysicalHeap {
    pub(crate) allocations: Table<Page>,
    available: [Option<u32>; GROUPS],
    pub(crate) first_active: Option<u32>,
    pub(crate) cursor: PhysicalCursor,
    pub(crate) descriptors: Descriptors,
    pub(crate) backing_bytes: usize,
    pub(crate) objects: usize,
    not_sync: PhantomData<Cell<()>>,
}

// SAFETY: owned pages contain only admitted Send objects. Transfer occurs outside
// loans; no safe shared method exposes payload references or mutable page controls.
unsafe impl Send for PhysicalHeap {}

impl Default for PhysicalHeap {
    fn default() -> Self {
        Self {
            allocations: Table::default(),
            available: [None; GROUPS],
            first_active: None,
            cursor: PhysicalCursor::default(),
            descriptors: Descriptors::default(),
            backing_bytes: 0,
            objects: 0,
            not_sync: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Reservation {
    pub(crate) reference: ObjectRef,
    pub(crate) page: u32,
    pub(crate) slot: usize,
}

pub(crate) struct CheckedObject<'a> {
    pub(crate) pointer: NonNull<u8>,
    pub(crate) descriptor: &'a Arc<Descriptor>,
    pub(crate) page: u32,
    pub(crate) slot: usize,
}

/// Live-source membership without descriptor dispatch or a payload loan.
pub(crate) struct Location<'a> {
    pub(crate) page: &'a Page,
    pub(crate) id: u32,
    pub(crate) slot: usize,
    pub(crate) descriptor: u32,
}

impl PhysicalHeap {
    pub(crate) fn page_count(&self) -> usize {
        self.allocations.len()
    }

    pub(crate) fn page(&self, id: u32) -> Result<&Page, GcError> {
        self.allocations
            .get(id)
            .ok_or(GcError::Storage("page identity"))
    }

    pub(crate) fn page_mut(&mut self, id: u32) -> Result<&mut Page, GcError> {
        self.allocations
            .get_mut(id)
            .ok_or(GcError::Storage("page identity"))
    }

    pub(crate) fn reserve(
        &mut self,
        descriptor: &Descriptor,
        limit: usize,
    ) -> Result<Reservation, GcError> {
        let group = descriptor.group;
        let id = if let Some(id) = group.and_then(|group| self.available[group]) {
            id
        } else {
            let id = self.allocations.prepare()?;
            let new_bytes = self
                .backing_bytes
                .checked_add(Page::backing_layout(descriptor.layout, group)?.size())
                .ok_or(GcError::Limit("backing bytes"))?;
            if new_bytes > limit {
                return Err(GcError::Limit("backing bytes"));
            }
            let page = match group {
                Some(group) => Page::new_small(group)?,
                None => Page::new_large(descriptor.layout)?,
            };
            self.allocations.insert(id, page);
            self.backing_bytes = new_bytes;
            self.link_active(id)?;
            if let Some(group) = group {
                self.link_available(id, group)?;
            }
            id
        };
        let slot = self.page_mut(id)?.reserve();
        if self.page(id)?.free() == 0
            && let Some(group) = group
        {
            self.unlink_available(id, group)?;
        }
        Ok(Reservation {
            reference: ObjectRef(self.page(id)?.pointer(slot)),
            page: id,
            slot,
        })
    }

    pub(crate) fn commit(
        &mut self,
        reservation: Reservation,
        descriptor: u32,
    ) -> Result<(), GcError> {
        self.descriptors.retain(descriptor)?;
        // SAFETY: the ticket owns a reserved, aligned, fully initialized object slot.
        unsafe {
            reservation.reference.0.as_ptr().write(ObjectHeader {
                descriptor,
                allocation: reservation.page,
            })
        };
        self.page_mut(reservation.page)?.commit(reservation.slot);
        self.objects += 1;
        Ok(())
    }

    pub(crate) fn rollback(&mut self, reservation: Reservation) -> Result<(), GcError> {
        self.release_slot(reservation.page, reservation.slot)
    }

    pub(crate) fn release_slot(&mut self, id: u32, slot: usize) -> Result<(), GcError> {
        let was_full = self.page(id)?.free() == 0;
        self.page_mut(id)?.release(slot);
        let group = self.page(id)?.group_id();
        if was_full && let Some(group) = group {
            self.link_available(id, group)?;
        }
        if self.page(id)?.free() == self.page(id)?.slots() {
            self.remove_page(id)?;
        }
        Ok(())
    }

    fn remove_page(&mut self, id: u32) -> Result<(), GcError> {
        let page = self.page(id)?;
        if let Some(group) = page.group_id() {
            self.unlink_available(id, group)?;
        }
        let links = self.page(id)?.active();
        if self.cursor.page == Some(id) {
            self.cursor = PhysicalCursor {
                page: links.next,
                slot: 0,
            };
        }
        match links.previous {
            Some(previous) => self.page_mut(previous)?.active_mut().next = links.next,
            None => self.first_active = links.next,
        }
        if let Some(next) = links.next {
            self.page_mut(next)?.active_mut().previous = links.previous;
        }
        let page = self.allocations.remove(id).expect("retired page");
        self.backing_bytes -= page.backing().layout.size();
        Ok(())
    }

    fn link_active(&mut self, id: u32) -> Result<(), GcError> {
        *self.page_mut(id)?.active_mut() = Links {
            previous: None,
            next: self.first_active,
        };
        if let Some(first) = self.first_active {
            self.page_mut(first)?.active_mut().previous = Some(id);
        }
        self.first_active = Some(id);
        Ok(())
    }

    fn link_available(&mut self, id: u32, group: usize) -> Result<(), GcError> {
        *self.page_mut(id)?.available_mut() = Links {
            previous: None,
            next: self.available[group],
        };
        if let Some(first) = self.available[group] {
            self.page_mut(first)?.available_mut().previous = Some(id);
        }
        self.available[group] = Some(id);
        Ok(())
    }

    fn unlink_available(&mut self, id: u32, group: usize) -> Result<(), GcError> {
        let links = self.page(id)?.available();
        match links.previous {
            Some(previous) => self.page_mut(previous)?.available_mut().next = links.next,
            None => self.available[group] = links.next,
        }
        if let Some(next) = links.next {
            self.page_mut(next)?.available_mut().previous = links.previous;
        }
        *self.page_mut(id)?.available_mut() = Links::default();
        Ok(())
    }

    /// The caller has a live source/lease, possibly from a different heap. Check
    /// original page membership before exposing payload bytes; never probe expired pointers.
    pub(crate) fn resolve(&self, reference: ObjectRef) -> Result<CheckedObject<'_>, GcError> {
        let location = self.locate(reference)?;
        self.checked_object(reference.0, location.id, location.slot)
    }

    /// Requires the same protected live source as resolve. Membership/occupancy
    /// checks precede use of page metadata, including for foreign live sources.
    pub(crate) fn locate(&self, reference: ObjectRef) -> Result<Location<'_>, GcError> {
        // SAFETY: every caller establishes live source retention first. A foreign
        // source is still live here; membership is rejected before any payload loan.
        let header = unsafe { reference.0.as_ref() };
        let page = self
            .page(header.allocation)
            .map_err(|_| GcError::ForeignHeap)?;
        let slot = page.slot_of(reference.0).ok_or(GcError::ForeignHeap)?;
        if !page.occupied(slot) {
            return Err(GcError::Storage("object occupancy"));
        }
        Ok(Location {
            page,
            id: header.allocation,
            slot,
            descriptor: header.descriptor,
        })
    }

    /// Resolve a collector-owned physical cursor without recovering its slot
    /// from an address. Occupancy proves the header is initialized.
    pub(crate) fn object_at(&self, id: u32, slot: usize) -> Result<CheckedObject<'_>, GcError> {
        let page = self.page(id)?;
        if slot >= page.slots() || !page.occupied(slot) {
            return Err(GcError::Storage("object occupancy"));
        }
        let reference = page.pointer(slot);
        self.checked_object(reference, id, slot)
    }

    // Both entrypoints prove initialized occupied storage before dispatch lookup.
    fn checked_object(
        &self,
        reference: NonNull<ObjectHeader>,
        id: u32,
        slot: usize,
    ) -> Result<CheckedObject<'_>, GcError> {
        // SAFETY: occupied slots own initialized headers written by commit.
        let header = unsafe { reference.as_ref() };
        let descriptor = self.descriptors.get(header.descriptor)?;
        // SAFETY: registration and original page geometry bound the aligned payload.
        let pointer = unsafe { reference.cast::<u8>().add(descriptor.payload_offset) };
        Ok(CheckedObject {
            pointer,
            descriptor,
            page: id,
            slot,
        })
    }
}
