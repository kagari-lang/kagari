//! One bounded publication/retention protocol for immutable linked execution facts.
use crate::{
    error::RuntimeError,
    module::{LoadedModule, ModuleStoreInner},
};
use std::{
    borrow::Borrow,
    collections::{HashMap, VecDeque},
    hash::Hash,
};

const RETAINED_DESCRIPTORS: usize = 128;

#[derive(Debug)]
pub(super) struct Published<T> {
    pub(super) value: T,
    pub(super) dependencies: Vec<LoadedModule>,
}

impl<T> Published<T> {
    pub(super) fn is_available(&self, store: &ModuleStoreInner) -> bool {
        self.dependencies
            .iter()
            .all(|owner| store.resolve(owner).is_some())
    }
}

#[derive(Debug)]
pub(super) struct DescriptorIndex<S, K, V> {
    scopes: HashMap<S, HashMap<K, Published<V>>>,
    order: VecDeque<(S, K)>,
}

impl<S, K, V> Default for DescriptorIndex<S, K, V> {
    fn default() -> Self {
        Self {
            scopes: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl<S: Eq + Hash + Clone, K: Eq + Hash + Clone, V> DescriptorIndex<S, K, V> {
    pub(super) fn values(&self) -> impl Iterator<Item = &Published<V>> {
        self.scopes.values().flat_map(|entries| entries.values())
    }

    pub(super) fn get<Q: Eq + Hash + ?Sized>(&self, scope: &S, key: &Q) -> Option<&Published<V>>
    where
        K: Borrow<Q>,
    {
        self.scopes.get(scope)?.get(key)
    }

    pub(super) fn insert(
        &mut self,
        scope: S,
        key: K,
        value: Published<V>,
    ) -> Result<(), RuntimeError> {
        if self.get(&scope, &key).is_some() {
            return Err(RuntimeError::module_validation(
                "duplicate executable descriptor",
            ));
        }
        self.scopes
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("descriptor scopes"))?;
        self.order
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("descriptor order"))?;
        // Reserve before changing published edges or adding a scope. Failed
        // preparation must not accumulate empty scopes outside the entry bound.
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
            entries.remove(&expired_key);
            if entries.is_empty() && expired_scope != scope {
                self.scopes.remove(&expired_scope);
            }
        }
        self.order.push_back((scope.clone(), key.clone()));
        self.scopes
            .get_mut(&scope)
            .expect("reserved descriptor scope")
            .insert(key, value);
        Ok(())
    }
}
