//! A reified environment owns the actual parent/operation/construction graph.
use crate::{
    execution_metadata::{
        environments::EnvironmentId,
        managed::{Dependencies, ProtectedMetadata, sources::MetadataError},
    },
    frame::types::EnvironmentRecord,
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
pub(crate) struct EnvironmentObject {
    id: EnvironmentId,
    record: Arc<EnvironmentRecord>,
    pub(super) dependencies: Dependencies,
}

impl EnvironmentObject {
    /// IDs and immutable type facts are not roots. The semantic preparation owner
    /// supplies actual protected nodes for every direct executable relationship.
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        id: EnvironmentId,
        record: Arc<EnvironmentRecord>,
        protected: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<Gc<'gc, Self>, MetadataError> {
        resources.poll_closed()?;
        let pending = Dependencies::collect(resources, |visit| record.trace_metadata(visit))?;
        let dependencies = Dependencies::resolve(cx, pending, protected, resources)?;
        // SAFETY: the new object has an empty, completely initialized adjacency.
        // The bounded publication below keeps every intermediate prefix scanned.
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

    pub(crate) fn id(&self) -> EnvironmentId {
        self.id
    }

    pub(crate) fn record(&self) -> &Arc<EnvironmentRecord> {
        &self.record
    }
}

// SAFETY: immutable direct edges cover the parent, operation groups and supplying
// construction revisions. Scanning uses stable numeric positions, visits every
// edge, and Drop neither reads peers nor reenters Runtime. Shared immutable fact
// backing is outside managed ownership; adjacency capacity is charged here.
unsafe impl GcObject for EnvironmentObject {
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
