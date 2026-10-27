//! Preflight exact queue capacities before retiring any identity.
use super::{
    ObjectStorage, PayloadKind, PayloadLocation,
    pages::{Retired, retirement::RetiredPage},
};
use crate::gc::HeapObjectId;

pub(in crate::gc) struct Retirements {
    pub(in crate::gc) payloads: Vec<Retired>,
    pub(in crate::gc) pages: Vec<RetiredPage>,
}

impl ObjectStorage {
    fn dead_page_end(&self, dead: &[usize], start: usize) -> Option<(PayloadLocation, usize)> {
        let location = self.slots.get(dead[start])?.location()?;
        let mut end = start + 1;
        if location.kind != PayloadKind::Interface {
            while end < dead.len() {
                let next = self.slots.get(dead[end])?.location()?;
                if next.kind == PayloadKind::Interface || next.page != location.page {
                    break;
                }
                end += 1;
            }
        }
        Some((location, end))
    }

    pub(in crate::gc) fn detach_many(&mut self, owner: u64, dead: &[usize]) -> Option<Retirements> {
        let mut page_count = 0;
        let mut payload_count = 0;
        let mut position = 0;
        while position < dead.len() {
            let (location, end) = self.dead_page_end(dead, position)?;
            if location.kind != PayloadKind::Interface
                && self
                    .pages
                    .full_page_descriptor(location, end - position)
                    .is_some()
            {
                page_count += 1;
            } else {
                payload_count += end - position;
            }
            position = end;
        }
        let mut retired = Retirements {
            payloads: Vec::new(),
            pages: Vec::new(),
        };
        retired.payloads.try_reserve_exact(payload_count).ok()?;
        retired.pages.try_reserve_exact(page_count).ok()?;
        position = 0;
        while position < dead.len() {
            let (location, end) = self.dead_page_end(dead, position)?;
            if location.kind != PayloadKind::Interface
                && let Some(page) = self.pages.detach_full_page(location, end - position)
            {
                for &index in &dead[position..end] {
                    let id = HeapObjectId::new(owner, index, self.slots[index].generation);
                    self.retire_identity(index);
                    self.initialization_owners.remove(&id);
                }
                retired.pages.push(page);
            } else {
                for &index in &dead[position..end] {
                    let id = HeapObjectId::new(owner, index, self.slots[index].generation);
                    retired.payloads.push(self.detach(id)?);
                }
            }
            position = end;
        }
        Some(retired)
    }

    pub(in crate::gc) fn finish_page(&mut self, retired: &RetiredPage) -> usize {
        self.pages.finish_page(retired)
    }
}
