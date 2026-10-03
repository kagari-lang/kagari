//! Rooted selections borrow immutable interface or constraint metadata.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    frame::types::{
        BoundOperation, TypeEnvironment, arguments::ScopedSignature, operations::ReceiverOperations,
    },
    gc::{
        RootedValue,
        interfaces::{InterfaceResultBinding, InterfaceValueSnapshot, MethodApplication},
    },
    module::LoadedModule,
    value::Value,
};
use kagari_abi::types::{AbiType, GenericParameterAbi, NominalAbiType};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use std::{cell::OnceCell, rc::Rc};

pub(crate) enum MethodSelection {
    Interface {
        snapshot: Rc<InterfaceValueSnapshot>,
        slot: usize,
    },
    Operation(Rc<BoundOperation>),
}

pub(crate) struct BoundReceiver {
    receiver: Value,
    concrete_type: AbiType<DefinitionId>,
    interface: NominalAbiType<DefinitionId>,
}

impl RootedInterfaceMethod {
    pub(super) fn from_interface(
        root: RootedValue,
        snapshot: Rc<InterfaceValueSnapshot>,
        slot: usize,
    ) -> Self {
        Self::selected(root, MethodSelection::Interface { snapshot, slot })
    }

    pub(super) fn from_operation(
        root: RootedValue,
        operation: Rc<BoundOperation>,
        receiver: Value,
        concrete_type: AbiType<DefinitionId>,
        interface: NominalAbiType<DefinitionId>,
    ) -> Self {
        let mut method = Self::selected(root, MethodSelection::Operation(operation));
        method.bound_receiver = Some(BoundReceiver {
            receiver,
            concrete_type,
            interface,
        });
        method
    }

    fn selected(root: RootedValue, selection: MethodSelection) -> Self {
        Self {
            selection,
            bound_receiver: None,
            environment: None,
            application: None,
            _root: root,
        }
    }

    fn bound_receiver(&self) -> &BoundReceiver {
        self.bound_receiver
            .as_ref()
            .expect("bound operation receiver")
    }

    pub fn receiver(&self) -> &Value {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => &snapshot.data,
            MethodSelection::Operation(_) => &self.bound_receiver().receiver,
        }
    }

    pub fn concrete_type(&self) -> &AbiType<DefinitionId> {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => &snapshot.concrete_type,
            MethodSelection::Operation(_) => &self.bound_receiver().concrete_type,
        }
    }

    pub fn interface_type(&self) -> &NominalAbiType<DefinitionId> {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => &snapshot.interface_type,
            MethodSelection::Operation(_) => &self.bound_receiver().interface,
        }
    }

    pub(crate) fn interface_expression(&self) -> &NominalAbiType<DefinitionId> {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => &snapshot.interface_expression,
            MethodSelection::Operation(_) => &self.bound_receiver().interface,
        }
    }

    pub fn implementation(&self) -> &LoadedModule {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => &snapshot.implementation,
            MethodSelection::Operation(operation) => &operation.owner,
        }
    }

    pub fn target(&self) -> CallableTarget {
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .target
            }
            MethodSelection::Operation(operation) => operation.target,
        }
    }

    pub fn parameter_types(&self) -> &[AbiType<DefinitionId>] {
        if let Some(application) = &self.application {
            return &application.signature.params;
        }
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .parameter_types
            }
            MethodSelection::Operation(operation) => &operation.signature.params,
        }
    }

    pub fn return_type(&self) -> &AbiType<DefinitionId> {
        if let Some(application) = &self.application {
            return &application.signature.result;
        }
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .return_type
            }
            MethodSelection::Operation(operation) => &operation.signature.result,
        }
    }

    pub(crate) fn type_parameters(&self) -> &[GenericParameterAbi<DefinitionId>] {
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .parameters
            }
            MethodSelection::Operation(operation) => operation
                .generic
                .as_ref()
                .map(|generic| generic.parameters.as_slice())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn entry_parameters(&self) -> &[GenericParameterAbi<DefinitionId>] {
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .entry_parameters
            }
            MethodSelection::Operation(operation) => operation
                .generic
                .as_ref()
                .map(|generic| generic.entry_parameters.as_slice())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn entry_arguments(&self) -> &[AbiType<DefinitionId>] {
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .entry_arguments
            }
            MethodSelection::Operation(operation) => operation
                .generic
                .as_ref()
                .map(|generic| generic.entry_arguments.as_slice())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn receiver_environment(&self) -> Option<&Rc<TypeEnvironment>> {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => snapshot.environment.as_ref(),
            MethodSelection::Operation(operation) => operation
                .generic
                .as_ref()
                .and_then(|generic| generic.receiver_environment.as_ref()),
        }
    }

    pub(crate) fn receiver_table(&self) -> Option<&InterfaceResultBinding> {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => Some(&snapshot.receiver_table),
            MethodSelection::Operation(operation) => operation
                .generic
                .as_ref()
                .map(|generic| &generic.receiver_table),
        }
    }

    pub(crate) fn result_adapter(&self) -> Option<&InterfaceResultBinding> {
        if let Some(application) = &self.application {
            return application.result_adapter.as_ref();
        }
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => snapshot.methods[*slot]
                .as_ref()
                .expect("checked interface slot")
                .result_adapter
                .as_ref(),
            MethodSelection::Operation(_) => None,
        }
    }

    pub(crate) fn scoped_signature(&self) -> Option<&ScopedSignature> {
        self.application
            .as_ref()
            .and_then(|application| application.scoped_signature.as_ref())
    }

    pub(super) fn application_cell(&self) -> &OnceCell<Rc<MethodApplication>> {
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .application
            }
            MethodSelection::Operation(operation) => &operation.application,
        }
    }

    pub(super) fn receiver_operations(
        &self,
        runtime: &Runtime,
    ) -> Result<Option<Rc<ReceiverOperations>>, RuntimeError> {
        let Some(table) = self.receiver_table() else {
            return Ok(None);
        };
        match &self.selection {
            MethodSelection::Interface { snapshot, slot } => {
                let cell = &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .receiver_operations;
                if let Some(prepared) = cell.get() {
                    return Ok(prepared.clone());
                }
                let prepared = runtime.bind_receiver_operations(
                    self.implementation(),
                    self.target(),
                    table,
                    snapshot.receiver_operations.get().cloned(),
                )?;
                if snapshot.receiver_operations.get().is_none()
                    && let Some(group) = &prepared
                {
                    snapshot
                        .receiver_operations
                        .set(group.clone())
                        .expect("receiver table prepared once");
                }
                // Only successful preparation is cached. No caller witnesses are stored here.
                cell.set(prepared.clone())
                    .expect("receiver operations prepared once");
                Ok(prepared)
            }
            MethodSelection::Operation(operation) => runtime.bind_receiver_operations(
                self.implementation(),
                self.target(),
                table,
                operation.receiver_operations.upgrade(),
            ),
        }
    }

    pub(super) fn is_operation(&self) -> bool {
        matches!(self.selection, MethodSelection::Operation(_))
    }
}
