//! Method facts name relationships; only explicitly supplied actual graphs admit use.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::SessionId,
    execution_metadata::{
        MetadataEdge,
        applications::{AdmittedApplication, ApplicationRecord, MethodComputation},
        groups::{AdmittedOperationGroup, OperationId},
        managed::{ProtectedMetadata, sources::MetadataSource},
    },
    objects::{
        interfaces::description::InterfaceDescription,
        method_view::{MethodFacts, MethodView, SelectedOperation, SelectionView},
    },
    session::ExecutionPhase,
};
use kagari_gc::error::GcError;
use std::sync::Arc;

/// Pure checked facts and nonowning preparation coordinates. Callers must supply
/// their actual frame/provider/Host graph on every use, including after cloning.
#[derive(Debug, Clone)]
pub(crate) struct MethodInvocation {
    selection: SelectionView,
    pub(crate) application: Option<AdmittedApplication>,
    dispatch: MethodDispatch,
}

#[derive(Debug, Clone)]
enum MethodDispatch {
    Live,
    Admitted(AdmittedApplication),
}

impl MethodInvocation {
    pub(crate) fn from_interface(
        runtime: &mut Runtime,
        description: Arc<InterfaceDescription>,
        slot: usize,
        protected: &[MetadataSource<'_>],
    ) -> Result<Self, RuntimeError> {
        if description
            .methods
            .get(slot)
            .and_then(Option::as_ref)
            .is_none()
        {
            return Err(TrapReason::InterfaceMethodUnavailable.into());
        }
        let method = Self {
            selection: SelectionView::Interface { description, slot },
            application: None,
            dispatch: MethodDispatch::Live,
        };
        method.authenticate(runtime, protected)?;
        Ok(method)
    }

    pub(crate) fn from_operation(
        runtime: &mut Runtime,
        execution: SessionId,
        id: OperationId,
        protected: &[MetadataSource<'_>],
    ) -> Result<Self, RuntimeError> {
        let group = runtime.with_metadata_graph(protected, |cx, graph, resources| {
            let Some(ProtectedMetadata::Operations(group)) =
                graph.find(cx, MetadataEdge::Operation(id), resources)?
            else {
                return Err(RuntimeError::module_validation(
                    "selected method operation is not protected",
                )
                .into());
            };
            Ok(cx.read(group, |group| group.record().clone()))
        })?;
        let operation = group
            .get(id)
            .ok_or_else(|| RuntimeError::module_validation("selected method operation member"))?;
        runtime.validate_executable_module(&operation.owner)?;
        let application = operation
            .application
            .map(|id| runtime.application_in(execution, id, protected))
            .transpose()?;
        Ok(Self {
            selection: SelectionView::Operation {
                id,
                operation: SelectedOperation { id, group },
            },
            application,
            dispatch: MethodDispatch::Live,
        })
    }

    pub(crate) fn interface_receiver(&self) -> bool {
        matches!(self.selection, SelectionView::Interface { .. })
    }

    pub(crate) fn is_admitted(&self) -> bool {
        matches!(self.dispatch, MethodDispatch::Admitted(_))
    }

    pub(crate) fn rebind_computation(
        &mut self,
        computation: AdmittedApplication,
    ) -> Result<(), RuntimeError> {
        let expected = self
            .computation()
            .ok_or_else(|| RuntimeError::module_validation("method is not admitted"))?;
        if expected.id() != computation.id()
            || !Arc::ptr_eq(expected.record(), computation.record())
        {
            return Err(RuntimeError::module_validation(
                "method computation facts changed during handoff",
            ));
        }
        self.dispatch = MethodDispatch::Admitted(computation);
        Ok(())
    }

    pub(crate) fn computation(&self) -> Option<&AdmittedApplication> {
        match &self.dispatch {
            MethodDispatch::Live => None,
            MethodDispatch::Admitted(application) => Some(application),
        }
    }

    /// Freeze one call, publishing its selected body and transported environment
    /// into the destination suffix. Reusable relationship handles remain live.
    pub(crate) fn admit(
        mut self,
        runtime: &mut Runtime,
        execution: SessionId,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<Self, RuntimeError> {
        let view = self.view(runtime, protected)?;
        if self.is_admitted() {
            return Ok(self);
        }
        let environment = self
            .application
            .as_ref()
            .and_then(|application| match application.record().as_ref() {
                ApplicationRecord::Semantic(application) => application.environment.as_ref(),
                _ => None,
            })
            .map(|facts| {
                let environment = runtime
                    .copy_environment_in(execution, facts, protected)?
                    .ok_or_else(|| {
                        RuntimeError::module_validation(
                            "method application environment is not protected",
                        )
                    })?;
                runtime.transport_environment(
                    execution,
                    view.proof_implementation(),
                    &view.selected,
                    &environment,
                )
            })
            .transpose()?;
        protected
            .try_reserve(2)
            .map_err(|_| RuntimeError::resource_limit("method admission sources"))?;
        if let Some(environment) = &environment {
            protected.push(MetadataSource::Environment(environment.source()));
        }
        // A newly selected ordinary body may open current publication. Retained
        // computations below use their own admitted Application graph exclusively.
        let origin = runtime.selection_origin(view.selected.owner())?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(protected.len() + 1)
            .map_err(|_| RuntimeError::resource_limit("method computation sources"))?;
        sources.extend_from_slice(protected);
        sources.push(MetadataSource::Revision {
            owner: view.selected.owner(),
            source: origin,
        });
        let computation = runtime.alloc_application(
            execution,
            ApplicationRecord::Admitted(MethodComputation {
                relationship: view.facts.clone(),
                selected: view.selected.clone(),
                environment: environment
                    .as_ref()
                    .map(|environment| environment.facts().clone()),
            }),
            &sources,
        );
        // The borrowed selected owner is local to this admission. Its actual
        // application edge becomes the durable dependency before the facts leave.
        self.dispatch = MethodDispatch::Admitted(computation?);
        Ok(self)
    }

    pub(crate) fn view(
        &self,
        runtime: &mut Runtime,
        protected: &[MetadataSource<'_>],
    ) -> Result<MethodView, RuntimeError> {
        self.authenticate(runtime, protected)?;
        let mut view = MethodView {
            facts: self.snapshot(),
            selected: match &self.dispatch {
                MethodDispatch::Live => {
                    let (owner, target) = match &self.selection {
                        SelectionView::Interface { description, slot } => (
                            &description.implementation,
                            description.methods[*slot]
                                .as_ref()
                                .expect("checked interface slot")
                                .target,
                        ),
                        SelectionView::Operation { operation, .. } => {
                            (&operation.owner, operation.target)
                        }
                    };
                    runtime.select_call_route(&owner.bind_call_route(target)?)?
                }
                MethodDispatch::Admitted(application) => match application.record().as_ref() {
                    ApplicationRecord::Admitted(computation) => computation.selected.clone(),
                    _ => {
                        return Err(RuntimeError::module_validation(
                            "invalid method computation record",
                        ));
                    }
                },
            },
            selected_environment: None,
        };
        if let MethodDispatch::Admitted(application) = &self.dispatch {
            let ApplicationRecord::Admitted(computation) = application.record().as_ref() else {
                return Err(RuntimeError::module_validation(
                    "invalid method computation record",
                ));
            };
            let owner = computation.selected.owner();
            runtime.validate_executable_module(owner)?;
            let candidate = runtime
                .resources()
                .active_session()
                .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization);
            if candidate.as_ref().is_some_and(|session| {
                session.root.publication_token() != owner.publication_token()
            }) || (candidate.is_none() && runtime.modules.is_staged(owner))
            {
                return Err(RuntimeError::execution_phase_violation(
                    "admitted method outside its publication scope",
                ));
            }
            view.selected_environment = computation.environment.clone();
        }
        Ok(view)
    }

    /// Inspect the checked relationship/application without selecting a body.
    /// Generic preparation uses this path; only admission/explicit inspection
    /// resolves an ordinary route, once for that computation.
    pub(super) fn facts(
        &self,
        runtime: &mut Runtime,
        protected: &[MetadataSource<'_>],
    ) -> Result<MethodFacts, RuntimeError> {
        self.authenticate(runtime, protected)?;
        Ok(self.snapshot())
    }

    fn snapshot(&self) -> MethodFacts {
        MethodFacts {
            selection: self.selection.clone(),
            application: self.application.clone(),
        }
    }

    fn authenticate(
        &self,
        runtime: &mut Runtime,
        protected: &[MetadataSource<'_>],
    ) -> Result<(), RuntimeError> {
        runtime.with_metadata_graph(protected, |cx, graph, resources| {
            match &self.selection {
                SelectionView::Interface { description, .. } => {
                    graph
                        .interface(cx, description, resources)?
                        .ok_or_else(|| {
                            RuntimeError::module_validation("method interface is not protected")
                        })?;
                }
                SelectionView::Operation { id, operation } => {
                    let Some(ProtectedMetadata::Operations(group)) =
                        graph.find(cx, MetadataEdge::Operation(*id), resources)?
                    else {
                        return Err(RuntimeError::module_validation(
                            "method dictionary is not protected",
                        )
                        .into());
                    };
                    if !cx.read(group, |group| Arc::ptr_eq(group.record(), &operation.group)) {
                        return Err(GcError::Representation.into());
                    }
                }
            }
            if self.application.as_ref().is_some_and(|application| {
                !matches!(
                    application.record().as_ref(),
                    ApplicationRecord::Semantic(_)
                )
            }) {
                return Err(
                    RuntimeError::module_validation("invalid semantic method application").into(),
                );
            }
            for application in self.application.iter().chain(self.computation()) {
                let Some(ProtectedMetadata::Application(value)) =
                    graph.find(cx, MetadataEdge::Application(application.id()), resources)?
                else {
                    return Err(RuntimeError::module_validation(
                        "method application is not protected",
                    )
                    .into());
                };
                if !cx.read(value, |value| {
                    Arc::ptr_eq(value.record(), application.record())
                }) {
                    return Err(GcError::Representation.into());
                }
                if let ApplicationRecord::Admitted(computation) = application.record().as_ref()
                    && let Some(environment) = &computation.environment
                {
                    graph
                        .environment(cx, environment, resources)?
                        .ok_or(GcError::Representation)?;
                }
            }
            Ok(())
        })
    }

    pub(super) fn receiver_operations(
        &self,
        runtime: &mut Runtime,
        execution: SessionId,
        protected: &[MetadataSource<'_>],
    ) -> Result<Option<AdmittedOperationGroup>, RuntimeError> {
        let view = self.facts(runtime, protected)?;
        if view.receiver_table().is_none() {
            return Ok(None);
        }
        match &self.selection {
            SelectionView::Interface { description, slot } => {
                runtime.interface_receiver_operations(execution, description, *slot, protected)
            }
            SelectionView::Operation { id, .. } => {
                let dictionary = runtime.operation_group_in(execution, id.group, protected)?;
                runtime.bind_receiver_operations(
                    view.proof_implementation(),
                    view.proof_target(),
                    &dictionary,
                )
            }
        }
    }
}
