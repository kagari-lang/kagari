//! Unordered foundation storage. Script Eq/Hash executes outside borrowed tables.
//! Cached script hashes select a collision bucket; tokens address already checked
//! keys without invoking Rust equality as a substitute for script equality.
use crate::value::{MapKey, Value};
use std::collections::{HashMap, HashSet, TryReserveError};

#[derive(Debug, Default)]
struct CustomBuckets(HashMap<i64, Vec<i64>>);

impl CustomBuckets {
    fn prepare(&mut self, key: &MapKey) -> Result<(), TryReserveError> {
        if let Some((hash, _)) = key.custom_parts() {
            self.0.try_reserve(1)?;
            self.0.entry(hash).or_default().try_reserve(1)?;
        }
        Ok(())
    }

    fn insert(&mut self, key: &MapKey) {
        if let Some((hash, token)) = key.custom_parts() {
            self.0
                .get_mut(&hash)
                .expect("prepared collision bucket")
                .push(token);
        }
    }

    fn remove(&mut self, key: &MapKey) {
        if let Some((hash, token)) = key.custom_parts() {
            let bucket = self.0.get_mut(&hash).expect("stored collision bucket");
            bucket.retain(|stored| *stored != token);
            if bucket.is_empty() {
                self.0.remove(&hash);
            }
        }
    }

    fn tokens(&self, hash: i64) -> &[i64] {
        self.0.get(&hash).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[derive(Debug, Default)]
pub(crate) struct HashMapStorage {
    entries: HashMap<MapKey, Value>,
    buckets: CustomBuckets,
}

impl HashMapStorage {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.entries.try_reserve(additional)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&MapKey, &Value)> {
        self.entries.iter()
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &MapKey> {
        self.entries.keys()
    }

    pub(crate) fn get(&self, key: &MapKey) -> Option<&Value> {
        self.entries.get(key)
    }

    pub(crate) fn get_mut(&mut self, key: &MapKey) -> Option<&mut Value> {
        self.entries.get_mut(key)
    }

    pub(crate) fn contains_key(&self, key: &MapKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Capacity is prepared before any semantic write, including the bucket index.
    pub(crate) fn insert(
        &mut self,
        key: MapKey,
        value: Value,
    ) -> Result<Option<Value>, TryReserveError> {
        if !self.entries.contains_key(&key) {
            self.entries.try_reserve(1)?;
            self.buckets.prepare(&key)?;
            self.buckets.insert(&key);
        }
        Ok(self.entries.insert(key, value))
    }

    pub(crate) fn remove(&mut self, key: &MapKey) -> Option<Value> {
        let value = self.entries.remove(key)?;
        self.buckets.remove(key);
        Some(value)
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.buckets.0.clear();
    }

    pub(crate) fn candidate(&self, hash: i64, index: usize) -> Option<(i64, Value)> {
        let token = *self.buckets.tokens(hash).get(index)?;
        let key = MapKey::custom(hash, token, Value::Unit);
        let (key, _) = self
            .entries
            .get_key_value(&key)
            .expect("indexed custom key");
        Some((token, key.to_value()))
    }
}

#[derive(Debug, Default)]
pub(crate) struct HashSetStorage {
    entries: HashSet<MapKey>,
    buckets: CustomBuckets,
}

impl HashSetStorage {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.entries.try_reserve(additional)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &MapKey> {
        self.entries.iter()
    }

    pub(crate) fn get(&self, key: &MapKey) -> Option<&MapKey> {
        self.entries.get(key)
    }

    pub(crate) fn contains(&self, key: &MapKey) -> bool {
        self.entries.contains(key)
    }

    pub(crate) fn insert(&mut self, key: MapKey) -> Result<bool, TryReserveError> {
        if self.entries.contains(&key) {
            return Ok(false);
        }
        self.entries.try_reserve(1)?;
        self.buckets.prepare(&key)?;
        self.buckets.insert(&key);
        Ok(self.entries.insert(key))
    }

    pub(crate) fn remove(&mut self, key: &MapKey) -> bool {
        if !self.entries.remove(key) {
            return false;
        }
        self.buckets.remove(key);
        true
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.buckets.0.clear();
    }

    pub(crate) fn candidate(&self, hash: i64, index: usize) -> Option<(i64, Value)> {
        let token = *self.buckets.tokens(hash).get(index)?;
        let key = self
            .entries
            .get(&MapKey::custom(hash, token, Value::Unit))
            .expect("indexed custom key");
        Some((token, key.to_value()))
    }
}
