//! Storage-owned progress: unlink repairs the cursor before an index can be reused.
use super::{Allocation, CheckedPayload, Pages, allocation::Small};
use crate::{
    gc::{
        directory::{PayloadLocation, descriptors::DescriptorId},
        graph::marks::PageMarks,
        object_header::ObjectHeader,
    },
    native::storage::scan::ScanBudget,
};
use std::ptr::NonNull;

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::gc::directory) struct Cursor {
    pub(in crate::gc::directory) page: Option<u32>,
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

pub(in crate::gc::directory) struct SweepSlot<'a> {
    pub(in crate::gc::directory) location: PayloadLocation,
    pub(in crate::gc::directory) descriptor: DescriptorId,
    pub(in crate::gc::directory) small: bool,
    pub(in crate::gc::directory) payload: CheckedPayload<'a>,
    pub(in crate::gc::directory) marks: &'a PageMarks,
}

/// Geometry and bitmap evidence share the page owner's uninterrupted borrow.
/// The mark reference retains that borrow; the raw base grants no independent access.
struct SmallScan<'a> {
    base: NonNull<u8>,
    stride: usize,
    count: usize,
    marks: &'a PageMarks,
}

impl<'a> SmallScan<'a> {
    /// Outer None is malformed storage; inner None is unspent bitmap admission.
    fn load(
        page: &'a Small,
        start: usize,
        occupied: &mut [u64; 2],
        budget: &mut ScanBudget,
    ) -> Option<Option<Self>> {
        let bits = page.bits();
        let words = bits.len() / 2;
        let count = page.slot_count();
        if start >= count || words > occupied.len() {
            return None;
        }
        if !budget.take(bits.len()) {
            return Some(None);
        }
        // Validate both planes, tails and cardinality before retaining evidence.
        // Reservation/retirement is neither free nor initialized payload storage.
        let mut used = 0;
        for word in 0..words {
            let initialized = bits[word];
            let reserved = bits[words + word];
            let valid = u64::MAX >> (64 - (count - word * 64).min(64));
            if initialized & reserved != 0 || (initialized | reserved) & !valid != 0 {
                return None;
            }
            used += (initialized | reserved).count_ones() as usize;
            let passed = start.saturating_sub(word * 64);
            occupied[word] = if passed >= 64 {
                0
            } else {
                initialized & (u64::MAX << passed)
            };
        }
        if used != count.checked_sub(page.state().free)? {
            return None;
        }
        if words == 1 {
            occupied[1] = 0;
        }
        Some(Some(Self {
            base: page.pointer(0),
            stride: page.stride(),
            count,
            marks: page.marks(),
        }))
    }
}

/// Only the numeric cursor is published; between slices all evidence is reloaded.
/// Two words cover the largest class (128 slots), not the heap's live population.
pub(in crate::gc::directory) struct Scanner<'a> {
    pages: &'a Pages,
    cursor: Cursor,
    occupied: [u64; 2],
    small: Option<SmallScan<'a>>,
}

impl<'a> Scanner<'a> {
    pub(in crate::gc::directory) fn cursor(&self) -> Cursor {
        self.cursor
    }

    fn next_page(&mut self) {
        let page = self.cursor.page.expect("unfinished sweep");
        self.cursor.page = self.pages.allocations[page as usize]
            .active()
            .expect("validated active allocation")
            .next();
        self.cursor.slot = 0;
        self.small = None;
    }

    /// Outer None is invalid storage; inner None is budget exhaustion or completion.
    /// Peeking does not consume an owner whose retirement reservation may still fail.
    pub(in crate::gc::directory) fn peek(
        &mut self,
        budget: &mut ScanBudget,
    ) -> Option<Option<SweepSlot<'a>>> {
        while let Some(index) = self.cursor.page {
            let (descriptor, pointer, header, small, marks) = if let Some(page) = &self.small {
                let word = if self.occupied[0] != 0 { 0 } else { 1 };
                if self.occupied[word] == 0 {
                    self.next_page();
                    continue;
                }
                let slot = word * 64 + self.occupied[word].trailing_zeros() as usize;
                self.cursor.slot = slot as u16;
                if !budget.take(1) {
                    return Some(None);
                }
                assert!(slot < page.count);
                // SAFETY: admitted immutable geometry and the checked occupied
                // bitmap select an initialized slot in the retained allocation.
                // No payload or trailer mutation can intervene in this Pages borrow.
                let pointer = unsafe { page.base.add(slot * page.stride) };
                let header = unsafe { ObjectHeader::read(pointer) };
                let descriptor = DescriptorId::from_header(header.descriptor_id)?;
                (descriptor, pointer, header, true, page.marks)
            } else {
                match self.pages.allocations.get(index as usize)? {
                    Allocation::Small(page) => {
                        let Some(scan) = SmallScan::load(
                            page,
                            self.cursor.slot as usize,
                            &mut self.occupied,
                            budget,
                        )?
                        else {
                            return Some(None);
                        };
                        self.small = Some(scan);
                        continue;
                    }
                    Allocation::Large(large) => {
                        if self.cursor.slot != 0 {
                            return None;
                        }
                        if !budget.take(1) {
                            return Some(None);
                        }
                        let Some(descriptor) = large.header().descriptor else {
                            self.next_page();
                            continue;
                        };
                        let pointer = large.backing().pointer();
                        // SAFETY: the published large descriptor proves initialized
                        // header storage under the same retained allocation borrow.
                        let header = unsafe { ObjectHeader::read(pointer) };
                        (descriptor, pointer, header, false, large.marks())
                    }
                    Allocation::Vacant(_) => return None,
                }
            };
            let description = self.pages.descriptors.get(descriptor)?;
            let location = PayloadLocation {
                kind: description.kind,
                page: index,
                slot: self.cursor.slot,
            };
            let payload =
                CheckedPayload::from_header(location, pointer, header, descriptor, description)?;
            assert!(location.slot < 128, "bounded slot block marks");
            return Some(Some(SweepSlot {
                location,
                descriptor,
                small,
                payload,
                marks,
            }));
        }
        Some(None)
    }

    /// Consume exactly the previously inspected owner, never an unreserved candidate.
    pub(in crate::gc::directory) fn advance(&mut self) {
        if self.small.is_some() {
            let slot = self.cursor.slot as usize;
            self.occupied[slot / 64] &= !(1 << (slot % 64));
            self.cursor.slot += 1;
            if self.occupied[0] != 0 || self.occupied[1] != 0 {
                return;
            }
        }
        self.next_page();
    }
}

impl Pages {
    pub(in crate::gc::directory) fn begin_sweep(&mut self) {
        // New allocations are prepended, never inserted into the unvisited suffix.
        self.sweep = Cursor {
            page: self.first_active,
            slot: 0,
        };
    }

    pub(in crate::gc::directory) fn scanner(&self) -> Scanner<'_> {
        Scanner {
            pages: self,
            cursor: self.sweep,
            occupied: [0; 2],
            small: None,
        }
    }

    pub(in crate::gc::directory) fn publish_sweep(&mut self, cursor: Cursor) {
        self.sweep = cursor;
    }

    pub(in crate::gc::directory) fn sweep_done(&self) -> bool {
        self.sweep.page.is_none()
    }

    #[cfg(test)]
    pub(super) fn sweep_slot(&self) -> Option<SweepSlot<'_>> {
        self.scanner()
            .peek(&mut ScanBudget::new(usize::MAX))
            .unwrap()
    }

    #[cfg(test)]
    pub(super) fn advance_sweep(&mut self) {
        let mut scanner = self.scanner();
        assert!(
            scanner
                .peek(&mut ScanBudget::new(usize::MAX))
                .unwrap()
                .is_some()
        );
        scanner.advance();
        self.sweep = scanner.cursor();
    }
}
