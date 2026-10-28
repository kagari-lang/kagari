//! Library-owned indexed hash storage and callback-aware operations.
pub(crate) mod cursor;
pub(crate) mod input;
pub(crate) mod operations;
#[cfg(test)]
mod tests;

use kagari_contract::standard::RuntimePrimitive;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        payload::NativeType,
        storage::{
            NativePayload,
            access::StorageAccess,
            scan::{ScanBudget, ScanStatus},
        },
        storage_type::StorageType,
        typed::NativeContext,
    },
    resource::HeapCharge,
    value::Value,
};
use std::{
    collections::{HashMap, hash_map::Entry as BucketEntry},
    fmt::Debug,
    hash::{BuildHasherDefault, Hasher},
};

/// The index key is already the complete script hash. Reuse those bits; this
/// index never hashes or compares a script value and cannot change Hash/Eq order.
#[derive(Debug, Default)]
pub(crate) struct StoredHash(u64);

impl Hasher for StoredHash {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0 = u64::from_ne_bytes(bytes.try_into().expect("i64 hash-index key"));
    }

    fn write_i64(&mut self, hash: i64) {
        self.0 = hash as u64;
    }
}

pub(crate) trait HashValue: Debug {
    const FIELDS: usize;

    fn from_value(value: Option<Value>) -> NativeResult<Self>
    where
        Self: Sized;

    fn as_value(&self) -> Option<&Value>;
}

impl HashValue for Value {
    const FIELDS: usize = 2;

    fn from_value(value: Option<Value>) -> NativeResult<Self> {
        value.ok_or_else(invalid)
    }

    fn as_value(&self) -> Option<&Value> {
        Some(self)
    }
}

impl HashValue for () {
    const FIELDS: usize = 1;

    fn from_value(value: Option<Value>) -> NativeResult<Self> {
        if value.is_some() {
            return Err(invalid());
        }
        Ok(())
    }

    fn as_value(&self) -> Option<&Value> {
        None
    }
}

#[derive(Debug)]
pub(crate) struct Entry<V> {
    pub(crate) key: Value,
    pub(crate) value: V,
    hash: i64,
    previous: usize,
    pub(crate) next: usize,
}

/// The index owns only scalar positions. All script references live in slots;
/// dense entry relocation republishes moved edges before the old slot is removed.
#[derive(Debug)]
pub(crate) struct HashTable<V> {
    pub(crate) key: StorageType,
    // Immutable construction-time builtin evidence; stored callbacks retain reload routes.
    pub(crate) builtin_keys: bool,
    pub(crate) value: Option<StorageType>,
    pub(crate) slots: Vec<Entry<V>>,
    pub(crate) buckets: HashMap<i64, usize, BuildHasherDefault<StoredHash>>,
}

pub(crate) trait HashPayload: NativePayload {
    type Value: HashValue;

    fn table(&self) -> &HashTable<Self::Value>;

    fn table_mut(&mut self) -> &mut HashTable<Self::Value>;
}

impl<V: HashValue> HashTable<V> {
    pub(crate) fn empty<P: NativePayload>(
        cx: &NativeContext<'_>,
        ty: &NativeType<P>,
    ) -> NativeResult<Self> {
        let conversion = cx.conversion_scope(ty.owner())?;
        let key_argument = conversion.parameter(ty.type_argument(), 0)?;
        let builtin_keys = match (cx.selected_at(0), cx.selected_at(1)) {
            (Ok(hash), Ok(equal)) => {
                if hash.parameters() != [key_argument.ty().clone()]
                    || equal.parameters() != [key_argument.ty().clone(), key_argument.ty().clone()]
                {
                    return Err(invalid());
                }
                hash.primitive() == Some(RuntimePrimitive::ValueHash)
                    && equal.primitive() == Some(RuntimePrimitive::ValueEq)
            }
            // A bare storage fixture has no callable authority. Only construction
            // with verified Hash/Eq slots may enable the builtin path.
            _ => false,
        };
        let key = StorageType::prepare_scoped(key_argument, ty.owner())?;
        let value = (V::FIELDS == 2)
            .then(|| {
                StorageType::prepare_scoped(
                    conversion.parameter(ty.type_argument(), 1)?,
                    ty.owner(),
                )
            })
            .transpose()?;
        Ok(Self {
            key,
            builtin_keys,
            value,
            slots: Vec::new(),
            buckets: HashMap::default(),
        })
    }

    pub(crate) fn scan<'a>(
        &'a self,
        state: &mut (usize, usize),
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        while state.0 < state.1 {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            if let Some(entry) = self.slots.get(state.0 / V::FIELDS) {
                if state.0.is_multiple_of(V::FIELDS) {
                    visit(&entry.key);
                } else if let Some(value) = entry.value.as_value() {
                    visit(value);
                }
            }
            state.0 += 1;
        }
        ScanStatus::Complete
    }

    pub(crate) fn charge(&self) -> HeapCharge {
        // Vec capacities are known backing. std's HashMap exposes entry capacity,
        // not buckets/control bytes, so that index term is a pressure estimate.
        HeapCharge::new(
            self.slots.len(),
            self.slots.capacity() * size_of::<Entry<V>>()
                + self.buckets.capacity() * size_of::<(i64, usize)>(),
        )
    }

    pub(crate) fn reserve(&mut self, _hash: i64) -> NativeResult<()> {
        self.slots.try_reserve(1).map_err(|_| capacity())?;
        self.buckets.try_reserve(1).map_err(|_| capacity())
    }

    pub(crate) fn insert(&mut self, hash: i64, key: Value, value: V) {
        let slot = self.slots.len();
        // Circular links retain insertion order without a separate tail allocation.
        let (previous, next) = match self.buckets.entry(hash) {
            BucketEntry::Occupied(bucket) => {
                let head = *bucket.get();
                let tail = self.slots[head].previous;
                self.slots[tail].next = slot;
                self.slots[head].previous = slot;
                (tail, head)
            }
            BucketEntry::Vacant(bucket) => {
                bucket.insert(slot);
                (slot, slot)
            }
        };
        self.slots.push(Entry {
            key,
            value,
            hash,
            previous,
            next,
        });
    }

    pub(crate) fn next(&self, hash: i64, slot: usize) -> Option<usize> {
        let next = self.slots[slot].next;
        (next != self.buckets[&hash]).then_some(next)
    }

    pub(crate) fn remove(&mut self, slot: usize, access: &mut StorageAccess<'_>) {
        let hash = self.slots[slot].hash;
        let head = self.buckets[&hash];
        let tail = self.slots[head].previous;
        if self.slots[slot].next == slot {
            self.buckets.remove(&hash);
        } else {
            // Preserve the old bucket's swap-remove visitation order: its tail
            // takes the removed position, before dense storage relocation below.
            let previous = self.slots[tail].previous;
            self.slots[previous].next = head;
            self.slots[head].previous = previous;
            if slot != tail {
                let (previous, next) = (self.slots[slot].previous, self.slots[slot].next);
                if next == slot {
                    self.slots[tail].previous = tail;
                    self.slots[tail].next = tail;
                } else {
                    self.slots[previous].next = tail;
                    self.slots[next].previous = tail;
                    self.slots[tail].previous = previous;
                    self.slots[tail].next = next;
                }
                if head == slot {
                    self.buckets.insert(hash, tail);
                }
            }
        }
        let last = self.slots.len() - 1;
        if slot != last {
            let moved = &self.slots[last];
            // SAFETY: Both edges still occupy this same traced payload. Publish
            // before relocating behind any partially scanned prefix.
            unsafe {
                access.republish_existing(&moved.key);
                if let Some(value) = moved.value.as_value() {
                    access.republish_existing(value);
                }
            }
            let (hash, previous, next) = (moved.hash, moved.previous, moved.next);
            if next == last {
                self.slots[last].previous = slot;
                self.slots[last].next = slot;
            } else {
                self.slots[previous].next = slot;
                self.slots[next].previous = slot;
            }
            if self.buckets[&hash] == last {
                self.buckets.insert(hash, slot);
            }
        }
        self.slots.swap_remove(slot);
    }

    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.buckets.clear();
    }
}

pub(crate) fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid library hash collection")
}

fn capacity() -> RuntimeError {
    RuntimeError::resource_limit("hash collection backing capacity")
}
