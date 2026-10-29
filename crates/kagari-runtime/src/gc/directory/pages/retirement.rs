//! Bounded homogeneous slot ranges share one dispatch and backing owner.
use super::{Allocation, Pages, Small, backing::Backing};
use crate::gc::{
    directory::{
        PayloadLocation,
        descriptors::{Descriptor, DescriptorId},
        retirement::RETIRE_LIMIT,
    },
    metrics::{IndivisibleMeasurements, IndivisibleSample, ObservationClock},
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
    pub(in crate::gc) fn objects(&self) -> usize {
        self.count
    }

    /// Observation belongs to this call, never to detached payload ownership.
    pub(in crate::gc) fn dispose(
        &mut self,
        mut observation: Option<(ObservationClock, &mut IndivisibleMeasurements)>,
        mut failed: impl FnMut(),
    ) -> bool {
        let Some(destroy) = self.descriptor.destroy else {
            self.remaining = 0;
            return false;
        };
        let mut panicked = false;
        while self.remaining != 0 {
            let slot = self.start.slot as usize + self.count - self.remaining;
            self.remaining -= 1;
            let started = observation.as_ref().and_then(|(clock, _)| clock.start());
            let failed_drop = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: checked initialized range, exact common descriptor and
                // retained backing. Consume before dispatch, including on unwind.
                unsafe {
                    destroy(
                        self.backing
                            .pointer()
                            .add(slot * self.stride + self.descriptor.payload_offset()),
                    )
                };
            }))
            .is_err();
            if let (Some(started), Some((_, measurements))) = (started, observation.as_mut()) {
                measurements.record(IndivisibleSample::new(
                    self.descriptor.kind.observation_owner(),
                    1,
                    1,
                    Some(self.descriptor.layout.size()),
                    started.elapsed(),
                ));
            }
            if failed_drop {
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
        self.dispose(None, || {});
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
    pub(super) fn descriptor_id(&self) -> DescriptorId {
        self.descriptor_id
    }

    pub(in crate::gc::directory) fn detach(self) -> RetiredRange {
        let retired = RetiredRange {
            backing: self.page.backing().clone(),
            descriptor: self.descriptor.clone(),
            descriptor_id: self.descriptor_id,
            start: self.start,
            stride: self.page.stride(),
            count: self.count,
            remaining: self.count,
        };
        let first = self.start.slot as usize;
        let parts = self.page.parts_mut();
        if !parts.slots.is_empty() {
            for entry in &mut parts.slots[first..first + self.count] {
                entry.descriptor = None;
            }
        }
        let words = parts.bits.len() / 2;
        for (word, mask) in self.bits.words() {
            parts.bits[word] &= !mask;
            parts.bits[words + word] |= mask;
        }
        retired
    }
}

impl Pages {
    /// Only the exclusive multi-fragment proof calls this after checking every
    /// interval and rejecting overlaps. No descriptor release occurs during detach.
    pub(super) fn detach_checked_range(
        &mut self,
        start: PayloadLocation,
        descriptor_id: DescriptorId,
        count: usize,
    ) -> RetiredRange {
        let Allocation::Small(page) = &mut self.allocations[start.page as usize] else {
            unreachable!("checked small allocation")
        };
        CheckedRange {
            page,
            descriptor: self
                .descriptors
                .get(descriptor_id)
                .expect("checked descriptor"),
            descriptor_id,
            start,
            count,
            bits: RangeBits::new(start.slot as usize, count),
        }
        .detach()
    }

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
        if end > page.slot_count() {
            return None;
        }
        let id = page.slot(first)?.descriptor?;
        let descriptor = self.descriptors.get(id)?;
        let words = page.bits().len() / 2;
        let bits = RangeBits::new(first, count);
        if descriptor.kind != start.kind
            || !(first..end).all(|index| {
                page.slot(index)
                    .is_some_and(|slot| slot.descriptor == Some(id))
            })
            || bits.words().any(|(word, mask)| {
                page.bits()[word] & mask != mask || page.bits()[words + word] & mask != 0
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
        let parts = page.parts_mut();
        let words = parts.bits.len() / 2;
        let free = parts
            .state
            .free
            .checked_add(retired.count)
            .expect("bounded slab free count");
        assert!(free <= parts.count, "retiring range belongs to this page");
        // Validate every word before changing any of them. Retiring bits prohibit
        // reuse, including after take/admission between separate sweep slices.
        for (word, mask) in bits.words() {
            assert_eq!(parts.bits[word] & mask, 0, "range is detached");
            assert_eq!(parts.bits[words + word] & mask, mask, "range is retiring");
        }
        for (word, mask) in bits.words() {
            parts.bits[words + word] &= !mask;
        }
        let was_full = parts.state.free == 0;
        parts.state.free = free;
        if free == parts.count {
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
