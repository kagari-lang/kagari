//! Immutable method facts escape closed reads; actual sources authorize each use.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::ExecutionRoots,
    execution_metadata::{
        MetadataEdge,
        application_key::{AdapterIdentity, InterfaceMethodIdentity, MethodIdentity},
        applications::{AdmittedApplication, ApplicationRecord, MethodApplication},
        groups::OperationId,
        groups::group::OperationGroup,
        operation::BoundOperation,
    },
    frame::types::compatibility::TypeView,
    frame::{
        arguments::FrameArguments,
        types::{TypeEnvironment, arguments::ScopedSignature},
    },
    module::{ExecutableModule, dispatch::SelectedTarget},
    objects::interfaces::description::{InterfaceDescription, InterfaceResultBinding},
    value::view::ValueView,
    value_check::scoped::{self, CheckError},
};

use kagari_bytecode::module::CallableTarget;
use kagari_types::ty::{GenericParam, Ty};
use std::{ops::Deref, sync::Arc};

#[derive(Debug, Clone)]
pub(super) enum SelectionView {
    Interface {
        description: Arc<InterfaceDescription>,
        slot: usize,
    },
    Operation {
        id: OperationId,
        operation: SelectedOperation,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct MethodFacts {
    pub(super) selection: SelectionView,
    pub(super) application: Option<AdmittedApplication>,
}

#[derive(Debug, Clone)]
pub(crate) struct MethodView {
    pub(super) facts: MethodFacts,
    pub(super) selected: SelectedTarget,
    pub(super) selected_environment: Option<TypeEnvironment>,
}

impl Deref for MethodView {
    type Target = MethodFacts;

    fn deref(&self) -> &Self::Target {
        &self.facts
    }
}

impl MethodView {
    pub(crate) fn environment(&self) -> Option<&TypeEnvironment> {
        self.selected_environment.as_ref()
    }

    pub(crate) fn implementation(&self) -> &ExecutableModule {
        self.selected.owner()
    }

    pub(crate) fn target(&self) -> CallableTarget {
        self.selected.target()
    }
}

#[derive(Debug, Clone)]
pub(super) struct SelectedOperation {
    pub(super) id: OperationId,
    pub(super) group: Arc<OperationGroup>,
}

impl Deref for SelectedOperation {
    type Target = BoundOperation;

    fn deref(&self) -> &Self::Target {
        self.group.get(self.id).expect("checked operation member")
    }
}

impl MethodFacts {
    /// An admitted method owns the supplying relationship and applied signature,
    /// independently of the caller's receiver/Host/preparation source lifetime.
    pub(crate) fn trace<'a, E>(
        &'a self,
        visit: &mut (impl FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized),
    ) -> Result<(), E> {
        match &self.selection {
            SelectionView::Interface { description, .. } => {
                visit(MetadataEdge::InterfaceDescription(description))?
            }
            SelectionView::Operation { id, .. } => visit(MetadataEdge::Operation(*id))?,
        }
        if let Some(application) = &self.application {
            visit(MetadataEdge::Application(application.id()))?;
        }
        Ok(())
    }

    pub(crate) fn interface_receiver(&self) -> bool {
        match self.selection {
            SelectionView::Interface { .. } => true,
            SelectionView::Operation { .. } => false,
        }
    }

    fn application(&self) -> Option<&MethodApplication> {
        match self
            .application
            .as_ref()
            .map(|application| application.record().as_ref())
        {
            Some(ApplicationRecord::Semantic(application)) => Some(application),
            _ => None,
        }
    }

    pub(crate) fn validate_arguments(
        &self,
        runtime: &mut Runtime,
        arguments: FrameArguments<'_, '_>,
    ) -> Result<(), RuntimeError> {
        runtime.resources.poll_execution()?;
        if !self.proof_implementation().belongs_to(runtime.host.owner())
            || arguments.len() != self.parameter_types().len()
        {
            return Err(TrapReason::InterfaceMethodArgumentDoesNotMatchItsLinkedSignature.into());
        }
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<bool, CheckError> {
                    arguments.check_bounds(slots)?;
                    for index in 0..arguments.len() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        let value = arguments.get(slots, index)?;
                        let expected = match self.scoped_signature() {
                            Some(signature) => {
                                signature.params[index].view(self.proof_implementation())
                            }
                            None => TypeView::new(
                                &self.parameter_types()[index],
                                self.proof_implementation(),
                                None,
                            ),
                        };
                        if !scoped::matches(
                            slots.scope(),
                            value,
                            expected,
                            host,
                            host_borrows,
                            resources,
                        )? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                })())
            })
        });
        let valid = result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| {
                runtime
                    .resources
                    .admit_execution_error(error.into_runtime(runtime))
            })?;
        if !valid {
            return Err(TrapReason::InterfaceMethodArgumentDoesNotMatchItsLinkedSignature.into());
        }
        Ok(())
    }

    pub(crate) fn validate_result(
        &self,
        runtime: &mut Runtime,
        result: &ValueView<'_>,
    ) -> Result<(), RuntimeError> {
        runtime.resources.poll_execution()?;
        if !self.proof_implementation().belongs_to(runtime.host.owner()) {
            return Err(TrapReason::InterfaceMethodResultDoesNotMatchItsLinkedSignature.into());
        }
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = runtime;
        let checked = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<bool, CheckError> {
                    let result = result.get(slots)?;
                    let expected = match self.scoped_signature() {
                        Some(signature) => signature.result.view(self.proof_implementation()),
                        None => {
                            TypeView::new(self.return_type(), self.proof_implementation(), None)
                        }
                    };
                    scoped::matches(
                        slots.scope(),
                        result,
                        expected,
                        host,
                        host_borrows,
                        resources,
                    )
                })())
            })
        });
        let valid = checked
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| {
                runtime
                    .resources
                    .admit_execution_error(error.into_runtime(runtime))
            })?;
        if !valid {
            return Err(TrapReason::InterfaceMethodResultDoesNotMatchItsLinkedSignature.into());
        }
        Ok(())
    }

    /// Relationship and value evidence stays in its supplying publication.
    pub(super) fn proof_implementation(&self) -> &ExecutableModule {
        match &self.selection {
            SelectionView::Interface { description, .. } => &description.implementation,
            SelectionView::Operation { operation, .. } => &operation.owner,
        }
    }

    pub(super) fn proof_target(&self) -> CallableTarget {
        match &self.selection {
            SelectionView::Interface { description, slot } => {
                description.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .target
            }
            SelectionView::Operation { operation, .. } => operation.target,
        }
    }

    pub(crate) fn parameter_types(&self) -> &[Ty] {
        if let Some(application) = self.application() {
            return &application.signature.params;
        }
        match &self.selection {
            SelectionView::Interface { description, slot } => {
                &description.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .parameter_types
            }
            SelectionView::Operation { operation, .. } => &operation.signature.params,
        }
    }

    pub(crate) fn return_type(&self) -> &Ty {
        if let Some(application) = self.application() {
            return &application.signature.result;
        }
        match &self.selection {
            SelectionView::Interface { description, slot } => {
                &description.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .return_type
            }
            SelectionView::Operation { operation, .. } => &operation.signature.result,
        }
    }

    pub(crate) fn type_parameters(&self) -> &[GenericParam] {
        match &self.selection {
            SelectionView::Interface { description, slot } => {
                &description.methods[*slot]
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

    pub(crate) fn entry_parameters(&self) -> &[GenericParam] {
        match &self.selection {
            SelectionView::Interface { description, slot } => {
                &description.methods[*slot]
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

    pub(crate) fn entry_arguments(&self) -> &[Ty] {
        match &self.selection {
            SelectionView::Interface { description, slot } => {
                &description.methods[*slot]
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
            SelectionView::Interface { description, .. } => description.environment.as_ref(),
            SelectionView::Operation { operation, .. } => operation
                .generic
                .as_ref()
                .and_then(|generic| generic.receiver_environment.as_ref()),
        }
    }

    pub(crate) fn receiver_table(&self) -> Option<&InterfaceResultBinding> {
        match &self.selection {
            SelectionView::Interface { description, .. } => Some(&description.receiver_table),
            SelectionView::Operation { operation, .. } => operation
                .generic
                .as_ref()
                .map(|generic| &generic.receiver_table),
        }
    }

    pub(crate) fn result_adapter(&self) -> Option<&InterfaceResultBinding> {
        if let Some(application) = self.application() {
            return application.result_adapter.as_ref();
        }
        match &self.selection {
            SelectionView::Interface { description, slot } => description.methods[*slot]
                .as_ref()
                .expect("checked interface slot")
                .result_adapter
                .as_ref(),
            SelectionView::Operation { .. } => None,
        }
    }

    pub(crate) fn scoped_signature(&self) -> Option<&ScopedSignature> {
        self.application()
            .and_then(|application| application.scoped_signature.as_ref())
    }

    pub(super) fn identity(&self) -> MethodIdentity {
        match &self.selection {
            SelectionView::Interface { description, slot } => description.methods[*slot]
                .as_ref()
                .expect("checked interface slot")
                .identity
                .get_or_init(|| {
                    let binding = &description.receiver_table;
                    MethodIdentity::Interface(Arc::new(InterfaceMethodIdentity {
                        owner: binding.owner.key(),
                        table: binding.table,
                        slot: *slot,
                        interface: description.interface_expression.clone(),
                        arguments: binding.arguments.clone(),
                        environment: binding
                            .environment
                            .as_ref()
                            .map(|environment| environment.id),
                        result_adapter: description.methods[*slot]
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
