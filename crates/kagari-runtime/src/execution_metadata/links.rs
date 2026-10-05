//! Checked publication of lazy executable edges. Preparation does not execute script.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        applications::ApplicationId,
        groups::{OperationGroupId, OperationId},
        interfaces::InterfaceSnapshotId,
    },
};
use std::cell::OnceCell;

/// Construction and reads are available to object policy; writes stay in this module.
#[derive(Debug, Clone)]
pub(crate) struct MetadataCache<T>(OnceCell<T>);

impl<T> MetadataCache<T> {
    pub(crate) fn new() -> Self {
        Self(OnceCell::new())
    }

    pub(crate) fn get(&self) -> Option<&T> {
        self.0.get()
    }

    #[cfg(test)]
    pub(crate) fn corrupt_for_test(&self, value: T) {
        assert!(self.0.set(value).is_ok());
    }
}

#[derive(Clone, Copy)]
pub(crate) enum MethodSelection {
    Interface {
        snapshot: InterfaceSnapshotId,
        slot: usize,
    },
    Operation(OperationId),
}

impl Runtime {
    pub(crate) fn cache_parent_interface(
        &self,
        owner: InterfaceSnapshotId,
        slot: usize,
        prepared: InterfaceSnapshotId,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_metadata(MetadataEdge::Interface(owner))?;
        self.validate_metadata(MetadataEdge::Interface(prepared))?;
        let invalid = || RuntimeError::module_validation("invalid parent interface cache");
        let view = self.gc.interface_metadata(owner).ok_or_else(invalid)?;
        let parent = view.parents.get(slot).ok_or_else(invalid)?;
        parent.prepared.0.set(prepared).map_err(|_| invalid())
    }

    pub(crate) fn cache_method_application(
        &self,
        owner: MethodSelection,
        prepared: ApplicationId,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_metadata(MetadataEdge::Application(prepared))?;
        let invalid = || RuntimeError::module_validation("invalid method application cache");
        match owner {
            MethodSelection::Interface { snapshot, slot } => {
                self.validate_metadata(MetadataEdge::Interface(snapshot))?;
                let view = self.gc.interface_metadata(snapshot).ok_or_else(invalid)?;
                let method = view
                    .methods
                    .get(slot)
                    .and_then(Option::as_ref)
                    .ok_or_else(invalid)?;
                method.application.0.set(prepared).map_err(|_| invalid())
            }
            MethodSelection::Operation(id) => {
                self.validate_metadata(MetadataEdge::Operation(id))?;
                let operation = self.gc.bound_operation(id).ok_or_else(invalid)?;
                operation.application.0.set(prepared).map_err(|_| invalid())
            }
        }
    }

    pub(crate) fn cache_receiver_operations(
        &self,
        owner: InterfaceSnapshotId,
        slot: usize,
        prepared: Option<OperationGroupId>,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_metadata(MetadataEdge::Interface(owner))?;
        if let Some(group) = prepared {
            self.validate_metadata(MetadataEdge::Group(group))?;
        }
        let invalid = || RuntimeError::module_validation("invalid receiver operation cache");
        let view = self.gc.interface_metadata(owner).ok_or_else(invalid)?;
        let method = view
            .methods
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or_else(invalid)?;
        if method.receiver_operations.get().is_some()
            || prepared.is_some_and(|group| {
                view.receiver_operations
                    .get()
                    .is_some_and(|old| *old != group)
            })
        {
            return Err(invalid());
        }
        // Validate both cells before either write; no callback or safepoint can
        // intervene. None caches successful preparation with no required group.
        if view.receiver_operations.get().is_none()
            && let Some(group) = prepared
        {
            view.receiver_operations
                .0
                .set(group)
                .expect("checked empty receiver cache");
        }
        method
            .receiver_operations
            .0
            .set(prepared)
            .expect("checked empty method cache");
        Ok(())
    }
}
