//! Borrowed method metadata cannot retain executable storage or cross an entry.
use crate::{
    execution_metadata::{
        applications::{ApplicationId, MethodApplication},
        operation::BoundOperation,
    },
    frame::types::{TypeEnvironment, arguments::ScopedSignature},
    gc::interfaces::{InterfaceResultBinding, InterfaceValueSnapshot},
    module::LoadedModule,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{GenericParam, Ty};
use std::cell::Ref;

pub(super) enum SelectionView<'a> {
    Interface {
        snapshot: Ref<'a, InterfaceValueSnapshot>,
        slot: usize,
    },
    Operation(Ref<'a, BoundOperation>),
}

pub(crate) struct MethodView<'a> {
    pub(super) selection: SelectionView<'a>,
    pub(super) application: Option<Ref<'a, MethodApplication>>,
}

impl MethodView<'_> {
    pub(crate) fn implementation(&self) -> &LoadedModule {
        match &self.selection {
            SelectionView::Interface { snapshot, .. } => &snapshot.implementation,
            SelectionView::Operation(operation) => &operation.owner,
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
            SelectionView::Operation(operation) => operation.target,
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
            SelectionView::Operation(operation) => &operation.signature.params,
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
            SelectionView::Operation(operation) => &operation.signature.result,
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
            SelectionView::Operation(operation) => operation
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
            SelectionView::Operation(operation) => operation
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
            SelectionView::Operation(operation) => operation
                .generic
                .as_ref()
                .map(|generic| generic.entry_arguments.as_slice())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn receiver_environment(&self) -> Option<&TypeEnvironment> {
        match &self.selection {
            SelectionView::Interface { snapshot, .. } => snapshot.environment.as_ref(),
            SelectionView::Operation(operation) => operation
                .generic
                .as_ref()
                .and_then(|generic| generic.receiver_environment.as_ref()),
        }
    }

    pub(crate) fn receiver_table(&self) -> Option<&InterfaceResultBinding> {
        match &self.selection {
            SelectionView::Interface { snapshot, .. } => Some(&snapshot.receiver_table),
            SelectionView::Operation(operation) => operation
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
            SelectionView::Operation(_) => None,
        }
    }

    pub(crate) fn scoped_signature(&self) -> Option<&ScopedSignature> {
        self.application
            .as_ref()
            .and_then(|application| application.scoped_signature.as_ref())
    }

    pub(super) fn cached_application(&self) -> Option<ApplicationId> {
        match &self.selection {
            SelectionView::Interface { snapshot, slot } => snapshot.methods[*slot]
                .as_ref()
                .expect("checked interface slot")
                .application
                .get()
                .copied(),
            SelectionView::Operation(operation) => operation.application.get().copied(),
        }
    }
}
