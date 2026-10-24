//! Active method selections are traced by execution windows, not host root leases.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{
        MetadataRoot,
        applications::{ApplicationId, MethodComputation},
        groups::{OperationGroupId, OperationId},
        interfaces::InterfaceSnapshotId,
        links::MethodSelection,
    },
    objects::method_view::{MethodView, SelectionView},
    session::ExecutionPhase,
};

/// A checked selection/application identity. It carries no external root lease;
/// its owner must publish these edges before crossing a collection boundary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MethodInvocation {
    selection: MethodSelection,
    pub(super) application: Option<ApplicationId>,
    dispatch: MethodDispatch,
}

#[derive(Debug, Clone, Copy)]
enum MethodDispatch {
    Live,
    Admitted(ApplicationId),
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
        if view
            .description
            .methods
            .get(slot)
            .and_then(Option::as_ref)
            .is_none()
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface method unavailable",
            ));
        }
        runtime.validate_executable_module(&view.description.implementation)?;
        Ok(Self {
            selection: MethodSelection::Interface { snapshot, slot },
            application: None,
            dispatch: MethodDispatch::Live,
        })
    }

    pub(super) fn from_operation(runtime: &Runtime, id: OperationId) -> Result<Self, RuntimeError> {
        let operation = runtime
            .gc
            .bound_operation(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid selected method operation"))?;
        runtime.validate_executable_module(&operation.owner)?;
        Ok(Self {
            selection: MethodSelection::Operation(id),
            application: operation.application,
            dispatch: MethodDispatch::Live,
        })
    }

    pub(super) fn is_admitted(&self) -> bool {
        matches!(self.dispatch, MethodDispatch::Admitted(_))
    }

    /// Admit once without mutating the reusable relationship/application handle.
    pub(crate) fn admit(mut self, runtime: &Runtime) -> Result<Self, RuntimeError> {
        let view = self.view(runtime)?;
        if matches!(self.dispatch, MethodDispatch::Admitted(_)) {
            return Ok(self);
        }
        let computation = MethodComputation {
            selected: view.selected.clone(),
            environment: view.environment().cloned(),
        };
        drop(view);
        self.dispatch = MethodDispatch::Admitted(runtime.gc.alloc_method_computation(computation)?);
        Ok(self)
    }

    pub(crate) fn append_metadata(self, roots: &mut Vec<MetadataRoot>) {
        roots.push(match self.selection {
            MethodSelection::Interface { snapshot, .. } => MetadataRoot::Interface(snapshot),
            MethodSelection::Operation(id) => MetadataRoot::Operation(id),
        });
        roots.extend(self.application.map(MetadataRoot::Application));
        if let MethodDispatch::Admitted(id) = self.dispatch {
            roots.push(MetadataRoot::Application(id));
        }
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
        let (owner, target) = match &selection {
            SelectionView::Interface { snapshot, slot } => (
                &snapshot.description.implementation,
                snapshot.description.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .target,
            ),
            SelectionView::Operation { operation, .. } => (&operation.owner, operation.target),
        };

        let application =
            self.application
                .map(|id| {
                    runtime.gc.method_application(id).ok_or_else(|| {
                        RuntimeError::module_validation("invalid method application")
                    })
                })
                .transpose()?;
        let (selected, selected_environment) = match self.dispatch {
            MethodDispatch::Live => {
                let selected = runtime.select_call_route(&owner.bind_call_route(target)?)?;
                let environment = application
                    .as_ref()
                    .and_then(|application| application.environment.as_ref())
                    .map(|environment| runtime.transport_environment(owner, &selected, environment))
                    .transpose()?;
                (selected, environment)
            }
            MethodDispatch::Admitted(id) => {
                let computation = runtime.gc.method_computation(id).ok_or_else(|| {
                    RuntimeError::module_validation("invalid admitted method computation")
                })?;
                let owner = computation.selected.owner();
                runtime.validate_executable_module(owner)?;
                let candidate = runtime.resources().active_session().filter(|session| {
                    session.options.phase == ExecutionPhase::CandidateInitialization
                });
                if candidate.as_ref().is_some_and(|session| {
                    session.root.publication_token() != owner.publication_token()
                }) || (candidate.is_none() && runtime.modules.is_staged(owner))
                {
                    return Err(RuntimeError::execution_phase_violation(
                        "admitted method outside its publication scope",
                    ));
                }
                if let Some(environment) = &computation.environment {
                    runtime.validate_environment(environment.id)?;
                }
                (
                    computation.selected.clone(),
                    computation.environment.clone(),
                )
            }
        };
        Ok(MethodView {
            selection,
            application,
            selected,
            selected_environment,
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
                let cell = &snapshot.description.methods[*slot]
                    .as_ref()
                    .expect("checked interface slot")
                    .receiver_operations;
                if let Some(prepared) = cell.get() {
                    return Ok(*prepared);
                }
                let prepared = runtime.bind_receiver_operations(
                    view.proof_implementation(),
                    view.proof_target(),
                    table,
                    snapshot.description.receiver_operations.get().copied(),
                )?;
                let MethodSelection::Interface { snapshot: id, slot } = self.selection else {
                    unreachable!("interface selection view");
                };
                runtime.cache_receiver_operations(id, slot, prepared)?;
                Ok(prepared)
            }
            SelectionView::Operation { .. } => runtime.bind_receiver_operations(
                view.proof_implementation(),
                view.proof_target(),
                table,
                group,
            ),
        }
    }
}
