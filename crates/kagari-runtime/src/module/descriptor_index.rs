//! Bounded descriptor retention with stable generation slots for active scanning.
use crate::error::RuntimeError;
use std::{
    borrow::Borrow,
    collections::{HashMap, VecDeque},
    hash::Hash,
};

const RETAINED_DESCRIPTORS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::module) struct DescriptorSlot {
    index: usize,
    generation: u64,
}

#[derive(Debug)]
struct Entry<V> {
    generation: u64,
    retained: bool,
    value: Option<V>,
}

#[derive(Debug)]
pub(super) struct DescriptorIndex<S, K, V> {
    scopes: HashMap<S, HashMap<K, DescriptorSlot>>,
    order: VecDeque<(S, K)>,
    slots: Vec<Entry<V>>,
    free: Vec<usize>,
    scanning: Option<DescriptorSlot>,
}

impl<S, K, V> Default for DescriptorIndex<S, K, V> {
    fn default() -> Self {
        Self {
            scopes: HashMap::new(),
            order: VecDeque::new(),
            slots: Vec::new(),
            free: Vec::new(),
            scanning: None,
        }
    }
}

impl<S: Eq + Hash + Clone, K: Eq + Hash + Clone, V> DescriptorIndex<S, K, V> {
    #[cfg(test)]
    pub(super) fn values(&self) -> impl Iterator<Item = &V> {
        self.slots
            .iter()
            .filter(|entry| entry.retained)
            .filter_map(|entry| entry.value.as_ref())
    }

    pub(super) fn get<Q: Eq + Hash + ?Sized>(&self, scope: &S, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.entry(*self.scopes.get(scope)?.get(key)?)
    }

    fn entry(&self, id: DescriptorSlot) -> Option<&V> {
        let entry = self.slots.get(id.index)?;
        (entry.generation == id.generation)
            .then_some(entry.value.as_ref())
            .flatten()
    }

    pub(super) fn scan_extent(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn scan_start(&mut self, index: usize) -> Option<DescriptorSlot> {
        let entry = self.slots.get(index)?;
        if !entry.retained || entry.value.is_none() {
            return None;
        }
        assert!(self.scanning.is_none(), "one active descriptor cursor");
        let id = DescriptorSlot {
            index,
            generation: entry.generation,
        };
        self.scanning = Some(id);
        Some(id)
    }

    pub(super) fn scan_entry(&self, id: DescriptorSlot) -> Option<&V> {
        (self.scanning == Some(id))
            .then(|| self.entry(id))
            .flatten()
    }

    pub(super) fn scan_finish(&mut self, id: DescriptorSlot) {
        assert_eq!(self.scanning, Some(id), "matching descriptor continuation");
        self.scanning = None;
        if !self.slots[id.index].retained {
            self.release(id);
        }
    }

    fn release(&mut self, id: DescriptorSlot) {
        let entry = &mut self.slots[id.index];
        assert_eq!(entry.generation, id.generation);
        entry.value = None;
        if let Some(generation) = entry.generation.checked_add(1) {
            entry.generation = generation;
            self.free.push(id.index);
        }
    }

    pub(super) fn insert(&mut self, scope: S, key: K, value: V) -> Result<(), RuntimeError> {
        if self.get(&scope, &key).is_some() {
            return Err(RuntimeError::module_validation(
                "duplicate prepared descriptor",
            ));
        }
        self.scopes
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("descriptor scopes"))?;
        self.order
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("descriptor order"))?;
        // Reserve one possible extra entry before eviction: the expired entry may
        // be the active cursor's pinned backing and cannot be reused yet.
        if self.free.is_empty() {
            self.slots
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("descriptor slots"))?;
            self.free
                .try_reserve(self.slots.len() + 1 - self.free.len())
                .map_err(|_| RuntimeError::resource_limit("descriptor retirement"))?;
        }
        if let Some(entries) = self.scopes.get_mut(&scope) {
            entries
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("descriptor entries"))?;
        } else {
            let mut entries = HashMap::new();
            entries
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("descriptor entries"))?;
            self.scopes.insert(scope.clone(), entries);
        }
        if self.order.len() == RETAINED_DESCRIPTORS {
            let (expired_scope, expired_key) =
                self.order.pop_front().expect("bounded descriptor order");
            let entries = self
                .scopes
                .get_mut(&expired_scope)
                .expect("published descriptor scope");
            let expired = entries
                .remove(&expired_key)
                .expect("published descriptor entry");
            if entries.is_empty() && expired_scope != scope {
                self.scopes.remove(&expired_scope);
            }
            self.slots[expired.index].retained = false;
            if self.scanning != Some(expired) {
                self.release(expired);
            }
        }
        let index = self.free.pop().unwrap_or_else(|| {
            self.slots.push(Entry {
                generation: 0,
                retained: false,
                value: None,
            });
            self.slots.len() - 1
        });
        let entry = &mut self.slots[index];
        entry.value = Some(value);
        entry.retained = true;
        let id = DescriptorSlot {
            index,
            generation: entry.generation,
        };
        self.order.push_back((scope.clone(), key.clone()));
        self.scopes
            .get_mut(&scope)
            .expect("reserved descriptor scope")
            .insert(key, id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviction_preserves_active_backing_and_generation_until_cursor_finishes() {
        let mut index = DescriptorIndex::default();
        for key in 0..RETAINED_DESCRIPTORS {
            index.insert((), key, key).unwrap();
        }
        let cursor = index.scan_start(0).unwrap();
        index
            .insert((), RETAINED_DESCRIPTORS, RETAINED_DESCRIPTORS)
            .unwrap();
        assert!(index.get(&(), &0).is_none());
        assert_eq!(index.scan_entry(cursor), Some(&0));
        index.scan_finish(cursor);
        assert!(index.scan_entry(cursor).is_none());
        index.insert((), RETAINED_DESCRIPTORS + 1, 999).unwrap();
        assert!(index.entry(cursor).is_none());
        assert_eq!(index.values().count(), RETAINED_DESCRIPTORS);
    }
}
