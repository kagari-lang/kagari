//! Reusable window identities with in-slot free links and bounded trailing storage.
use super::Window;
use std::collections::TryReserveError;

const NO_SLOT: usize = usize::MAX;

#[derive(Debug)]
enum Slot {
    Occupied(Window),
    Vacant { previous: usize, next: usize },
}

impl Slot {
    fn window(&self) -> Option<&Window> {
        match self {
            Self::Occupied(window) => Some(window),
            Self::Vacant { .. } => None,
        }
    }

    fn window_mut(&mut self) -> Option<&mut Window> {
        match self {
            Self::Occupied(window) => Some(window),
            Self::Vacant { .. } => None,
        }
    }
}

#[derive(Debug)]
pub(super) struct Windows {
    slots: Vec<Slot>,
    free_head: usize,
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_head: NO_SLOT,
        }
    }
}

impl Windows {
    /// Reserve before parameter/root publication. Reusing a hole needs no growth.
    #[inline]
    pub(super) fn reserve_slot(&mut self) -> Result<(), TryReserveError> {
        if self.free_head == NO_SLOT {
            self.slots.try_reserve(1)
        } else {
            Ok(())
        }
    }

    // Keep publication visible to frame admission so the Window does not need a
    // separate stack temporary and copy across this small ownership boundary.
    #[inline(always)]
    pub(super) fn insert(&mut self, window: Window) -> usize {
        if self.free_head == NO_SLOT {
            let index = self.slots.len();
            self.slots.push(Slot::Occupied(window));
            index
        } else {
            let index = self.free_head;
            self.unlink_free(index);
            self.slots[index] = Slot::Occupied(window);
            index
        }
    }

    /// The owner has already checked the window identity. Drop dependencies now;
    /// no retired executable may stay alive just because its identity is reusable.
    pub(super) fn retire(&mut self, index: usize) {
        if index + 1 == self.slots.len() {
            self.slots.pop();
            while matches!(self.slots.last(), Some(Slot::Vacant { .. })) {
                self.unlink_free(self.slots.len() - 1);
                self.slots.pop();
            }
        } else {
            self.slots[index] = Slot::Vacant {
                previous: NO_SLOT,
                next: self.free_head,
            };
            if self.free_head != NO_SLOT {
                let Slot::Vacant { previous, .. } = &mut self.slots[self.free_head] else {
                    unreachable!("free head is vacant");
                };
                *previous = index;
            }
            self.free_head = index;
        }
    }

    // Backlinks let trailing holes retire in any order without searching either
    // the live windows or the free chain. Links exist only in vacant slots.
    fn unlink_free(&mut self, index: usize) {
        let Slot::Vacant { previous, next } = self.slots[index] else {
            unreachable!("free link is vacant");
        };
        if previous == NO_SLOT {
            self.free_head = next;
        } else {
            let Slot::Vacant { next: link, .. } = &mut self.slots[previous] else {
                unreachable!("previous free link is vacant");
            };
            *link = next;
        }
        if next != NO_SLOT {
            let Slot::Vacant { previous: link, .. } = &mut self.slots[next] else {
                unreachable!("next free link is vacant");
            };
            *link = previous;
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<&Window> {
        self.slots.get(index)?.window()
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut Window> {
        self.slots.get_mut(index)?.window_mut()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &Window> {
        self.slots.iter().filter_map(Slot::window)
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }
}
