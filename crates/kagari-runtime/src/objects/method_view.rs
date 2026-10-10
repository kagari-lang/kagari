//! Borrowed method metadata cannot retain executable storage or cross an entry.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{
        application_key::{AdapterIdentity, InterfaceMethodIdentity, MethodIdentity},
        applications::MethodApplication,
        groups::OperationId,
        operation::BoundOperation,
    },
    frame::{
        arguments::FrameArguments,
        types::{TypeEnvironment, arguments::ScopedSignature},
    },
    gc::interfaces::{InterfaceResultBinding, InterfaceValueSnapshot},
    module::LoadedModule,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{GenericParam, Ty};
use std::{cell::Ref, sync::Arc};

pub(super) enum SelectionView<'a> {
    Interface {
        snapshot: Ref<'a, InterfaceValueSnapshot>,
        slot: usize,
    },
    Operation {
        id: OperationId,
        operation: Ref<'a, BoundOperation>,
    },
}

pub(crate) struct MethodView<'a> {
    pub(super) selection: SelectionView<'a>,
    pub(super) application: Option<Ref<'a, MethodApplication>>,
}

impl MethodView<'_> {
    pub(crate) fn validate_arguments(
        &self,
        runtime: &Runtime,
        arguments: FrameArguments<'_>,
    ) -> Result<(), RuntimeError> {
        if !self.implementation().belongs_to(runtime.host.owner())
            || arguments.len() != self.parameter_types().len()
            || !arguments.all(runtime, |index, value| match self.scoped_signature() {
                Some(signature) => {
                    signature.params[index].matches(runtime, value, self.implementation())
                }
                None => runtime.matches_interface_method_abi(
                    value,
                    &self.parameter_types()[index],
                    self.implementation(),
                ),
            })?
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method argument does not match its linked signature",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_result(
        &self,
        runtime: &Runtime,
        result: &Value,
    ) -> Result<(), RuntimeError> {
        if !self.implementation().belongs_to(runtime.host.owner())
            || !match self.scoped_signature() {
                Some(signature) => signature
                    .result
                    .matches(runtime, result, self.implementation()),
                None => runtime.matches_interface_method_abi(
                    result,
                    self.return_type(),
                    self.implementation(),
                ),
            }
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method result does not match its linked signature",
            ));
        }
        Ok(())
    }

    pub(crate) fn environment(&self) -> Option<&TypeEnvironment> {
        self.application
            .as_ref()
            .and_then(|application| application.environment.as_ref())
    }

    pub(crate) fn implementation(&self) -> &LoadedModule {
        match &self.selection {
            SelectionView::Interface { snapshot, .. } => &snapshot.implementation,
            SelectionView::Operation { operation, .. } => &operation.owner,
        }
    }

    pub(crate) fn target(&self) -> CallableTarget {
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => {
                snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .target
            }
            SelectionView::Operation { operation, .. } => operation.target,
        }
    }

    pub(crate) fn parameter_types(&self) -> &[Ty<DefinitionId>] {
        if let Some(application) = &self.application {
            return &application.signature.params;
        }
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .parameter_types
            }
            SelectionView::Operation { operation, .. } => &operation.signature.params,
        }
    }

    pub(crate) fn return_type(&self) -> &Ty<DefinitionId> {
        if let Some(application) = &self.application {
            return &application.signature.result;
        }
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .return_type
            }
            SelectionView::Operation { operation, .. } => &operation.signature.result,
        }
    }

    pub(crate) fn type_parameters(&self) -> &[GenericParam<DefinitionId>] {
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .parameters
            }
            SelectionView::Operation { operation, .. } => operation
                .generic
                .as_ref()
                .map(|generic| generic.parameters.as_slice())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn entry_parameters(&self) -> &[GenericParam<DefinitionId>] {
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .entry_parameters
            }
            SelectionView::Operation { operation, .. } => operation
                .generic
                .as_ref()
                .map(|generic| generic.entry_parameters.as_slice())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn entry_arguments(&self) -> &[Ty<DefinitionId>] {
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => {
                &snapshot.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .entry_arguments
            }
            SelectionView::Operation { operation, .. } => operation
                .generic
                .as_ref()
                .map(|generic| generic.entry_arguments.as_slice())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn receiver_environment(&self) -> Option<&TypeEnvironment> {
        match &self.selection {
            SelectionView::Interface { snapshot, .. } => snapshot.environment.as_ref(),
            SelectionView::Operation { operation, .. } => operation
                .generic
                .as_ref()
                .and_then(|generic| generic.receiver_environment.as_ref()),
        }
    }

    pub(crate) fn receiver_table(&self) -> Option<&InterfaceResultBinding> {
        match &self.selection {
            SelectionView::Interface { snapshot, .. } => Some(&snapshot.receiver_table),
            SelectionView::Operation { operation, .. } => operation
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
            SelectionView::Interface { snapshot, slot } => snapshot.methods[*slot]
                .as_ref()
                .expect("checked interface slot")
                .result_adapter
                .as_ref(),
            SelectionView::Operation { .. } => None,
        }
    }

    pub(crate) fn scoped_signature(&self) -> Option<&ScopedSignature> {
        self.application
            .as_ref()
            .and_then(|application| application.scoped_signature.as_ref())
    }

    pub(super) fn identity(&self) -> MethodIdentity {
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => snapshot.methods[*slot]
                .as_ref()
                .expect("checked interface slot")
                .identity
                .get_or_init(|| {
                    let binding = &snapshot.receiver_table;
                    MethodIdentity::Interface(Arc::new(InterfaceMethodIdentity {
                        owner: binding.owner.key(),
                        table: binding.table,
                        slot: *slot,
                        interface: snapshot.interface_expression.clone(),
                        arguments: binding.arguments.clone(),
                        environment: binding
                            .environment
                            .as_ref()
                            .map(|environment| environment.id),
                        result_adapter: snapshot.methods[*slot]
                            .as_ref()
                            .and_then(|method| method.result_adapter.as_ref())
                            .map(|adapter| AdapterIdentity {
                                owner: adapter.owner.key(),
                                table: adapter.table,
                                arguments: adapter.arguments.clone(),
                                environment: adapter
                                    .environment
                                    .as_ref()
                                    .map(|environment| environment.id),
                            }),
                    }))
                })
                .clone(),
            SelectionView::Operation { id, .. } => MethodIdentity::Operation(*id),
        }
    }
}
