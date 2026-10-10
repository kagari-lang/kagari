//! Compatibility evidence belongs to immutable type descriptions, not live values.
//! Weak producer links cannot retain executable instances or create descriptor cycles.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    frame::types::compatibility::TypeView,
    module::{
        EnumVariantRef, LoadedModule, ModuleKey, ProgramDescriptor, StructLayoutRef,
        descriptor_index::DescriptorIndex, layout_identity::LayoutIdentity,
    },
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

/// Complete nominal meaning without retaining a layout's argument bindings.
/// The immutable program does not own type arguments; this proof cannot create
/// a cycle through a supplied argument's lexical environment.
#[derive(Debug)]
pub(crate) struct NominalAdmission {
    owner: LoadedModule,
    identity: LayoutIdentity,
    kind: AggregateKind,
}

impl NominalAdmission {
    pub(crate) fn structure(layout: &StructLayoutRef) -> Option<Self> {
        Some(Self {
            owner: layout.module.clone(),
            identity: layout.canonical?,
            kind: AggregateKind::Struct,
        })
    }

    pub(crate) fn enumeration(layout: &EnumVariantRef) -> Option<Self> {
        Some(Self {
            owner: layout.module.clone(),
            identity: layout.canonical?,
            kind: AggregateKind::Enum,
        })
    }

    pub(crate) fn matches_struct(&self, actual: &StructLayoutRef, expected: TypeView<'_>) -> bool {
        self.kind == AggregateKind::Struct
            && admit(
                self.kind,
                LayoutEndpoint {
                    owner: &self.owner,
                    identity: Some(self.identity),
                },
                LayoutEndpoint {
                    owner: &actual.module,
                    identity: actual.canonical,
                },
                || actual.matches_view(expected),
            )
    }

    pub(crate) fn matches_enum(&self, actual: &EnumVariantRef, expected: TypeView<'_>) -> bool {
        // Type admission accepts every valid member; pattern admission separately
        // checks the selected tag through EnumVariantRef::matches_layout.
        self.kind == AggregateKind::Enum
            && admit(
                self.kind,
                LayoutEndpoint {
                    owner: &self.owner,
                    identity: Some(self.identity),
                },
                LayoutEndpoint {
                    owner: &actual.module,
                    identity: actual.canonical,
                },
                || actual.matches_view(expected),
            )
    }
}

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
