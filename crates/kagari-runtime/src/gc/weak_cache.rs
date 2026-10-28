//! Semantic weak interning with stable, admission-prepared release locators.
//!
//! Slots own the cache entries; the scalar hash index only names collision-list heads.
//! Semantic collision walks belong to lookup/admission, never collector retirement.
//! This follows generational slot ownership (as in <https://github.com/orlp/slotmap>)
//! and the runtime's existing stable metadata slots, without a secondary registry.
use crate::{
    error::RuntimeError,
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::{BuildHasher, Hash},
    mem,
    num::NonZeroUsize,
    sync::{Arc, Weak},
};

/// Conservative structural bound: checked slot lookup, fixed identity validation,
/// detachment, at most two neighbor patches or one scalar-head update, and
/// generation/free-list commits. Final Arc/Weak destruction is separate.
///
/// A scalar HashMap operation is an ordinary expected-constant lookup primitive;
/// its internal probes are not a semantic-key collision-list traversal.
pub(crate) const WEAK_CACHE_REMOVE_WORK: usize = 12;

/// One checked index representation for occupied, collision and vacant links.
/// The nonzero encoding keeps optional links one word without narrowing indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotIndex(NonZeroUsize);

impl SlotIndex {
    fn new(index: usize) -> Result<Self, RuntimeError> {
        index
            .checked_add(1)
            .and_then(NonZeroUsize::new)
            .map(Self)
            .ok_or_else(|| RuntimeError::resource_limit("weak cache slot identity"))
    }

    fn index(self) -> usize {
        self.0.get() - 1
    }
}

/// Immutable publication facts share the key's single allocation. No target is
/// retained strongly, and cloning a release token never copies or reallocates K.
#[derive(Debug)]
struct Admission<K, V> {
    slot: SlotIndex,
    generation: u64,
    hash: u64,
    key: K,
    target: Weak<V>,
}

/// A cache admission's checked slot and retained allocation identity.
///
/// Each preparation creates a fresh admission Arc, distinguishing foreign caches
/// and abandoned preparations even when their slot/generation match. Its lifetime
/// prevents address reuse until the last token drops. This is a cache locator,
/// not a graph identity or a root of V. Final key/Weak destruction belongs outside
/// cache/store borrows, just as it did for the original key allocation.
#[derive(Debug)]
pub(crate) struct WeakCacheToken<K, V> {
    admission: Arc<Admission<K, V>>,
}

impl<K, V> Clone for WeakCacheToken<K, V> {
    fn clone(&self) -> Self {
        Self {
            admission: self.admission.clone(),
        }
    }
}

impl<K, V> WeakCacheToken<K, V> {
    /// Semantic key access is for admission validation, not retirement lookup.
    pub(crate) fn key(&self) -> &K {
        &self.admission.key
    }

    pub(crate) fn is_for(&self, value: &Arc<V>) -> bool {
        Weak::ptr_eq(&self.admission.target, &Arc::downgrade(value))
    }
}

#[derive(Debug)]
struct CacheEntry<K, V> {
    token: WeakCacheToken<K, V>,
    previous: Option<SlotIndex>,
    next: Option<SlotIndex>,
}

/// Occupied generations belong to the immutable admission. Vacant slots retain
/// the next full generation and free link instead of duplicating occupied facts.
#[derive(Debug)]
enum CacheSlot<K, V> {
    Occupied(CacheEntry<K, V>),
    Vacant {
        generation: u64,
        next_free: Option<SlotIndex>,
    },
}

impl<K, V> CacheSlot<K, V> {
    fn entry(&self) -> Option<&CacheEntry<K, V>> {
        match self {
            Self::Occupied(entry) => Some(entry),
            Self::Vacant { .. } => None,
        }
    }

    fn entry_mut(&mut self) -> Option<&mut CacheEntry<K, V>> {
        match self {
            Self::Occupied(entry) => Some(entry),
            Self::Vacant { .. } => None,
        }
    }

    fn generation(&self) -> u64 {
        match self {
            Self::Occupied(entry) => entry.token.admission.generation,
            Self::Vacant { generation, .. } => *generation,
        }
    }
}

/// Original weak-cache storage. Slots contain the only entry records; the hash
/// index contains only scalar heads. Neither structure owns targets strongly.
/// Vacant slots retain only their generation/free link and are reused, including
/// while old tokens exist. Slot storage follows high-water occupancy, except for
/// generation-exhausted slots; retirement never shrinks or rebuilds either buffer.
#[derive(Debug)]
pub(crate) struct WeakCache<K, V> {
    entries: Vec<CacheSlot<K, V>>,
    heads: HashMap<u64, SlotIndex>,
    free: Option<SlotIndex>,
    hasher: RandomState,
    #[cfg(test)]
    removal_lookups: usize,
}

impl<K, V> Default for WeakCache<K, V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            heads: HashMap::new(),
            free: None,
            hasher: RandomState::new(),
            #[cfg(test)]
            removal_lookups: 0,
        }
    }
}

impl<K, V> WeakCache<K, V> {
    /// Precharge before inspecting any cache slot. Insufficient budget returns
    /// Pending without inspecting or mutating cache state. Complete with None
    /// means the original admission is already absent or the token is foreign.
    ///
    /// Removal never walks a collision chain, hashes/compares K, allocates, or
    /// resizes. Dispose the returned ownership outside cache/store borrows.
    pub(crate) fn remove(
        &mut self,
        token: &WeakCacheToken<K, V>,
        budget: &mut ScanBudget,
    ) -> (ScanStatus, Option<WeakCacheToken<K, V>>) {
        if !budget.take(WEAK_CACHE_REMOVE_WORK) {
            return (ScanStatus::Pending, None);
        }
        #[cfg(test)]
        {
            self.removal_lookups += 1;
        }
        let identity = &token.admission;
        let Some(slot) = self.entries.get(identity.slot.index()) else {
            return (ScanStatus::Complete, None);
        };
        let Some(entry) = slot.entry() else {
            return (ScanStatus::Complete, None);
        };
        if slot.generation() != identity.generation
            || !Arc::ptr_eq(&entry.token.admission, &token.admission)
        {
            return (ScanStatus::Complete, None);
        }
        (ScanStatus::Complete, Some(self.unlink(identity.slot)))
    }

    /// The caller either precharged retirement or is committing ordinary admission.
    fn unlink(&mut self, index: SlotIndex) -> WeakCacheToken<K, V> {
        let generation = self.entries[index.index()].generation();
        let next_generation = generation.checked_add(1);
        let vacant = CacheSlot::Vacant {
            generation: next_generation.unwrap_or(generation),
            next_free: next_generation.and(self.free),
        };
        let CacheSlot::Occupied(entry) = mem::replace(&mut self.entries[index.index()], vacant)
        else {
            unreachable!("occupied cache slot")
        };
        if let Some(previous) = entry.previous {
            self.entries[previous.index()]
                .entry_mut()
                .expect("occupied cache predecessor")
                .next = entry.next;
        } else if let Some(next) = entry.next {
            *self
                .heads
                .get_mut(&entry.token.admission.hash)
                .expect("cache hash head") = next;
        } else {
            self.heads.remove(&entry.token.admission.hash);
        }
        if let Some(next) = entry.next {
            self.entries[next.index()]
                .entry_mut()
                .expect("occupied cache successor")
                .previous = entry.previous;
        }
        if next_generation.is_some() {
            self.free = Some(index);
        }
        entry.token
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.heads.is_empty()
    }
}

impl<K: Hash + Eq, V> WeakCache<K, V> {
    /// This semantic collision walk is only used by normal lookup/admission.
    fn find(&self, key: &K, hash: u64) -> Option<SlotIndex> {
        let mut next = self.heads.get(&hash).copied();
        while let Some(index) = next {
            let entry = self.entries[index.index()]
                .entry()
                .expect("occupied cache chain");
            if entry.token.key() == key {
                return Some(index);
            }
            next = entry.next;
        }
        None
    }

    /// Consumers must still check graph admission after promotion; a successful
    /// Weak upgrade does not prove the target survives the current collection.
    pub(crate) fn get(&self, key: &K) -> Option<Arc<V>> {
        let index = self.find(key, self.hasher.hash_one(key))?;
        self.entries[index.index()]
            .entry()?
            .token
            .admission
            .target
            .upgrade()
    }

    #[cfg(test)]
    pub(crate) fn token(&self, key: &K) -> Option<WeakCacheToken<K, V>> {
        let index = self.find(key, self.hasher.hash_one(key))?;
        Some(self.entries[index.index()].entry()?.token.clone())
    }

    /// Finish semantic hashing/equality and reserve storage before exposing the
    /// release token to its original owner. Dropping the guard preserves entries.
    pub(crate) fn prepare_insert(
        &mut self,
        key: K,
        value: &Arc<V>,
    ) -> Result<WeakCacheInsertion<'_, K, V>, RuntimeError> {
        let hash = self.hasher.hash_one(&key);
        let existing = self.find(&key, hash);
        let replacement = existing.and_then(|index| {
            self.entries[index.index()]
                .generation()
                .checked_add(1)
                .map(|generation| (index, generation))
        });
        let (slot, generation, plan) = if let Some((index, generation)) = replacement {
            (index, generation, InsertionPlan::Replace)
        } else {
            self.heads
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("weak cache hash index"))?;
            let index = match self.free {
                Some(index) => index,
                None => SlotIndex::new(self.entries.len())?,
            };
            let generation = if index.index() == self.entries.len() {
                self.entries
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("weak cache slots"))?;
                0
            } else {
                self.entries[index.index()].generation()
            };
            (
                index,
                generation,
                InsertionPlan::Insert { replaced: existing },
            )
        };
        Ok(WeakCacheInsertion {
            cache: self,
            plan,
            token: WeakCacheToken {
                admission: Arc::new(Admission {
                    slot,
                    generation,
                    hash,
                    key,
                    target: Arc::downgrade(value),
                }),
            },
        })
    }
}

enum InsertionPlan {
    Replace,
    Insert { replaced: Option<SlotIndex> },
}

/// Exclusive admission preparation prevents intervening cache mutation. Commit
/// performs fixed-size linking only and returns displaced ownership to the caller.
pub(crate) struct WeakCacheInsertion<'a, K, V> {
    cache: &'a mut WeakCache<K, V>,
    plan: InsertionPlan,
    token: WeakCacheToken<K, V>,
}

impl<K, V> WeakCacheInsertion<'_, K, V> {
    pub(crate) fn token(&self) -> WeakCacheToken<K, V> {
        self.token.clone()
    }

    pub(crate) fn commit(self) -> Option<WeakCacheToken<K, V>> {
        let Self { cache, plan, token } = self;
        let index = token.admission.slot;
        match plan {
            InsertionPlan::Replace => {
                let slot = &mut cache.entries[index.index()];
                Some(mem::replace(
                    &mut slot.entry_mut().expect("prepared replacement").token,
                    token,
                ))
            }
            InsertionPlan::Insert { replaced } => {
                // Only a generation-exhausted replacement takes this path. Its
                // removal cannot change the prepared free-list head.
                let displaced = replaced.map(|old| cache.unlink(old));
                let next = cache.heads.insert(token.admission.hash, index);
                if let Some(next) = next {
                    cache.entries[next.index()]
                        .entry_mut()
                        .expect("cache head")
                        .previous = Some(index);
                }
                let generation = token.admission.generation;
                let entry = CacheSlot::Occupied(CacheEntry {
                    token,
                    previous: None,
                    next,
                });
                if index.index() == cache.entries.len() {
                    cache.entries.push(entry);
                } else {
                    let slot = &mut cache.entries[index.index()];
                    debug_assert_eq!(cache.free, Some(index));
                    debug_assert_eq!(slot.generation(), generation);
                    let CacheSlot::Vacant { next_free, .. } = slot else {
                        unreachable!("prepared vacant slot")
                    };
                    cache.free = *next_free;
                    *slot = entry;
                }
                displaced
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "execution-diagnostics")]
    use crate::diagnostics::allocations;
    use std::{
        hash::Hasher,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[derive(Debug)]
    struct WideKey {
        parts: Vec<usize>,
        hashes: Arc<AtomicUsize>,
        equalities: Arc<AtomicUsize>,
    }

    impl Hash for WideKey {
        fn hash<H: Hasher>(&self, state: &mut H) {
            for _ in &self.parts {
                self.hashes.fetch_add(1, Ordering::Relaxed);
            }
            0_u64.hash(state);
        }
    }

    impl PartialEq for WideKey {
        fn eq(&self, other: &Self) -> bool {
            self.parts.len() == other.parts.len()
                && self.parts.iter().zip(&other.parts).all(|(left, right)| {
                    self.equalities.fetch_add(1, Ordering::Relaxed);
                    left == right
                })
        }
    }

    impl Eq for WideKey {}

    fn retire<K, V>(
        cache: &mut WeakCache<K, V>,
        token: &WeakCacheToken<K, V>,
    ) -> Option<WeakCacheToken<K, V>> {
        let mut budget = ScanBudget::new(WEAK_CACHE_REMOVE_WORK);
        let before = cache.removal_lookups;
        let (status, removed) = cache.remove(token, &mut budget);
        assert_eq!(status, ScanStatus::Complete);
        assert_eq!(budget.spent(), WEAK_CACHE_REMOVE_WORK);
        assert_eq!(cache.removal_lookups, before + 1);
        removed
    }

    #[cfg(feature = "execution-diagnostics")]
    #[test]
    fn admission_replaces_one_allocation_and_cloned_tokens_do_not_allocate_or_root() {
        let ((mut cache, value), setup) = allocations::measure(|| {
            let mut cache = WeakCache::default();
            let value = Arc::new(7);
            cache.prepare_insert(1usize, &value).unwrap().commit();
            (cache, value)
        });
        let ((token, displaced), admission) = allocations::measure(|| {
            let insertion = cache.prepare_insert(1, &value).unwrap();
            (insertion.token(), insertion.commit().unwrap())
        });
        assert_eq!(
            admission.requests, 1,
            "one allocation replaces the key allocation, not an extra owner"
        );
        let (clone, cloning) = allocations::measure(|| token.clone());
        assert_eq!(cloning.requests, 0);
        assert_eq!(
            Arc::strong_count(&value),
            1,
            "tokens retain only a Weak target"
        );
        let (_, release) = allocations::measure(|| {
            drop(value);
            assert!(token.admission.target.upgrade().is_none());
            drop(retire(&mut cache, &token));
            drop(cache);
            assert_eq!(
                *clone.key(),
                1,
                "external tokens keep the immutable key alive after owner teardown"
            );
            drop(token);
            drop(clone);
            drop(displaced);
        });
        assert_eq!(release.requests, 0);
        assert_eq!(
            setup.net_bytes + admission.net_bytes + cloning.net_bytes + release.net_bytes,
            0
        );
    }

    #[test]
    fn wide_keys_and_over_1024_collisions_retire_with_one_fixed_precharge() {
        const COUNT: usize = 1033;
        const PARTS: usize = 2048;
        let hashes = Arc::new(AtomicUsize::new(0));
        let equalities = Arc::new(AtomicUsize::new(0));
        let mut cache = WeakCache::default();
        let value = Arc::new(7);
        let mut tokens = Vec::new();
        for index in 0..COUNT {
            let mut parts = vec![0; PARTS];
            parts[0] = index;
            let admission = cache
                .prepare_insert(
                    WideKey {
                        parts,
                        hashes: hashes.clone(),
                        equalities: equalities.clone(),
                    },
                    &value,
                )
                .unwrap();
            tokens.push(admission.token());
            assert!(admission.commit().is_none());
        }
        assert!(Arc::ptr_eq(&cache.get(tokens[0].key()).unwrap(), &value));
        assert!(hashes.load(Ordering::Relaxed) >= COUNT * PARTS);
        assert!(equalities.load(Ordering::Relaxed) >= PARTS);
        hashes.store(0, Ordering::Relaxed);
        equalities.store(0, Ordering::Relaxed);
        for available in [0, WEAK_CACHE_REMOVE_WORK - 1] {
            let mut insufficient = ScanBudget::new(available);
            let before = cache.removal_lookups;
            let (status, removed) = cache.remove(&tokens[0], &mut insufficient);
            assert_eq!(status, ScanStatus::Pending);
            assert!(removed.is_none());
            assert_eq!(insufficient.spent(), 0);
            assert_eq!(cache.removal_lookups, before);
            assert_eq!(hashes.load(Ordering::Relaxed), 0);
            assert_eq!(equalities.load(Ordering::Relaxed), 0);
        }
        // Exercise tail, head and middle unlinking after growth; remaining entries
        // must stay reachable through the repaired links until their own release.
        let special = [0, COUNT - 1, COUNT / 2];
        for index in special
            .into_iter()
            .chain((0..COUNT).filter(|index| !special.contains(index)))
        {
            assert!(retire(&mut cache, &tokens[index]).unwrap().is_for(&value));
            assert!(retire(&mut cache, &tokens[index]).is_none());
        }
        assert_eq!(hashes.load(Ordering::Relaxed), 0);
        assert_eq!(equalities.load(Ordering::Relaxed), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn old_foreign_and_abandoned_tokens_cannot_remove_equal_key_replacements() {
        let mut cache = WeakCache::default();
        let value = Arc::new(7);
        let first = cache.prepare_insert(vec![1; 2048], &value).unwrap();
        let old = first.token();
        assert!(first.commit().is_none());
        let replacement = cache.prepare_insert(vec![1; 2048], &value).unwrap();
        let current = replacement.token();
        let displaced = replacement.commit().unwrap();
        assert!(retire(&mut cache, &old).is_none());
        assert!(retire(&mut cache, &displaced).is_none());
        let abandoned = cache.prepare_insert(vec![1; 2048], &value).unwrap();
        let abandoned_token = abandoned.token();
        drop(abandoned);
        assert!(retire(&mut cache, &abandoned_token).is_none());
        let mut other = WeakCache {
            hasher: cache.hasher.clone(),
            ..WeakCache::default()
        };
        other
            .prepare_insert(vec![1; 2048], &value)
            .unwrap()
            .commit();
        let foreign = other.prepare_insert(vec![1; 2048], &value).unwrap();
        let foreign_token = foreign.token();
        foreign.commit();
        assert_eq!(foreign_token.admission.slot, current.admission.slot);
        assert_eq!(
            foreign_token.admission.generation,
            current.admission.generation
        );
        assert!(retire(&mut cache, &foreign_token).is_none());
        assert!(Arc::ptr_eq(&cache.get(&vec![1; 2048]).unwrap(), &value));
        assert!(retire(&mut cache, &current).is_some());
        let reused = cache.prepare_insert(vec![1; 2048], &value).unwrap();
        let reused_token = reused.token();
        reused.commit();
        assert_eq!(reused_token.admission.slot, abandoned_token.admission.slot);
        assert_eq!(
            reused_token.admission.generation,
            abandoned_token.admission.generation
        );
        assert!(retire(&mut cache, &abandoned_token).is_none());
        drop(value);
        assert!(cache.get(&vec![1; 2048]).is_none());
        assert!(retire(&mut cache, &reused_token).is_some());
        assert!(cache.is_empty());
    }

    #[test]
    fn vacant_slots_reuse_storage_and_only_external_tokens_retain_old_keys() {
        const WIDTH: usize = 8;
        let mut cache = WeakCache::default();
        let mut retained = None;
        let mut retained_key = None;
        let mut capacity = 0;
        for round in 0..64 {
            let mut tokens = Vec::new();
            let mut keys = Vec::new();
            for index in 0..WIDTH {
                let value = Arc::new(index);
                let weak = Arc::downgrade(&value);
                let admission = cache.prepare_insert(vec![round, index], &value).unwrap();
                let token = admission.token();
                keys.push(Arc::downgrade(&token.admission));
                tokens.push(token);
                assert!(admission.commit().is_none());
                assert_eq!(Arc::strong_count(&value), 1);
                drop(value);
                assert!(
                    weak.upgrade().is_none(),
                    "cache entries and tokens are weak targets"
                );
            }
            if round == 0 {
                capacity = cache.entries.capacity();
                retained = Some(tokens[0].clone());
                retained_key = Some(keys[0].clone());
            }
            assert_eq!(cache.entries.len(), WIDTH);
            assert_eq!(cache.entries.capacity(), capacity);
            for token in tokens {
                drop(retire(&mut cache, &token));
            }
            assert!(cache.is_empty());
            assert!(cache.entries.iter().all(|slot| slot.entry().is_none()));
            for (index, key) in keys.into_iter().enumerate() {
                assert_eq!(key.upgrade().is_some(), round == 0 && index == 0);
            }
        }
        let retained_key = retained_key.unwrap();
        assert!(retained_key.upgrade().is_some());
        drop(retained);
        assert!(retained_key.upgrade().is_none());
    }

    #[test]
    fn exhausted_generation_never_wraps_on_replacement_or_slot_reuse() {
        let mut cache = WeakCache::default();
        let value = Arc::new(7);
        let admission = cache.prepare_insert(1, &value).unwrap();
        let initial = admission.token();
        admission.commit();
        assert!(retire(&mut cache, &initial).is_some());
        // Simulate the final reusable generation before preparing immutable facts.
        let CacheSlot::Vacant { generation, .. } =
            &mut cache.entries[initial.admission.slot.index()]
        else {
            unreachable!("retired initial admission")
        };
        *generation = u64::MAX;
        let admission = cache.prepare_insert(1, &value).unwrap();
        let exhausted = admission.token();
        admission.commit();
        assert_eq!(exhausted.admission.generation, u64::MAX);
        let replacement = cache.prepare_insert(1, &value).unwrap();
        let current = replacement.token();
        let displaced = replacement.commit().unwrap();
        assert_ne!(current.admission.slot, exhausted.admission.slot);
        assert!(retire(&mut cache, &exhausted).is_none());
        assert!(retire(&mut cache, &displaced).is_none());
        assert!(retire(&mut cache, &current).is_some());
        let next = cache.prepare_insert(2, &value).unwrap();
        let next_token = next.token();
        next.commit();
        assert_ne!(next_token.admission.slot, exhausted.admission.slot);
        assert!(retire(&mut cache, &next_token).is_some());
        assert!(cache.is_empty());
    }
}
