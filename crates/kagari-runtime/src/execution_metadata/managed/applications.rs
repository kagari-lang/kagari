//! Semantic applications and admitted computations retain their real graph.
use crate::{
    execution_metadata::{
        applications::{ApplicationId, ApplicationRecord},
        managed::{Dependencies, DependencyKind, ProtectedMetadata, sources::MetadataError},
    },
    module::managed::revision::RevisionState,
    resource::ResourceState,
    value::{carriers::ScopedValue, interfaces::InterfaceObject},
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct ApplicationObject {
    id: ApplicationId,
    record: Arc<ApplicationRecord>,
    pub(super) dependencies: Dependencies,
}

impl ApplicationObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        id: ApplicationId,
        record: Arc<ApplicationRecord>,
        protected: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<Gc<'gc, Self>, MetadataError> {
        resources.poll_closed()?;
        let pending = Dependencies::collect(resources, |visit| record.trace(visit))?;
        let dependencies = Dependencies::resolve(cx, pending, protected, resources)?;
        // SAFETY: initially empty, fully initialized adjacency. Publication below
        // retains every prefix; semantic facts contain no hidden managed values.
        let owner = unsafe {
            cx.alloc_with(|_| {
                Ok(Self {
                    id,
                    record,
                    dependencies: Dependencies::empty(dependencies.len())?,
                })
            })?
        };
        // SAFETY: owner is private until this complete publication succeeds.
        unsafe {
            Dependencies::publish(
                cx,
                owner,
                |owner| &mut owner.dependencies,
                &dependencies,
                resources,
            )?;
        }
        Ok(owner)
    }

    /// An admitted computation has exactly one direct selected-body edge. Its
    /// relationship/application/environment remain separate fixed children; this
    /// does not search a receiver or reopen current publication by a code ID.
    pub(crate) fn body<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Result<Gc<'gc, RevisionState>, GcError> {
        let selected = cx
            .read(owner, |owner| match owner.record.as_ref() {
                ApplicationRecord::Admitted(computation) => {
                    Some(computation.selected.owner().program_identity())
                }
                _ => None,
            })
            .ok_or(GcError::Representation)?;
        // Admitted trace emits at most four direct targets: relationship, semantic
        // application, body and environment. Every program edge is the selection.
        let body = cx
            .follow(owner, |owner| {
                owner
                    .dependencies
                    .edges
                    .iter()
                    .find(|edge| matches!(edge.kind, DependencyKind::Revision))
                    .map(|edge| &edge.target)
            })
            .ok_or(GcError::Representation)?;
        let body = cx.downcast::<RevisionState>(body)?;
        if cx.read(body, |body| body.code().program_identity()) != selected {
            return Err(GcError::Representation);
        }
        Ok(body)
    }

    /// Interface computations own one direct interface provider; dictionary
    /// computations have a concrete caller-supplied receiver instead.
    pub(crate) fn receiver<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Result<ScopedValue<'gc>, GcError> {
        let interface = cx
            .follow(owner, |owner| {
                owner
                    .dependencies
                    .edges
                    .iter()
                    .find(|edge| matches!(edge.kind, DependencyKind::Interface))
                    .map(|edge| &edge.target)
            })
            .ok_or(GcError::Representation)?;
        let interface = cx.downcast::<InterfaceObject>(interface)?;
        InterfaceObject::data(cx, interface)
    }

    pub(crate) fn owns_receiver_data(&self) -> bool {
        match self.record.as_ref() {
            ApplicationRecord::Admitted(computation) => {
                computation.relationship.interface_receiver()
            }
            ApplicationRecord::Semantic(_)
            | ApplicationRecord::Native(_)
            | ApplicationRecord::StoredSelections(_) => false,
        }
    }

    pub(crate) fn id(&self) -> ApplicationId {
        self.id
    }

    pub(crate) fn record(&self) -> &Arc<ApplicationRecord> {
        &self.record
    }
}

// SAFETY: all direct managed relationships are initialized and scanned. The
// application descriptor contains only immutable code/type facts, scanning never
// accesses peers, and Drop releases no Host callback or owned root registration.
unsafe impl GcObject for ApplicationObject {
    type ScanState = usize;

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> usize {
        0
    }

    fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        // SAFETY: this admitted object retains the complete immutable container.
        unsafe { self.dependencies.scan(next, budget, visitor) }
    }

    fn charge(&self) -> ObjectCharge {
        self.dependencies.charge()
    }
}
