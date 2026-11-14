//! Candidate allocation provenance is Runtime policy, independent of GC identity.
use crate::{host::HostRegistryId, module::ModuleKey};
use std::sync::Arc;

/// One optional pointer in mutable language providers. A candidate session shares
/// immutable identity facts, never its session, executable code or managed roots.
/// Independent sessions for the same exact program are intentionally equivalent.
#[derive(Debug, Clone, Default)]
pub(crate) struct CandidateOrigin(Option<Arc<Owner>>);

#[derive(Debug)]
struct Owner {
    identity: (HostRegistryId, ModuleKey),
}

impl CandidateOrigin {
    pub(crate) fn new(identity: (HostRegistryId, ModuleKey)) -> Self {
        Self(Some(Arc::new(Owner { identity })))
    }

    pub(crate) fn matches(&self, identity: (HostRegistryId, ModuleKey)) -> bool {
        self.0
            .as_ref()
            .is_some_and(|owner| owner.identity == identity)
    }
}
