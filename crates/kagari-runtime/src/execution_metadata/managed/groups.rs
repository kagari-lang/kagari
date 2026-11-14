//! One operation dictionary retains actual supplying revisions and applications.
use crate::{
    execution_metadata::{
        MetadataEdge,
        groups::group::OperationGroup,
        managed::{Dependencies, ProtectedMetadata, sources::MetadataError},
    },
    resource::ResourceState,
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct OperationGroupObject {
    record: Arc<OperationGroup>,
    pub(super) dependencies: Dependencies,
}

impl OperationGroupObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        record: Arc<OperationGroup>,
        protected: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<Gc<'gc, Self>, MetadataError> {
        resources.poll_closed()?;
        let mut pending = Vec::new();
        for (index, operation) in record.entries().iter().enumerate() {
            if index.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            pending.try_reserve(4)?;
            pending.push(MetadataEdge::Program(&operation.owner));
            pending.extend(operation.application.map(MetadataEdge::Application));
            if let Some(generic) = &operation.generic {
                pending.push(MetadataEdge::Binding(&generic.receiver_table));
                pending.extend(
                    generic
                        .receiver_environment
                        .iter()
                        .map(MetadataEdge::EnvironmentFacts),
                );
            }
        }
        // Group membership is physical containment, not a self-ID edge. Individual
        // members keep their visibility/evidence while sharing dictionary storage.
        let dependencies = Dependencies::resolve(cx, pending, protected, resources)?;
        // SAFETY: initially empty, fully initialized adjacency. The publication
        // below scans every prefix and exposes no partially prepared dictionary.
        let owner = unsafe {
            cx.alloc_with(|_| {
                Ok(Self {
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

    pub(crate) fn record(&self) -> &Arc<OperationGroup> {
        &self.record
    }
}

// SAFETY: all supplying program/application/environment edges are in dependencies.
// Member facts and indexes are immutable, the scan coordinate is stable, and no
// destructor touches a peer. Shared immutable descriptor backing is outside this
// adjacency charge; complete Runtime metadata accounting remains its own owner.
unsafe impl GcObject for OperationGroupObject {
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
