//! Storage-owned progress: unlink repairs the cursor before an index can be reused.
use super::{Allocation, Pages};
use crate::{
    gc::directory::{PayloadLocation, descriptors::DescriptorId},
    native::storage::scan::ScanBudget,
};

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

pub(in crate::gc::directory) struct SweepSlot {
    pub(in crate::gc::directory) location: PayloadLocation,
    pub(in crate::gc::directory) directory: u32,
    pub(in crate::gc::directory) descriptor: DescriptorId,
    pub(in crate::gc::directory) small: bool,
}

/// Bitmap evidence lives only within selection's uninterrupted storage borrow.
/// Only the numeric cursor is published; between slices all evidence is reloaded.
/// Two words cover the largest class (128 slots), not the heap's live population.
pub(in crate::gc::directory) struct Scanner<'a> {
    pages: &'a Pages,
    cursor: Cursor,
    occupied: [u64; 2],
    loaded: bool,
}

impl Scanner<'_> {
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
        self.loaded = false;
    }

    /// Outer None is invalid storage; inner None is budget exhaustion or completion.
    /// Peeking does not consume an owner whose retirement reservation may still fail.
    pub(in crate::gc::directory) fn peek(
        &mut self,
        budget: &mut ScanBudget,
    ) -> Option<Option<SweepSlot>> {
        while let Some(index) = self.cursor.page {
            let (directory, descriptor, small) = match self.pages.allocations.get(index as usize)? {
                Allocation::Small(page) => {
                    if !self.loaded {
                        let bits = page.bits();
                        let words = bits.len() / 2;
                        let count = page.slots().len();
                        if self.cursor.slot as usize >= count || words > self.occupied.len() {
                            return None;
                        }
                        // Inspect both planes before using occupied bits as authoritative.
                        // Reservation/retirement is not a free or initialized slot. Tail
                        // and cardinality checks reject a missing publication bit too.
                        if !budget.take(bits.len()) {
                            return Some(None);
                        }
                        let mut used = 0;
                        for word in 0..words {
                            let occupied = bits[word];
                            let reserved = bits[words + word];
                            let valid = u64::MAX >> (64 - (count - word * 64).min(64));
                            if occupied & reserved != 0 || (occupied | reserved) & !valid != 0 {
                                return None;
                            }
                            used += (occupied | reserved).count_ones() as usize;
                            let passed = (self.cursor.slot as usize).saturating_sub(word * 64);
                            self.occupied[word] = if passed >= 64 {
                                0
                            } else {
                                occupied & (u64::MAX << passed)
                            };
                        }
                        if used != count.checked_sub(page.state().free)? {
                            return None;
                        }
                        if words == 1 {
                            self.occupied[1] = 0;
                        }
                        self.loaded = true;
                    }
                    let word = if self.occupied[0] != 0 { 0 } else { 1 };
                    if self.occupied[word] == 0 {
                        self.next_page();
                        continue;
                    }
                    self.cursor.slot =
                        (word * 64 + self.occupied[word].trailing_zeros() as usize) as u16;
                    if !budget.take(1) {
                        return Some(None);
                    }
                    let slot = page.slots().get(self.cursor.slot as usize)?;
                    (slot.directory, slot.descriptor?, true)
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
                    (large.header().directory, descriptor, false)
                }
                Allocation::Vacant(_) => return None,
            };
            return Some(Some(SweepSlot {
                location: PayloadLocation {
                    kind: self.pages.descriptors.get(descriptor)?.kind,
                    page: index,
                    slot: self.cursor.slot,
                },
                directory,
                descriptor,
                small,
            }));
        }
        Some(None)
    }

    /// Consume exactly the previously inspected owner, never an unreserved candidate.
    pub(in crate::gc::directory) fn advance(&mut self) {
        if self.loaded {
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
            loaded: false,
        }
    }

    pub(in crate::gc::directory) fn publish_sweep(&mut self, cursor: Cursor) {
        self.sweep = cursor;
    }

    pub(in crate::gc::directory) fn sweep_done(&self) -> bool {
        self.sweep.page.is_none()
    }

    #[cfg(test)]
    pub(super) fn sweep_slot(&self) -> Option<SweepSlot> {
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
