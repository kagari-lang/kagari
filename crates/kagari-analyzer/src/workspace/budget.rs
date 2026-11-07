//! Shared immutable input accounting across queued captures and retained contexts.
use crate::workspace::CaptureFailure;
use kagari_source::source::{SourceContentId, SourceFile};
use std::{
    collections::BTreeMap,
    ptr,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

#[derive(Debug)]
struct Entry {
    lease: Weak<ContentLease>,
    bytes: usize,
}

#[derive(Debug, Default)]
struct Registry {
    entries: BTreeMap<SourceContentId, Entry>,
    bytes: usize,
}

#[derive(Debug)]
struct BudgetState {
    limit: usize,
    registry: Mutex<Registry>,
}

impl BudgetState {
    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct InputBudget(Arc<BudgetState>);

#[derive(Debug)]
struct ContentLease {
    source: Arc<SourceFile>,
    budget: Arc<BudgetState>,
}

impl Drop for ContentLease {
    fn drop(&mut self) {
        let mut registry = self.budget.registry();
        let identity = self.source.content_id();
        // A zero-strong-count weak entry can be replaced while its old destructor
        // waits for this lock. That destructor must not remove the replacement.
        if registry
            .entries
            .get(&identity)
            .is_some_and(|entry| ptr::eq(entry.lease.as_ptr(), self))
        {
            let entry = registry
                .entries
                .remove(&identity)
                .expect("matched content lease");
            registry.bytes -= entry.bytes;
        }
    }
}

/// Keeping this value alive keeps every underlying content allocation charged.
#[derive(Clone, Debug, Default)]
pub(crate) struct InputLease {
    contents: Vec<Arc<ContentLease>>,
}

impl InputBudget {
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(BudgetState {
            limit,
            registry: Mutex::default(),
        }))
    }

    pub fn retain(
        &self,
        files: impl IntoIterator<Item = Arc<SourceFile>>,
    ) -> Result<InputLease, CaptureFailure> {
        let files = files
            .into_iter()
            .map(|file| (file.content_id(), file))
            .collect::<BTreeMap<_, _>>();
        let mut registry = self.0.registry();
        let expired = registry
            .entries
            .iter()
            .filter_map(|(identity, entry)| (entry.lease.strong_count() == 0).then_some(*identity))
            .collect::<Vec<_>>();
        for identity in expired {
            let entry = registry
                .entries
                .remove(&identity)
                .expect("expired registry entry");
            registry.bytes -= entry.bytes;
        }
        let mut contents = Vec::with_capacity(files.len());
        let mut fresh = Vec::new();
        let mut additional = 0usize;
        for (identity, file) in files {
            if let Some(lease) = registry
                .entries
                .get(&identity)
                .and_then(|entry| entry.lease.upgrade())
            {
                contents.push(lease);
            } else {
                if let Some(expired) = registry.entries.remove(&identity) {
                    registry.bytes -= expired.bytes;
                }
                additional = additional.saturating_add(file.retained_bytes());
                fresh.push((identity, file));
            }
        }
        if additional > self.0.limit.saturating_sub(registry.bytes) {
            // Existing lease clones may be their last owners by now. Release the
            // mutex before their destructors so rejected admission cannot deadlock.
            drop(registry);
            return Err(CaptureFailure::InvalidInput(
                "retained source input budget exhausted".into(),
            ));
        }
        for (identity, source) in fresh {
            let bytes = source.retained_bytes();
            let lease = Arc::new(ContentLease {
                source,
                budget: self.0.clone(),
            });
            registry.entries.insert(
                identity,
                Entry {
                    lease: Arc::downgrade(&lease),
                    bytes,
                },
            );
            registry.bytes += bytes;
            contents.push(lease);
        }
        drop(registry);
        Ok(InputLease { contents })
    }

    #[cfg(any(test, feature = "verification"))]
    pub fn retained_bytes(&self) -> usize {
        self.0.registry().bytes
    }
}

impl InputLease {
    pub fn retained_bytes(&self) -> usize {
        self.contents
            .iter()
            .map(|lease| lease.source.retained_bytes())
            .sum()
    }
}
