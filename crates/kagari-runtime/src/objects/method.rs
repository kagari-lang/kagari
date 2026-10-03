//! Rooted selections borrow immutable interface or constraint metadata.
use crate::{
    RootedInterfaceMethod,
    frame::types::{BoundOperation, TypeEnvironment},
    gc::{
        RootedValue,
        interfaces::{InterfaceResultBinding, InterfaceValueSnapshot},
    },
    module::LoadedModule,
    value::Value,
};
use kagari_abi::types::{AbiType, GenericParameterAbi, NominalAbiType};
use kagari_bytecode::module::CallableTarget;
use std::rc::Rc;

pub(crate) enum MethodSelection {
    Interface {
        snapshot: Rc<InterfaceValueSnapshot>,
        slot: usize,
    },
    Operation(Rc<BoundOperation>),
}

pub(crate) struct BoundReceiver {
    receiver: Value,
    concrete_type: AbiType,
    interface: NominalAbiType,
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
        concrete_type: AbiType,
        interface: NominalAbiType,
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
            resolved_signature: None,
            scoped_signature: None,
            result_adapter: None,
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

    pub fn concrete_type(&self) -> &AbiType {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => &snapshot.concrete_type,
            MethodSelection::Operation(_) => &self.bound_receiver().concrete_type,
        }
    }

    pub fn interface_type(&self) -> &NominalAbiType {
        match &self.selection {
            MethodSelection::Interface { snapshot, .. } => &snapshot.interface_type,
            MethodSelection::Operation(_) => &self.bound_receiver().interface,
        }
    }

    pub(crate) fn interface_expression(&self) -> &NominalAbiType {
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

    pub fn parameter_types(&self) -> &[AbiType] {
        if let Some(signature) = &self.resolved_signature {
            return &signature.params;
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

    pub fn return_type(&self) -> &AbiType {
        if let Some(signature) = &self.resolved_signature {
            return &signature.result;
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

    pub(crate) fn type_parameters(&self) -> &[GenericParameterAbi] {
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

    pub(crate) fn entry_parameters(&self) -> &[GenericParameterAbi] {
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

    pub(crate) fn entry_arguments(&self) -> &[AbiType] {
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
        if self.result_adapter.is_some() {
            return self.result_adapter.as_ref();
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
}
