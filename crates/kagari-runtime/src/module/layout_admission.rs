//! Compatibility evidence belongs to immutable type descriptions, not live values.
//! Weak producer links cannot retain executable instances or create descriptor cycles.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::module::{
    LoadedModule, ModuleKey, ProgramDescriptor, descriptor_index::DescriptorIndex,
    layout_identity::LayoutIdentity,
};
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AggregateKind {
    Struct,
    Enum,
}

#[derive(Clone, Copy)]
pub(super) struct LayoutEndpoint<'a> {
    pub(super) owner: &'a LoadedModule,
    pub(super) identity: Option<LayoutIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AdmissionKey {
    producer: ModuleKey,
    actual: LayoutIdentity,
    expected: LayoutIdentity,
}

type AdmissionIndex = DescriptorIndex<AggregateKind, AdmissionKey, Weak<ProgramDescriptor>>;

/// Lazily allocated, bounded pure-fact evidence; safe to share with detached type facts.
#[derive(Debug, Default)]
pub(super) struct LayoutAdmissions {
    entries: OnceLock<Box<Mutex<AdmissionIndex>>>,
}

impl LayoutAdmissions {
    fn contains(&self, kind: AggregateKind, key: &AdmissionKey, producer: &LoadedModule) -> bool {
        self.entries
            .get()
            .and_then(|entries| {
                let entries = entries.try_lock().ok()?;
                entries.get(&kind, key)?.upgrade()
            })
            .is_some_and(|owner| Arc::ptr_eq(&owner, &producer.program))
    }

    fn retain(&self, kind: AggregateKind, key: AdmissionKey, producer: &LoadedModule) {
        let entries = self
            .entries
            .get_or_init(|| Box::new(Mutex::new(Default::default())));
        if let Ok(mut entries) = entries.try_lock() {
            // Contention, duplicate publication or capacity failure loses reuse only.
            // No graph comparison, callback or executable borrow crosses this lock.
            let _ = entries.insert(kind, key, Arc::downgrade(&producer.program));
        }
    }
}

/// Admit a complete layout pair once while preserving all per-value checks at callers.
/// Program identity and never-recycled layout IDs name immutable scoped meanings.
pub(super) fn admit(
    kind: AggregateKind,
    expected: LayoutEndpoint<'_>,
    actual: LayoutEndpoint<'_>,
    compatible: impl FnOnce() -> bool,
) -> bool {
    if expected.owner.registry_owner != actual.owner.registry_owner {
        return false;
    }
    let identities = expected.identity.zip(actual.identity);
    if let Some((expected_id, actual_id)) = identities
        && Arc::ptr_eq(&expected.owner.program, &actual.owner.program)
        && expected_id == actual_id
    {
        return true;
    }
    let key = identities.map(|(expected_id, actual_id)| AdmissionKey {
        producer: actual.owner.program_key(),
        actual: actual_id,
        expected: expected_id,
    });
    if let Some(key) = &key
        && expected
            .owner
            .program
            .layout_admissions
            .contains(kind, key, actual.owner)
    {
        return true;
    }
    #[cfg(feature = "execution-diagnostics")]
    diagnostics::record(Event::LayoutComparison);
    if !compatible() {
        return false;
    }
    if let Some(key) = key {
        expected
            .owner
            .program
            .layout_admissions
            .retain(kind, key, actual.owner);
    }
    true
}
