//! Active method selections are traced by execution windows, not host root leases.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{
        MetadataRoot,
        applications::ApplicationId,
        groups::{OperationGroupId, OperationId},
        interfaces::InterfaceSnapshotId,
        links::MethodSelection,
    },
    objects::method_view::{MethodView, SelectionView},
};

/// A checked selection/application identity. It carries no external root lease;
/// its owner must publish these edges before crossing a collection boundary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MethodInvocation {
    selection: MethodSelection,
    pub(super) application: Option<ApplicationId>,
}

impl MethodInvocation {
    pub(super) fn from_interface(
        runtime: &Runtime,
        snapshot: InterfaceSnapshotId,
        slot: usize,
    ) -> Result<Self, RuntimeError> {
        let view = runtime
            .gc
            .interface_metadata(snapshot)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface snapshot"))?;
        if view.methods.get(slot).and_then(Option::as_ref).is_none() {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method unavailable",
            ));
        }
        runtime.validate_loaded_module(&view.implementation)?;
        Ok(Self {
            selection: MethodSelection::Interface { snapshot, slot },
            application: None,
        })
    }

    pub(super) fn from_operation(runtime: &Runtime, id: OperationId) -> Result<Self, RuntimeError> {
        let operation = runtime
            .gc
            .bound_operation(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid selected method operation"))?;
        runtime.validate_loaded_module(&operation.owner)?;
        Ok(Self {
            selection: MethodSelection::Operation(id),
            application: None,
        })
    }

    pub(crate) fn append_metadata(self, roots: &mut Vec<MetadataRoot>) {
        roots.push(match self.selection {
            MethodSelection::Interface { snapshot, .. } => MetadataRoot::Interface(snapshot),
            MethodSelection::Operation(id) => MetadataRoot::Operation(id),
        });
        roots.extend(self.application.map(MetadataRoot::Application));
    }

    pub(crate) fn view<'a>(&self, runtime: &'a Runtime) -> Result<MethodView<'a>, RuntimeError> {
        let selection = match &self.selection {
            MethodSelection::Interface { snapshot, slot } => SelectionView::Interface {
                snapshot: runtime.gc.interface_metadata(*snapshot).ok_or_else(|| {
                    RuntimeError::module_validation("invalid selected interface snapshot")
                })?,
                slot: *slot,
            },
            MethodSelection::Operation(id) => SelectionView::Operation {
                id: *id,
                operation: runtime.gc.bound_operation(*id).ok_or_else(|| {
                    RuntimeError::module_validation("invalid selected method operation")
                })?,
            },
        };
        Ok(MethodView {
            selection,
            application: self
                .application
                .map(|id| {
                    runtime.gc.method_application(id).ok_or_else(|| {
                        RuntimeError::module_validation("invalid method application")
                    })
                })
                .transpose()?,
        })
    }

    pub(super) fn receiver_operations(
        &self,
        runtime: &Runtime,
    ) -> Result<Option<OperationGroupId>, RuntimeError> {
        let view = self.view(runtime)?;
        let Some(table) = view.receiver_table() else {
            return Ok(None);
        };
        let group = match self.selection {
            MethodSelection::Operation(id) => Some(id.group),
            MethodSelection::Interface { .. } => None,
        };
        match &view.selection {
            SelectionView::Interface { snapshot, slot } => {
                let cell = &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .receiver_operations;
                if let Some(prepared) = cell.get() {
                    return Ok(*prepared);
                }
                let prepared = runtime.bind_receiver_operations(
                    view.implementation(),
                    view.target(),
                    table,
                    snapshot.receiver_operations.get().copied(),
                )?;
                let MethodSelection::Interface { snapshot: id, slot } = self.selection else {
                    unreachable!("interface selection view");
                };
                runtime.cache_receiver_operations(id, slot, prepared)?;
                Ok(prepared)
            }
            SelectionView::Operation { .. } => {
                runtime.bind_receiver_operations(view.implementation(), view.target(), table, group)
            }
        }
    }
}
