//! Stable numeric identities in reclaimable radix blocks, with no historical free list.

#[cfg(test)]
mod tests;

use crate::error::GcError;
use std::{
    mem,
    ops::{Index, IndexMut},
};

const BITS: u32 = 6;
const LEAF_BITS: u32 = 8;
const WIDTH: usize = 1 << BITS;

/// Small tables are direct arrays. Only tables exceeding one block acquire a
/// directory. Leaves cover eight ID bits; branches cover six, up to the full u32 identity space.
/// Prepare reserves one slot for a no-fail insert; no second prepare may intervene.
#[derive(Debug)]
pub(crate) struct Table<T> {
    slots: Vec<Option<T>>,
    children: Vec<Option<Box<Self>>>,
    // Occupied slots in a leaf; full child subtrees in a branch.
    full: [u64; 4],
    shift: u32,
    len: usize,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            children: Vec::new(),
            full: [0; 4],
            shift: 0,
            len: 0,
        }
    }
}

impl<T> Table<T> {
    fn branch(shift: u32) -> Result<Self, GcError> {
        let mut children = Vec::new();
        children.try_reserve_exact(WIDTH)?;
        children.resize_with(WIDTH, || None);
        Ok(Self {
            children,
            shift,
            ..Self::default()
        })
    }

    fn is_full(&self) -> bool {
        if self.shift == 0 {
            self.full.iter().all(|word| *word == u64::MAX)
        } else {
            self.full[0] == u64::MAX
        }
    }

    pub(crate) fn prepare(&mut self) -> Result<u32, GcError> {
        if self.is_full() {
            if self.shift == 26 {
                return Err(GcError::Limit("metadata identities"));
            }
            let shift = if self.shift == 0 {
                LEAF_BITS
            } else {
                self.shift + BITS
            };
            let mut root = Self::branch(shift)?;
            root.len = self.len;
            root.children[0] = Some(Box::new(mem::take(self)));
            root.full[0] = 1;
            *self = root;
        }
        self.prepare_slot()
    }

    fn prepare_slot(&mut self) -> Result<u32, GcError> {
        if self.shift == 0 {
            let word = self
                .full
                .iter()
                .position(|word| *word != u64::MAX)
                .ok_or(GcError::Limit("metadata identities"))?;
            let index = word * 64 + (!self.full[word]).trailing_zeros() as usize;
            if index >= self.slots.len() {
                self.slots.try_reserve(index + 1 - self.slots.len())?;
                self.slots.resize_with(index + 1, || None);
            }
            return Ok(index as u32);
        }
        let index = (!self.full[0]).trailing_zeros() as usize;
        if index == WIDTH {
            return Err(GcError::Limit("metadata identities"));
        }
        if self.children[index].is_none() {
            self.children[index] = Some(Box::new(if self.shift == LEAF_BITS {
                Self::default()
            } else {
                Self::branch(self.shift - BITS)?
            }));
        }
        let offset = self.children[index]
            .as_mut()
            .expect("prepared child")
            .prepare_slot()?;
        Ok(((index as u32) << self.shift) | offset)
    }

    pub(crate) fn insert(&mut self, id: u32, value: T) {
        if self.shift == 0 {
            assert!(
                self.slots[id as usize].is_none(),
                "prepared empty metadata slot"
            );
            self.slots[id as usize] = Some(value);
            self.full[id as usize / 64] |= 1 << (id % 64);
        } else {
            let index = (id >> self.shift) as usize;
            let child = self.children[index]
                .as_mut()
                .expect("prepared metadata branch");
            child.insert(id & ((1 << self.shift) - 1), value);
            if child.is_full() {
                self.full[0] |= 1 << index;
            }
        }
        self.len += 1;
    }

    #[inline]
    pub(crate) fn get(&self, id: u32) -> Option<&T> {
        if self.shift == 0 {
            return self.slots.get(id as usize)?.as_ref();
        }
        self.children
            .get((id >> self.shift) as usize)?
            .as_ref()?
            .get(id & ((1 << self.shift) - 1))
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        if self.shift == 0 {
            return self.slots.get_mut(id as usize)?.as_mut();
        }
        self.children
            .get_mut((id >> self.shift) as usize)?
            .as_mut()?
            .get_mut(id & ((1 << self.shift) - 1))
    }

    /// Releases at most one bounded block per level, without visiting old IDs.
    pub(crate) fn remove(&mut self, id: u32) -> Option<T> {
        let value = if self.shift == 0 {
            let value = self.slots.get_mut(id as usize)?.take()?;
            self.full[id as usize / 64] &= !(1 << (id % 64));
            value
        } else {
            let index = (id >> self.shift) as usize;
            let child = self.children.get_mut(index)?.as_mut()?;
            let value = child.remove(id & ((1 << self.shift) - 1))?;
            self.full[0] &= !(1 << index);
            if child.len == 0 {
                self.children[index] = None;
            }
            value
        };
        self.len -= 1;
        if self.len == 0 {
            *self = Self::default();
        }
        Some(value)
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn storage(&self) -> (usize, usize) {
        let mut result = (
            self.slots.capacity(),
            self.slots.capacity() * size_of::<Option<T>>()
                + self.children.capacity() * size_of::<Option<Box<Self>>>(),
        );
        for child in self.children.iter().flatten() {
            let (slots, bytes) = child.storage();
            result.0 += slots;
            result.1 += bytes + size_of::<Self>();
        }
        result
    }

    pub(crate) fn trim(&mut self) {
        if self.len == 0 {
            *self = Self::default();
            return;
        }
        self.prune_empty();
        // Removing a zero prefix preserves every live ID. A high-ID survivor
        // keeps its path; only a sole child zero permits reducing lookup depth.
        while self.shift != 0
            && self.children[0]
                .as_ref()
                .is_some_and(|child| child.len == self.len)
        {
            *self = *self.children[0].take().expect("sole populated child");
        }
    }

    fn prune_empty(&mut self) {
        for child in &mut self.children {
            if child.as_ref().is_some_and(|child| child.len == 0) {
                *child = None;
            } else if let Some(child) = child {
                child.prune_empty();
            }
        }
    }
}

impl<T> Index<u32> for Table<T> {
    type Output = T;

    fn index(&self, id: u32) -> &T {
        self.get(id).expect("live metadata identity")
    }
}

impl<T> IndexMut<u32> for Table<T> {
    fn index_mut(&mut self, id: u32) -> &mut T {
        self.get_mut(id).expect("live metadata identity")
    }
}

/// Keep headroom and avoid repeated shrink/grow on small population changes.
pub(crate) fn shrink_target(capacity: usize, needed: usize) -> Option<usize> {
    if needed == 0 {
        return (capacity != 0).then_some(0);
    }
    let target = needed.saturating_mul(2).max(16);
    (capacity > target.saturating_mul(2)).then_some(target)
}

pub(crate) fn shrink_vec<T>(buffer: &mut Vec<T>, needed: usize) {
    if let Some(target) = shrink_target(buffer.capacity(), needed.max(buffer.len())) {
        buffer.shrink_to(target);
    }
}
