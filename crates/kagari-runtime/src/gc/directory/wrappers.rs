//! Dense wrapper occupancy uses checked swap-removal, never payload relocation.
use super::{ObjectStorage, PayloadKind, PayloadLocation};

impl ObjectStorage {
    pub(super) fn valid_wrapper(&self, ordinal: usize) -> bool {
        let Some(wrapper) = self.interface_directories.get(ordinal) else {
            return false;
        };
        self.slots
            .get(wrapper.directory as usize)
            .and_then(|slot| slot.location())
            == Some(PayloadLocation {
                kind: PayloadKind::Interface,
                page: ordinal as u32,
                slot: 0,
            })
    }

    /// Preflight all possible swap sources before a bounded wrapper batch commits.
    /// Earlier swaps can only move an already checked suffix owner again.
    pub(super) fn prepare_wrapper_removal(&self, count: usize) -> Option<()> {
        let first = self.interface_directories.len().checked_sub(count)?;
        (first..self.interface_directories.len())
            .all(|index| self.valid_wrapper(index))
            .then_some(())
    }

    /// The selected identity and all possible suffix sources were checked under
    /// the same exclusive borrow. Repair the moved wrapper's locator immediately.
    pub(super) fn remove_wrapper_prepared(&mut self, ordinal: usize) {
        self.interface_directories.swap_remove(ordinal);
        if let Some(moved) = self.interface_directories.get(ordinal) {
            self.slots[moved.directory as usize].location.page = ordinal as u32;
        }
        if let Some(sweep) = &mut self.sweep {
            // Reverse traversal never extends to appended newborn wrappers. A
            // removal outside sweep may shorten the captured unvisited prefix.
            sweep.wrapper = sweep.wrapper.min(self.interface_directories.len());
        }
    }
}
