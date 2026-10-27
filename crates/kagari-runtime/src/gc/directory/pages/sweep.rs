//! Storage-owned progress: unlink repairs the cursor before an index can be reused.
use super::{Allocation, Pages};
use crate::gc::directory::{PayloadLocation, descriptors::DescriptorId};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Cursor {
    pub(super) page: Option<u32>,
    slot: u16,
}

impl Cursor {
    pub(super) fn unlink(&mut self, page: u32, next: Option<u32>) {
        if self.page == Some(page) {
            self.page = next;
            self.slot = 0;
        }
    }
}

/// A numeric position is consumed only under the same exclusive storage borrow.
/// Vacant/retiring slots still cost work, but never resolve an object identity.
pub(in crate::gc::directory) struct SweepSlot {
    pub(in crate::gc::directory) location: PayloadLocation,
    pub(in crate::gc::directory) directory: u32,
    pub(in crate::gc::directory) descriptor: DescriptorId,
    pub(in crate::gc::directory) small: bool,
}

impl Pages {
    pub(in crate::gc::directory) fn begin_sweep(&mut self) {
        // New allocations are prepended, never inserted into the unvisited suffix.
        self.sweep = Cursor {
            page: self.first_active,
            slot: 0,
        };
    }

    pub(in crate::gc::directory) fn sweep_page(&self) -> Option<u32> {
        self.sweep.page
    }

    pub(in crate::gc::directory) fn sweep_done(&self) -> bool {
        self.sweep.page.is_none()
    }

    pub(in crate::gc::directory) fn sweep_slot(&self) -> Option<SweepSlot> {
        let index = self.sweep.page?;
        let (directory, descriptor, small) = match &self.allocations[index as usize] {
            Allocation::Small(page) => {
                let slot = &page.slots[self.sweep.slot as usize];
                (slot.directory, slot.descriptor?, true)
            }
            Allocation::Large(large) => (large.directory, large.descriptor?, false),
            Allocation::Vacant(_) => unreachable!("unlink repairs sweep cursor"),
        };
        Some(SweepSlot {
            location: PayloadLocation {
                kind: self
                    .descriptors
                    .get(descriptor)
                    .expect("occupied descriptor")
                    .kind,
                page: index,
                slot: self.sweep.slot,
            },
            directory,
            descriptor,
            small,
        })
    }

    pub(in crate::gc::directory) fn advance_sweep(&mut self) {
        let index = self.sweep.page.expect("unfinished page sweep");
        let allocation = &self.allocations[index as usize];
        let count = match allocation {
            Allocation::Small(page) => page.slots.len(),
            Allocation::Large(_) => 1,
            Allocation::Vacant(_) => unreachable!("unlink repairs sweep cursor"),
        };
        self.sweep.slot += 1;
        if self.sweep.slot as usize == count {
            self.sweep.page = allocation.active().unwrap().next();
            self.sweep.slot = 0;
        }
    }
}
