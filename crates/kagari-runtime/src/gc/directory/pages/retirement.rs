//! Bounded homogeneous slot ranges share one dispatch and backing owner.
use super::{Allocation, Pages, Small, backing::Backing};
use crate::gc::directory::{
    PayloadLocation,
    descriptors::{Descriptor, DescriptorId},
    retirement::RETIRE_LIMIT,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

/// Only this range is detached. Other initialized slots remain in the original
/// page, and retiring bits prevent reuse until destruction has completed.
pub(in crate::gc) struct RetiredRange {
    backing: Arc<Backing>,
    descriptor: Arc<Descriptor>,
    descriptor_id: DescriptorId,
    start: PayloadLocation,
    stride: usize,
    count: usize,
    remaining: usize,
}

impl RetiredRange {
    pub(in crate::gc) fn dispose(&mut self, mut failed: impl FnMut()) -> bool {
        let Some(destroy) = self.descriptor.destroy else {
            self.remaining = 0;
            return false;
        };
        let mut panicked = false;
        while self.remaining != 0 {
            let slot = self.start.slot as usize + self.count - self.remaining;
            self.remaining -= 1;
            if catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: checked initialized range, exact common descriptor and
                // retained backing. Consume before dispatch, including on unwind.
                unsafe { destroy(self.backing.pointer.add(slot * self.stride)) };
            }))
            .is_err()
            {
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

impl Drop for RetiredRange {
    fn drop(&mut self) {
        self.dispose(|| {});
    }
}

/// A range of at most 32 slots touches at most two bitmap words.
#[derive(Clone, Copy)]
struct RangeBits {
    word: usize,
    first: u64,
    second: u64,
}

impl RangeBits {
    fn new(first: usize, count: usize) -> Self {
        let first_count = count.min(64 - first % 64);
        Self {
            word: first / 64,
            first: ((1u64 << first_count) - 1) << (first % 64),
            second: (1u64 << (count - first_count)) - 1,
        }
    }

    fn words(self) -> impl Iterator<Item = (usize, u64)> {
        [(self.word, self.first), (self.word + 1, self.second)]
            .into_iter()
            .filter(|(_, mask)| *mask != 0)
    }
}

/// Exclusive, borrow-scoped proof of an initialized homogeneous interval. No
/// page mutation or descriptor reuse can intervene between validation and detach.
pub(in crate::gc::directory) struct CheckedRange<'a> {
    page: &'a mut Small,
    descriptor: &'a Arc<Descriptor>,
    descriptor_id: DescriptorId,
    start: PayloadLocation,
    count: usize,
    bits: RangeBits,
}

impl CheckedRange<'_> {
    pub(in crate::gc::directory) fn detach(self) -> RetiredRange {
        let retired = RetiredRange {
            backing: self.page.backing.clone(),
            descriptor: self.descriptor.clone(),
            descriptor_id: self.descriptor_id,
            start: self.start,
            stride: self.page.stride,
            count: self.count,
            remaining: self.count,
        };
        let first = self.start.slot as usize;
        for entry in &mut self.page.slots[first..first + self.count] {
            entry.descriptor = None;
        }
        let words = self.page.bits.len() / 2;
        for (word, mask) in self.bits.words() {
            self.page.bits[word] &= !mask;
            self.page.bits[words + word] |= mask;
        }
        retired
    }
}

impl Pages {
    /// Validate before any irreversible change. The returned proof borrows both
    /// page and descriptor until detachment consumes it without further fallibility.
    pub(in crate::gc::directory) fn prepare_range(
        &mut self,
        start: PayloadLocation,
        count: usize,
    ) -> Option<CheckedRange<'_>> {
        if count == 0 || count > RETIRE_LIMIT {
            return None;
        }
        let Allocation::Small(page) = self.allocations.get_mut(start.page as usize)? else {
            return None;
        };
        let first = start.slot as usize;
        let end = first.checked_add(count)?;
        let slots = page.slots.get(first..end)?;
        let id = slots.first()?.descriptor?;
        let descriptor = self.descriptors.get(id)?;
        let words = page.bits.len() / 2;
        let bits = RangeBits::new(first, count);
        if descriptor.kind != start.kind
            || !slots.iter().all(|slot| slot.descriptor == Some(id))
            || bits.words().any(|(word, mask)| {
                page.bits[word] & mask != mask || page.bits[words + word] & mask != 0
            })
        {
            return None;
        }
        Some(CheckedRange {
            page,
            descriptor,
            descriptor_id: id,
            start,
            count,
            bits,
        })
    }

    pub(in crate::gc::directory) fn finish_range(&mut self, retired: &RetiredRange) -> usize {
        assert_eq!(retired.remaining, 0);
        let before = self.backing_bytes;
        let page = self.small_mut(retired.start.page);
        let bits = RangeBits::new(retired.start.slot as usize, retired.count);
        let words = page.bits.len() / 2;
        let free = page
            .free
            .checked_add(retired.count)
            .expect("bounded slab free count");
        assert!(
            free <= page.slots.len(),
            "retiring range belongs to this page"
        );
        // Validate every word before changing any of them. Retiring bits prohibit
        // reuse, including after take/admission between separate sweep slices.
        for (word, mask) in bits.words() {
            assert_eq!(page.bits[word] & mask, 0, "range is detached");
            assert_eq!(page.bits[words + word] & mask, mask, "range is retiring");
        }
        for (word, mask) in bits.words() {
            page.bits[words + word] &= !mask;
        }
        let was_full = page.free == 0;
        page.free = free;
        if free == page.slots.len() {
            if !was_full {
                self.unlink_available(retired.start.page);
            }
            self.release_allocation(retired.start.page);
        } else if was_full {
            self.link_available(retired.start.page);
        }
        self.descriptors
            .release_count(retired.descriptor_id, retired.count);
        // The token retains final backing/dispatch owners outside the store borrow.
        before - self.backing_bytes
    }
}
