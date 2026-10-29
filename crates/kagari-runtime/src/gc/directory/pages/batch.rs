//! Borrow-scoped proof for a bounded set of disjoint physical fragments.
pub(in crate::gc::directory) mod order;
use super::{
    Allocation, Pages, Retired,
    retirement::RetiredRange,
    sweep::{Cursor, SweepSlot},
};
use crate::{
    gc::directory::{PayloadLocation, descriptors::DescriptorId, retirement::RETIRE_LIMIT},
    native::storage::scan::{ScanBudget, ScanStatus},
};

pub(in crate::gc) enum Disposal {
    Payload(Retired),
    Range(RetiredRange),
}

#[derive(Clone, Copy)]
struct Fragment {
    start: PayloadLocation,
    descriptor: DescriptorId,
    count: usize,
    small: bool,
}

impl Fragment {
    fn extends(&self, location: PayloadLocation, descriptor: DescriptorId) -> bool {
        self.small
            && self.descriptor == descriptor
            && self.start.kind == location.kind
            && self.start.page == location.page
            && self.start.slot as usize + self.count == location.slot as usize
    }
}

/// Caller-owned workspace. Only physical discovery can append fragments; no
/// unchecked locations enter the selection or escape its page borrow as proof.
pub(in crate::gc::directory) struct Selection {
    fragments: [Option<Fragment>; RETIRE_LIMIT],
    count: usize,
}

impl Selection {
    #[inline(always)]
    pub(in crate::gc::directory) fn new() -> Self {
        Self {
            fragments: [None; RETIRE_LIMIT],
            count: 0,
        }
    }
}

pub(in crate::gc::directory) enum Decision {
    Survivor,
    Retire,
    Pending,
}

/// The exclusive page borrow and immutable plans prevent changes between the
/// complete preflight and mutation. No proof or borrowed page survives disposal.
pub(in crate::gc::directory) struct CheckedBatch<'a> {
    pages: &'a mut Pages,
    fragments: &'a [Option<Fragment>],
    output: &'a mut [Option<Disposal>; RETIRE_LIMIT],
    cursor: Cursor,
}

impl Pages {
    /// Select checked owners without publishing progress or detaching payloads.
    /// The callback receives the next fragment count when admission needs a new
    /// interval, or None when the candidate extends the last interval. It must
    /// reserve retirement work before returning Retire. Pending leaves that owner
    /// unconsumed. Invalid storage, selection or callback failure abandons the
    /// whole preview; the returned proof exclusively borrows pages until commit.
    pub(in crate::gc::directory) fn prepare_batch<'a>(
        &'a mut self,
        selection: &'a mut Selection,
        output: &'a mut [Option<Disposal>; RETIRE_LIMIT],
        budget: &mut ScanBudget,
        mut select: impl FnMut(&SweepSlot<'_>, Option<usize>, &mut ScanBudget) -> Option<Decision>,
    ) -> Option<CheckedBatch<'a>> {
        if selection.count != 0 || output.iter().any(Option::is_some) {
            return None;
        }
        let mut scanner = self.scanner();
        let mut owners = 0;
        while owners < RETIRE_LIMIT {
            let Some(slot) = scanner.peek(budget)? else {
                break;
            };
            let extends = slot.small
                && selection.count != 0
                && selection.fragments[selection.count - 1]
                    .is_some_and(|fragment| fragment.extends(slot.location, slot.descriptor));
            let new_fragment = (!extends).then_some(selection.count + 1);
            match select(&slot, new_fragment, budget)? {
                Decision::Pending => break,
                Decision::Survivor => {}
                Decision::Retire => {
                    if extends {
                        selection.fragments[selection.count - 1].as_mut()?.count += 1;
                    } else {
                        selection.fragments[selection.count] = Some(Fragment {
                            start: slot.location,
                            descriptor: slot.descriptor,
                            count: 1,
                            small: slot.small,
                        });
                        selection.count += 1;
                    }
                    owners += 1;
                }
            }
            scanner.advance();
        }
        let cursor = scanner.cursor();
        let fragments = &selection.fragments[..selection.count];
        // Each occupied slot, header and descriptor was checked by this scanner.
        // Homogeneous intervals only extend by one adjacent checked slot. The same
        // borrow covers selection and commit, so physical admission is not repeated.
        // Admission prepaid the bounded index proof; no physical state changes,
        // including if the proof fails or is abandoned before detachment.
        if !order::disjoint(fragments) {
            return None;
        }
        Some(CheckedBatch {
            pages: self,
            fragments,
            output,
            cursor,
        })
    }
}

impl CheckedBatch<'_> {
    pub(in crate::gc::directory) fn status(&self) -> ScanStatus {
        if self.cursor.page.is_none() {
            ScanStatus::Complete
        } else {
            ScanStatus::Pending
        }
    }

    pub(in crate::gc::directory) fn fragments(&self) -> usize {
        self.fragments.len()
    }

    pub(in crate::gc::directory) fn detach(mut self) {
        for ordinal in 0..self.fragments.len() {
            let fragment = self.fragments[ordinal].expect("checked physical fragment");
            let token = if fragment.small {
                Disposal::Range(self.pages.detach_checked_range(
                    fragment.start,
                    fragment.descriptor,
                    fragment.count,
                ))
            } else {
                Disposal::Payload(self.detach_prepared_large(fragment))
            };
            // Every destination was checked empty before the first detach. Publishing
            // each token immediately leaves unwind cleanup with an initialized prefix.
            self.output[ordinal] = Some(token);
        }
        self.pages.publish_sweep(self.cursor);
    }

    fn detach_prepared_large(&mut self, fragment: Fragment) -> Retired {
        let descriptor = self
            .pages
            .descriptors
            .get(fragment.descriptor)
            .expect("checked admitted descriptor")
            .clone();
        let Allocation::Large(large) = &mut self.pages.allocations[fragment.start.page as usize]
        else {
            unreachable!("checked large allocation")
        };
        let backing = large.backing().clone();
        // Acquire both owners before mutation. Construct initialized ownership only
        // after clearing publication, so no early unwind can destroy a live payload.
        *large.descriptor_mut() = None;
        Retired {
            location: fragment.start,
            backing,
            descriptor,
            descriptor_id: fragment.descriptor,
            offset: 0,
            initialized: true,
        }
    }
}
