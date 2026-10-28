//! Runtime-local representation IDs. Live and retiring slots hold admission counts.
use crate::{
    error::RuntimeError,
    gc::{
        directory::PayloadKind,
        metrics::{IndivisibleMeasurements, IndivisibleSample, ObservationClock},
        weak_cache::WeakCacheToken,
    },
    module::layout_storage::{StructStorageKey, StructStorageLayout},
    native::{
        binding::NativeResult,
        storage::{
            NativeObject,
            scan::{ScanBudget, ScanStatus},
        },
    },
};
use std::{
    alloc::Layout,
    any::TypeId,
    collections::HashMap,
    mem::{self, needs_drop},
    num::NonZeroU32,
    ptr::NonNull,
    sync::{Arc, OnceLock, Weak},
};

#[derive(Debug, Clone)]
pub(in crate::gc) struct Descriptor {
    id: OnceLock<DescriptorId>,
    pub(super) kind: PayloadKind,
    pub(super) layout: Layout,
    pub(super) edges: bool,
    pub(in crate::gc) structure: Option<Arc<StructStorageLayout>>,
    pub(super) destroy: Option<unsafe fn(NonNull<u8>)>,
}

impl Descriptor {
    pub(super) fn new(
        kind: PayloadKind,
        layout: Layout,
        edges: bool,
        destroy: unsafe fn(NonNull<u8>),
    ) -> Self {
        Self {
            id: OnceLock::new(),
            kind,
            layout,
            edges,
            structure: None,
            destroy: Some(destroy),
        }
    }

    /// The existing StructStorageLayouts owner interns this token by semantic key.
    /// The physical descriptor table does not repeat semantic interning.
    pub(in crate::gc) fn structure(description: StructStorageLayout) -> NativeResult<Arc<Self>> {
        let layout = Layout::from_size_align(description.size, 1)
            .map_err(|_| RuntimeError::resource_limit("struct inline layout"))?;
        let mut descriptor = Self::new(
            PayloadKind::Struct,
            layout,
            !description.references.is_empty(),
            |_| {},
        );
        descriptor.structure = Some(Arc::new(description));
        // Encoded fields contain scalar bits and non-owning heap IDs. Their
        // metadata owner lives in this descriptor, not individual slot bytes.
        descriptor.destroy = None;
        Ok(Arc::new(descriptor))
    }

    pub(in crate::gc) fn structure_layout(&self) -> &StructStorageLayout {
        self.structure
            .as_deref()
            .expect("canonical struct representation")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gc) struct DescriptorId(NonZeroU32);

impl DescriptorId {
    fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Builtin(PayloadKind),
    Native(TypeId),
}

#[derive(Debug)]
enum CacheKey {
    Uncached,
    Builtin(PayloadKind),
    Native(TypeId),
    Structure(WeakCacheToken<StructStorageKey, Descriptor>),
}

/// Cache association belongs to the occupied representation, never a free slot.
/// Keep admission state and its release hint together rather than two independent
/// discriminants and space for an association on vacant identities.
#[derive(Debug)]
enum Entry {
    Admitted {
        descriptor: Arc<Descriptor>,
        allocations: usize,
        cache: CacheKey,
    },
    Idle {
        descriptor: Weak<Descriptor>,
        cache: CacheKey,
    },
    Free(Option<DescriptorId>),
}

impl Entry {
    fn cache_mut(&mut self) -> Option<&mut CacheKey> {
        match self {
            Self::Admitted { cache, .. } | Self::Idle { cache, .. } => Some(cache),
            Self::Free(_) => None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Descriptors {
    reusable: HashMap<Key, DescriptorId>,
    entries: Vec<Entry>,
    free: Option<DescriptorId>,
}

#[derive(Debug)]
pub(in crate::gc) struct DescriptorCursor {
    next: usize,
    end: usize,
}

pub(in crate::gc) struct DescriptorStep {
    pub(in crate::gc) status: ScanStatus,
    pub(in crate::gc) removed_structure: Option<WeakCacheToken<StructStorageKey, Descriptor>>,
}

impl Descriptors {
    fn intern(
        &mut self,
        key: Key,
        make: impl FnOnce() -> Descriptor,
    ) -> NativeResult<Arc<Descriptor>> {
        if let Some(id) = self.reusable.get(&key)
            && let Some(descriptor) = self.resolve(*id)
        {
            return Ok(descriptor);
        }
        self.reusable
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("page descriptors"))?;
        let descriptor = Arc::new(make());
        // Even a factory token whose later placement fails has a stable directory
        // slot. Its dead weak cache entry is therefore visible to bounded maintenance.
        let cache = match &key {
            Key::Builtin(kind) => CacheKey::Builtin(*kind),
            Key::Native(ty) => CacheKey::Native(*ty),
        };
        let id = self.register(&descriptor, cache)?;
        self.reusable.insert(key, id);
        Ok(descriptor)
    }

    fn resolve(&self, id: DescriptorId) -> Option<Arc<Descriptor>> {
        match self.entries.get(id.index())? {
            Entry::Admitted { descriptor, .. } => Some(descriptor.clone()),
            Entry::Idle { descriptor, .. } => descriptor.upgrade(),
            Entry::Free(_) => None,
        }
    }

    pub(super) fn builtin<T: 'static>(
        &mut self,
        kind: PayloadKind,
        edges: bool,
    ) -> NativeResult<Arc<Descriptor>> {
        self.intern(Key::Builtin(kind), || {
            let mut descriptor = Descriptor::new(kind, Layout::new::<T>(), edges, destroy::<T>);
            descriptor.destroy = needs_drop::<T>().then_some(destroy::<T>);
            descriptor
        })
    }

    pub(super) fn native(&mut self, object: &NativeObject) -> NativeResult<Arc<Descriptor>> {
        let (layout, _) = object.physical_layout()?;
        self.intern(Key::Native(object.payload_type()), || {
            Descriptor::new(PayloadKind::Native, layout, true, destroy::<NativeObject>)
        })
    }

    pub(super) fn register_structure(
        &mut self,
        descriptor: &Arc<Descriptor>,
        token: WeakCacheToken<StructStorageKey, Descriptor>,
    ) -> NativeResult<()> {
        if descriptor.structure.is_none() || !token.is_for(descriptor) {
            return Err(RuntimeError::module_validation(
                "invalid struct cache descriptor target",
            ));
        }
        self.register(descriptor, CacheKey::Structure(token))?;
        Ok(())
    }

    fn register(
        &mut self,
        descriptor: &Arc<Descriptor>,
        cache: CacheKey,
    ) -> NativeResult<DescriptorId> {
        if let Some(&id) = descriptor.id.get() {
            if self
                .resolve(id)
                .is_some_and(|current| Arc::ptr_eq(&current, descriptor))
            {
                let original = self.entries[id.index()]
                    .cache_mut()
                    .expect("resolved descriptor");
                if !matches!(cache, CacheKey::Uncached) {
                    // One descriptor owns one prepared cache admission. Replacing
                    // its token before cache commit would lose cleanup for the
                    // original entry if the new insertion is abandoned.
                    if !matches!(original, CacheKey::Uncached) {
                        return Err(RuntimeError::module_validation(
                            "representation cache admission already registered",
                        ));
                    }
                    *original = cache;
                }
                return Ok(id);
            }
            return Err(RuntimeError::module_validation(
                "foreign representation token",
            ));
        }
        let id = if let Some(id) = self.free {
            id
        } else {
            let index = u32::try_from(self.entries.len())
                .ok()
                .and_then(|index| index.checked_add(1))
                .and_then(NonZeroU32::new)
                .ok_or_else(|| RuntimeError::resource_limit("descriptor identity"))?;
            self.entries
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("descriptor directory"))?;
            DescriptorId(index)
        };
        descriptor.id.set(id).map_err(|_| {
            RuntimeError::module_validation("representation token admitted by another owner")
        })?;
        let entry = Entry::Idle {
            descriptor: Arc::downgrade(descriptor),
            cache,
        };
        if id.index() == self.entries.len() {
            self.entries.push(entry);
        } else {
            let Entry::Free(next) = self.entries[id.index()] else {
                unreachable!("free descriptor")
            };
            self.free = next;
            self.entries[id.index()] = entry;
        }
        Ok(id)
    }

    /// IDs cannot recycle while an allocation, disposal token or preparation
    /// retains the descriptor. Exact Arc identity rejects foreign/stale tokens.
    pub(super) fn admit(&mut self, descriptor: Arc<Descriptor>) -> NativeResult<DescriptorId> {
        // Avoid cloning the strong owner on the ordinary already-admitted path.
        if let Some(&id) = descriptor.id.get()
            && let Some(Entry::Admitted {
                descriptor: current,
                allocations,
                ..
            }) = self.entries.get_mut(id.index())
            && Arc::ptr_eq(current, &descriptor)
        {
            *allocations = allocations
                .checked_add(1)
                .ok_or_else(|| RuntimeError::resource_limit("descriptor admissions"))?;
            return Ok(id);
        }
        let id = self.register(&descriptor, CacheKey::Uncached)?;
        let entry = &mut self.entries[id.index()];
        let cache = mem::replace(
            entry.cache_mut().expect("registered descriptor"),
            CacheKey::Uncached,
        );
        *entry = Entry::Admitted {
            descriptor,
            allocations: 1,
            cache,
        };
        Ok(id)
    }

    pub(super) fn get(&self, id: DescriptorId) -> Option<&Arc<Descriptor>> {
        match self.entries.get(id.index())? {
            Entry::Admitted { descriptor, .. } => Some(descriptor),
            _ => None,
        }
    }

    pub(super) fn release(&mut self, id: DescriptorId) {
        self.release_count(id, 1);
    }

    pub(super) fn release_count(&mut self, id: DescriptorId, count: usize) {
        let entry = &mut self.entries[id.index()];
        let Entry::Admitted {
            descriptor,
            allocations,
            cache,
        } = entry
        else {
            unreachable!("admitted descriptor")
        };
        *allocations = allocations
            .checked_sub(count)
            .expect("admitted descriptor count");
        if *allocations == 0 {
            *entry = Entry::Idle {
                descriptor: Arc::downgrade(descriptor),
                cache: mem::replace(cache, CacheKey::Uncached),
            };
        }
    }

    pub(super) fn begin_maintenance(&self) -> DescriptorCursor {
        DescriptorCursor {
            next: 0,
            end: self.entries.len(),
        }
    }

    /// Inspect captured directory slots, not HashMap capacity. A removal reserves
    /// fixed-key unlink, weak-owner release and the caller's struct-cache dispatch.
    /// Return each opaque struct token unchanged; its owner reports identity-probe
    /// work and destroys final key/Weak owners outside this borrow.
    pub(super) fn maintenance_slice(
        &mut self,
        cursor: &mut DescriptorCursor,
        budget: &mut ScanBudget,
        mut observation: Option<(ObservationClock, &mut IndivisibleMeasurements)>,
    ) -> DescriptorStep {
        let mut removed_structure = None;
        while cursor.next < cursor.end && budget.take(1) {
            let index = cursor.next;
            if matches!(&self.entries[index], Entry::Idle { descriptor, .. } if descriptor.strong_count() == 0)
            {
                if !budget.take(3) {
                    break;
                }
                let id = DescriptorId(NonZeroU32::new(index as u32 + 1).expect("descriptor index"));
                let Entry::Idle { descriptor, cache } =
                    mem::replace(&mut self.entries[index], Entry::Free(self.free))
                else {
                    unreachable!("dead idle descriptor")
                };
                self.free = Some(id);
                let reusable = match cache {
                    CacheKey::Builtin(kind) => Some(Key::Builtin(kind)),
                    CacheKey::Native(ty) => Some(Key::Native(ty)),
                    CacheKey::Structure(token) => {
                        removed_structure = Some(token);
                        None
                    }
                    CacheKey::Uncached => None,
                };
                // A newer admission for the same fixed key may already exist.
                if let Some(key) = reusable
                    && self.reusable.get(&key) == Some(&id)
                {
                    self.reusable.remove(&key);
                }
                let started = observation.as_ref().and_then(|(clock, _)| clock.start());
                drop(descriptor);
                if let (Some(started), Some((_, destruction))) = (started, observation.as_mut()) {
                    destruction.record(IndivisibleSample::new(
                        "descriptor weak state",
                        1,
                        1,
                        Some(size_of::<Weak<Descriptor>>()),
                        started.elapsed(),
                    ));
                }
            }
            cursor.next += 1;
            if removed_structure.is_some() {
                break;
            }
        }
        DescriptorStep {
            status: if cursor.next == cursor.end {
                ScanStatus::Complete
            } else {
                ScanStatus::Pending
            },
            removed_structure,
        }
    }

    #[cfg(test)]
    pub(super) fn corrupt_structure(&mut self, id: DescriptorId) {
        let Entry::Admitted { descriptor, .. } = &mut self.entries[id.index()] else {
            unreachable!()
        };
        let descriptor = Arc::make_mut(descriptor);
        Arc::make_mut(descriptor.structure.as_mut().unwrap()).fields = Box::new([]);
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(super) fn usage(&self) -> (usize, usize, usize, usize, usize) {
        let records = self
            .entries
            .iter()
            .filter(|entry| !matches!(entry, Entry::Free(_)))
            .count();
        (
            records * (size_of::<Descriptor>() + 2 * size_of::<usize>()),
            records,
            self.entries.capacity() * size_of::<Entry>(),
            self.reusable.len(),
            self.reusable.capacity(),
        )
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(super) fn structures(&self) -> usize {
        self.entries.iter().filter(|entry| matches!(entry, Entry::Admitted { descriptor, .. } if descriptor.structure.is_some())).count()
    }
}

unsafe fn destroy<T>(pointer: NonNull<u8>) {
    // SAFETY: sealed descriptor/layout and initialized representation. Retirement
    // consumes initialization before this single dispatch, including on unwind.
    unsafe { pointer.cast::<T>().as_ptr().drop_in_place() };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Runtime,
        gc::weak_cache::{WEAK_CACHE_REMOVE_WORK, WeakCache},
        test_support::layouts::layout,
    };
    use kagari_types::{scalar::BuiltinType, ty::Ty};

    #[test]
    fn maintenance_captures_extent_and_unlinks_only_its_dead_cache_owner() {
        let mut descriptors = Descriptors::default();
        let old = descriptors
            .builtin::<u64>(PayloadKind::Range, false)
            .unwrap();
        let old_id = *old.id.get().unwrap();
        let mut cursor = descriptors.begin_maintenance();
        drop(old);
        // Replacement published before the old slot is maintained uses another
        // directory record. Retiring the old hint must not erase its cache key.
        let replacement = descriptors
            .builtin::<u64>(PayloadKind::Range, false)
            .unwrap();
        assert_ne!(*replacement.id.get().unwrap(), old_id);
        let mut tiny = ScanBudget::new(1);
        let step = descriptors.maintenance_slice(&mut cursor, &mut tiny, None);
        assert_eq!(step.status, ScanStatus::Pending);
        assert_eq!(tiny.spent(), 1);
        assert!(matches!(
            descriptors.entries[old_id.index()],
            Entry::Idle { .. }
        ));
        let runtime = Runtime::default();
        let heap = runtime.gc();
        heap.start_collection_observation().unwrap();
        heap.stats
            .borrow_mut()
            .observation
            .as_mut()
            .unwrap()
            .collecting = true;
        let clock = heap.observation_clock();
        let mut measurements = IndivisibleMeasurements::default();
        let mut budget = ScanBudget::new(4);
        let step = descriptors.maintenance_slice(
            &mut cursor,
            &mut budget,
            Some((clock, &mut measurements)),
        );
        assert_eq!(measurements.count, 1);
        assert_eq!(step.status, ScanStatus::Complete);
        assert_eq!(budget.spent(), 4);
        assert!(matches!(
            descriptors.entries[old_id.index()],
            Entry::Free(_)
        ));
        assert!(Arc::ptr_eq(
            &replacement,
            &descriptors
                .builtin::<u64>(PayloadKind::Range, false)
                .unwrap()
        ));
        let newborn = descriptors
            .builtin::<u32>(PayloadKind::HostRoot, false)
            .unwrap();
        assert_eq!(
            *newborn.id.get().unwrap(),
            old_id,
            "reused slots do not revive an exhausted cursor"
        );
        drop(newborn);
        drop(replacement);
        assert_eq!(
            descriptors
                .maintenance_slice(
                    &mut cursor,
                    &mut ScanBudget::new(4),
                    Some((clock, &mut measurements))
                )
                .status,
            ScanStatus::Complete
        );
        assert_eq!(descriptors.reusable.len(), 2);
        let mut next = descriptors.begin_maintenance();
        loop {
            let step = descriptors.maintenance_slice(
                &mut next,
                &mut ScanBudget::new(4),
                Some((clock, &mut measurements)),
            );
            if step.status == ScanStatus::Complete {
                break;
            }
        }
        assert_eq!(
            measurements.count, 3,
            "each retired weak owner is observed once"
        );
        assert_eq!(measurements.total_items, 3);
        assert_eq!(
            measurements.total_known_bytes,
            3 * size_of::<Weak<Descriptor>>() as u128
        );
        assert!(
            descriptors.reusable.is_empty(),
            "failed/unadmitted factories leave no weak cache registry residue"
        );
    }

    #[test]
    fn unadmitted_structure_returns_prepared_token_even_when_cache_commit_is_abandoned() {
        let mut runtime = Runtime::default();
        let semantic = layout(
            &mut runtime,
            "Unused",
            &[("value", Ty::Builtin(BuiltinType::I32), true)],
        );
        for commit in [false, true] {
            let descriptor =
                Descriptor::structure(StructStorageLayout::prepare(semantic.clone()).unwrap())
                    .unwrap();
            let weak = Arc::downgrade(&descriptor);
            let mut descriptors = Descriptors::default();
            let mut cache = WeakCache::default();
            let insertion = cache
                .prepare_insert(semantic.storage_key(), &descriptor)
                .unwrap();
            descriptors
                .register_structure(&descriptor, insertion.token())
                .unwrap();
            assert!(
                descriptors
                    .register_structure(&descriptor, insertion.token())
                    .is_err(),
                "a descriptor cannot lose its original admission's cleanup token"
            );
            if commit {
                assert!(insertion.commit().is_none());
            } else {
                drop(insertion);
                assert!(cache.is_empty());
            }
            let mut cursor = descriptors.begin_maintenance();
            let live = descriptors.maintenance_slice(&mut cursor, &mut ScanBudget::new(4), None);
            assert_eq!(live.status, ScanStatus::Complete);
            assert!(live.removed_structure.is_none());
            drop(descriptor);
            let mut cursor = descriptors.begin_maintenance();
            let dead = descriptors.maintenance_slice(&mut cursor, &mut ScanBudget::new(4), None);
            assert_eq!(dead.status, ScanStatus::Complete);
            let token = dead.removed_structure.unwrap();
            assert!(
                weak.upgrade().is_none(),
                "the cache token does not root its descriptor"
            );
            let (status, removed) =
                cache.remove(&token, &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK));
            assert_eq!(status, ScanStatus::Complete);
            assert_eq!(removed.is_some(), commit);
            assert!(cache.is_empty());
        }
    }

    #[test]
    fn struct_token_checks_target_and_old_retirement_preserves_replacement() {
        let mut runtime = Runtime::default();
        let semantic = layout(
            &mut runtime,
            "Replaced",
            &[("value", Ty::Builtin(BuiltinType::I32), true)],
        );
        let key = semantic.storage_key();
        let old =
            Descriptor::structure(StructStorageLayout::prepare(semantic.clone()).unwrap()).unwrap();
        let replacement =
            Descriptor::structure(StructStorageLayout::prepare(semantic).unwrap()).unwrap();
        let mut descriptors = Descriptors::default();
        let mut cache = WeakCache::default();
        let insertion = cache.prepare_insert(key.clone(), &old).unwrap();
        assert!(
            descriptors
                .register_structure(&replacement, insertion.token())
                .is_err()
        );
        assert!(
            replacement.id.get().is_none(),
            "failed target validation cannot register a descriptor"
        );
        descriptors
            .register_structure(&old, insertion.token())
            .unwrap();
        assert!(insertion.commit().is_none());
        drop(old);
        let mut old_cursor = descriptors.begin_maintenance();
        let insertion = cache.prepare_insert(key.clone(), &replacement).unwrap();
        descriptors
            .register_structure(&replacement, insertion.token())
            .unwrap();
        assert!(insertion.commit().is_some());
        let step = descriptors.maintenance_slice(&mut old_cursor, &mut ScanBudget::new(4), None);
        assert_eq!(step.status, ScanStatus::Complete);
        let (status, removed) = cache.remove(
            &step.removed_structure.unwrap(),
            &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK),
        );
        assert_eq!(status, ScanStatus::Complete);
        assert!(removed.is_none());
        assert!(Arc::ptr_eq(&cache.get(&key).unwrap(), &replacement));
        drop(replacement);
        let mut cursor = descriptors.begin_maintenance();
        let step = descriptors.maintenance_slice(&mut cursor, &mut ScanBudget::new(5), None);
        assert_eq!(step.status, ScanStatus::Complete);
        let (status, removed) = cache.remove(
            &step.removed_structure.unwrap(),
            &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK),
        );
        assert_eq!(status, ScanStatus::Complete);
        assert!(removed.is_some());
        assert!(cache.is_empty());
    }
}
