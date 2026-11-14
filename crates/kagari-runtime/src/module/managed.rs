//! Traced installation state; immutable executable descriptors grant no GC access.
pub(crate) mod cell;
pub(crate) mod revision;
pub(crate) mod source;
pub(crate) mod statics;

use crate::{
    host::HostRegistryId,
    module::{
        Installation, InstallationId,
        managed::{revision::RevisionState, statics::StaticState},
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

/// The publication owns one genuine root. Frames, values and revisions keep
/// ordinary edges to this state; identity lookup tables are not additional roots.
#[derive(Debug)]
pub(crate) struct InstallationState {
    registry: HostRegistryId,
    identity: InstallationId,
    current: Option<Edge<RevisionState>>,
    statics: Option<Edge<StaticState>>,
}

impl InstallationState {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        identity: &Installation,
    ) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: a new installation contains no edges. Publication below is the
        // sole writer and commits both prepared references in one closed operation.
        unsafe {
            cx.alloc_with(|_| {
                Ok(Self {
                    registry: identity.registry_owner,
                    identity: identity.id,
                    current: None,
                    statics: None,
                })
            })
        }
    }

    pub(crate) fn identity(&self) -> (HostRegistryId, InstallationId) {
        (self.registry, self.identity)
    }

    pub(crate) fn current<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Option<Gc<'gc, RevisionState>> {
        cx.follow(owner, |owner| owner.current.as_ref())
    }

    /// Closing publication removes current selection, while already retained
    /// revisions keep their own static bindings. Admission/revocation authority
    /// remains with ModuleStore; no metadata handle can reopen these edges.
    pub(crate) fn revoke<'gc>(cx: &mut Scope<'gc>, owner: Gc<'gc, Self>) -> Result<(), GcError> {
        // SAFETY: only initialized fixed edges are removed; default scan restart
        // and unchanged charge remain valid, with no allocation or peer access.
        unsafe {
            cx.with_object_mut(owner, |owner, _| {
                owner.current = None;
                owner.statics = None;
                Ok(())
            })
        }
    }

    /// Publication admission/expected-current checks belong to ModuleStore. This
    /// physical commit refuses foreign installation graphs and incomplete staging.
    pub(crate) fn publish<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        revision: Gc<'gc, RevisionState>,
    ) -> Result<(), GcError> {
        if !owner.ptr_eq(RevisionState::installation(cx, revision)) {
            return Err(GcError::Representation);
        }
        let statics = RevisionState::statics(cx, revision)
            .ok_or(GcError::Storage("revision statics are not linked"))?;
        // SAFETY: fixed traced fields, protected same-scope inputs, publication
        // before a no-fail coupled commit. Default scan restart remains in force.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let revision = mutation.edge_scoped(revision)?;
                let statics = mutation.edge_scoped(statics)?;
                owner.current = Some(revision);
                owner.statics = Some(statics);
                Ok(())
            })
        }
    }
}

// SAFETY: both initialized optional edges are always scanned, independently of
// publication status. State is a finite field coordinate. Drop touches no peer.
unsafe impl GcObject for InstallationState {
    type ScanState = u8;

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> u8 {
        0
    }

    fn scan(
        &self,
        next: &mut u8,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while *next < 2 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let status = match *next {
                // SAFETY: the admitted owner retains the selected initialized edge.
                0 => match &self.current {
                    Some(edge) => unsafe { visitor.try_visit_stored(edge, budget)? },
                    None => ScanStatus::Complete,
                },
                // SAFETY: as above, for the independent static binding container.
                _ => match &self.statics {
                    Some(edge) => unsafe { visitor.try_visit_stored(edge, budget)? },
                    None => ScanStatus::Complete,
                },
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }
}
