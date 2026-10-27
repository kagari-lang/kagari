//! Whole homogeneous pages retain one dispatch/backing owner during destruction.
use super::{Allocation, Pages, Small};
use crate::gc::directory::{
    PayloadLocation,
    descriptors::{Descriptor, DescriptorId},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

/// Kept separate from per-object retirement: partial/mixed pages must not pay
/// for a larger token or reserve page capacity they cannot use.
pub(in crate::gc) struct RetiredPage {
    page: Box<Small>,
    descriptor: Arc<Descriptor>,
    descriptor_id: DescriptorId,
    remaining: usize,
}

impl RetiredPage {
    pub(in crate::gc) fn dispose(&mut self, mut failed: impl FnMut()) -> bool {
        let Some(destroy) = self.descriptor.destroy else {
            self.remaining = 0;
            return false;
        };
        let mut panicked = false;
        while self.remaining != 0 {
            let slot = self.page.slots.len() - self.remaining;
            self.remaining -= 1;
            if catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: the full detached page owns all initialized slots and
                // their common descriptor. Consume before dispatch, even on panic.
                unsafe { destroy(self.page.pointer(slot)) };
            }))
            .is_err()
            {
                // Quarantine before the next destructor can observe runtime state.
                failed();
                panicked = true;
            }
        }
        panicked
    }

    pub(in crate::gc) fn release(self) -> bool {
        catch_unwind(AssertUnwindSafe(|| drop(self))).is_err()
    }
}

impl Drop for RetiredPage {
    fn drop(&mut self) {
        self.dispose(|| {});
    }
}

impl Pages {
    /// The caller's occupied-directory walk groups every dead slot on a page.
    /// A matching count therefore proves the entire full page is dead. No page
    /// with an outstanding per-object retirement or mixed descriptor qualifies.
    pub(in crate::gc::directory) fn full_page_descriptor(
        &self,
        location: PayloadLocation,
        count: usize,
    ) -> Option<DescriptorId> {
        let Allocation::Small(page) = self.allocations.get(location.page as usize)? else {
            return None;
        };
        if page.free != 0
            || page.slots.len() != count
            || page.bits[page.bits.len() / 2..]
                .iter()
                .any(|&bits| bits != 0)
        {
            return None;
        }
        let id = page.slots.first()?.descriptor?;
        if !page.slots.iter().all(|slot| slot.descriptor == Some(id)) {
            return None;
        }
        (self.descriptors.get(id)?.kind == location.kind).then_some(id)
    }

    pub(in crate::gc::directory) fn detach_full_page(
        &mut self,
        location: PayloadLocation,
        count: usize,
    ) -> Option<RetiredPage> {
        let id = self.full_page_descriptor(location, count)?;
        let descriptor = self.descriptors.get(id)?.clone();
        // Full pages are not on the available list. Transfer the allocation;
        // its directory index may now be reused without affecting this owner.
        let Allocation::Small(page) = self.take_allocation(location.page) else {
            unreachable!("checked full page")
        };
        Some(RetiredPage {
            page,
            descriptor,
            descriptor_id: id,
            remaining: count,
        })
    }

    pub(in crate::gc::directory) fn finish_page(&mut self, retired: &RetiredPage) -> usize {
        assert_eq!(retired.remaining, 0);
        self.descriptors
            .release_count(retired.descriptor_id, retired.page.slots.len());
        // The token retains final backing and dispatch owners outside the borrow.
        retired.page.backing.layout.size()
    }
}
