//! Borrow-scoped proof for a bounded set of disjoint physical fragments.
pub(in crate::gc::directory) mod order;
use super::{Allocation, Pages, Retired, retirement::RetiredRange};
use crate::gc::directory::{PayloadLocation, descriptors::DescriptorId, retirement::RETIRE_LIMIT};

pub(in crate::gc) enum Disposal {
    Payload(Retired),
    Range(RetiredRange),
}

#[derive(Clone, Copy)]
pub(in crate::gc::directory) struct Fragment {
    pub(in crate::gc::directory) start: PayloadLocation,
    pub(in crate::gc::directory) descriptor: DescriptorId,
    pub(in crate::gc::directory) count: usize,
    pub(in crate::gc::directory) small: bool,
}

impl Fragment {
    pub(in crate::gc::directory) fn extends(
        &self,
        location: PayloadLocation,
        descriptor: DescriptorId,
    ) -> bool {
        self.small
            && self.descriptor == descriptor
            && self.start.kind == location.kind
            && self.start.page == location.page
            && self.start.slot as usize + self.count == location.slot as usize
    }
}

/// The exclusive page borrow and immutable plans prevent changes between the
/// complete preflight and mutation. No proof or borrowed page survives disposal.
pub(in crate::gc::directory) struct CheckedBatch<'a> {
    pages: &'a mut Pages,
    fragments: &'a [Option<Fragment>],
    output: &'a mut [Option<Disposal>; RETIRE_LIMIT],
}

impl Pages {
    pub(in crate::gc::directory) fn prepare_batch<'a>(
        &'a mut self,
        fragments: &'a [Option<Fragment>],
        output: &'a mut [Option<Disposal>; RETIRE_LIMIT],
    ) -> Option<CheckedBatch<'a>> {
        if fragments.len() > RETIRE_LIMIT || output.iter().any(Option::is_some) {
            return None;
        }
        let mut owners = 0usize;
        for fragment in fragments {
            let fragment = (*fragment)?;
            owners = owners.checked_add(fragment.count)?;
            if fragment.count == 0 || owners > RETIRE_LIMIT {
                return None;
            }

            if fragment.small {
                let range = self.prepare_range(fragment.start, fragment.count)?;
                if range.descriptor_id() != fragment.descriptor {
                    return None;
                }
            } else {
                if fragment.count != 1 || fragment.start.slot != 0 {
                    return None;
                }
                let Allocation::Large(large) =
                    self.allocations.get(fragment.start.page as usize)?
                else {
                    return None;
                };
                if large.header().descriptor != Some(fragment.descriptor) {
                    return None;
                }
                self.get(fragment.start)?;
            }
        }
        // Admission prepaid the bounded index proof; no physical state changes,
        // including if the proof fails or is abandoned before detachment.
        if !order::disjoint(fragments) {
            return None;
        }
        Some(CheckedBatch {
            pages: self,
            fragments,
            output,
        })
    }
}

impl CheckedBatch<'_> {
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
        Retired::Payload {
            location: fragment.start,
            backing,
            descriptor,
            descriptor_id: fragment.descriptor,
            offset: 0,
            initialized: true,
        }
    }
}
