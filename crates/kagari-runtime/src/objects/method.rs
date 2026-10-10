//! Rooted selections borrow immutable interface or constraint metadata.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{
        MetadataRoot,
        groups::{OperationGroupId, OperationId},
        interfaces::InterfaceSnapshotId,
        links::MethodSelection,
    },
    gc::roots::RootedValue,
    module::LoadedModule,
    objects::method_view::{MethodView, SelectionView},
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{NominalTy, Ty};

#[derive(Clone)]
pub(crate) struct BoundReceiver {
    receiver: Value,
    concrete_type: Ty<DefinitionId>,
    interface: NominalTy<DefinitionId>,
    interface_expression: NominalTy<DefinitionId>,
}

impl RootedInterfaceMethod {
    pub(super) fn refresh_roots(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        let mut metadata = vec![match &self.selection {
            MethodSelection::Interface { snapshot, .. } => MetadataRoot::Interface(*snapshot),
            MethodSelection::Operation(id) => MetadataRoot::Operation(*id),
        }];
        metadata.extend(
            self.environment
                .iter()
                .map(|environment| MetadataRoot::Environment(environment.id)),
        );
        metadata.extend(
            self.application
                .iter()
                .copied()
                .map(MetadataRoot::Application),
        );
        self._root.set_metadata(runtime, metadata)
    }

    pub(super) fn from_interface(
        runtime: &Runtime,
        root: RootedValue,
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
        Ok(Self::selected(
            root,
            MethodSelection::Interface { snapshot, slot },
            BoundReceiver {
                receiver: view.data,
                concrete_type: view.concrete_type.clone(),
                interface: view.interface_type.clone(),
                interface_expression: view.interface_expression.clone(),
            },
        ))
    }

    pub(super) fn from_operation(
        runtime: &Runtime,
        root: RootedValue,
        id: OperationId,
        receiver: Value,
        concrete_type: Ty<DefinitionId>,
        interface: NominalTy<DefinitionId>,
    ) -> Result<Self, RuntimeError> {
        let operation = runtime
            .gc
            .bound_operation(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid selected method operation"))?;
        runtime.validate_loaded_module(&operation.owner)?;
        drop(operation);
        Ok(Self::selected(
            root,
            MethodSelection::Operation(id),
            BoundReceiver {
                receiver,
                concrete_type,
                interface_expression: interface.clone(),
                interface,
            },
        ))
    }

    fn selected(
        root: RootedValue,
        selection: MethodSelection,
        bound_receiver: BoundReceiver,
    ) -> Self {
        Self {
            selection,
            bound_receiver,
            environment: None,
            application: None,
            _root: root,
        }
    }

    pub fn receiver(&self) -> &Value {
        &self.bound_receiver.receiver
    }

    pub fn concrete_type(&self) -> &Ty<DefinitionId> {
        &self.bound_receiver.concrete_type
    }

    pub fn interface_type(&self) -> &NominalTy<DefinitionId> {
        &self.bound_receiver.interface
    }

    pub(crate) fn interface_expression(&self) -> &NominalTy<DefinitionId> {
        &self.bound_receiver.interface_expression
    }

    pub(crate) fn view<'a>(&'a self, runtime: &'a Runtime) -> Result<MethodView<'a>, RuntimeError> {
        if !self._root.is_valid(&runtime.gc) {
            return Err(RuntimeError::module_validation(
                "method root belongs to another runtime or was released",
            ));
        }
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

    pub fn implementation(&self, runtime: &Runtime) -> Result<LoadedModule, RuntimeError> {
        Ok(self.view(runtime)?.implementation().clone())
    }

    pub fn target(&self, runtime: &Runtime) -> Result<CallableTarget, RuntimeError> {
        Ok(self.view(runtime)?.target())
    }

    pub fn parameter_types(
        &self,
        runtime: &Runtime,
    ) -> Result<Vec<Ty<DefinitionId>>, RuntimeError> {
        Ok(self.view(runtime)?.parameter_types().to_vec())
    }

    pub fn return_type(&self, runtime: &Runtime) -> Result<Ty<DefinitionId>, RuntimeError> {
        Ok(self.view(runtime)?.return_type().clone())
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
