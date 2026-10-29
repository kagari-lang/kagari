//! Current selection and exact observation share only immutable diagnostic records.
use crate::{
    RootedInterfaceMethod, Runtime,
    closure::{CallableObservation, ObservedCallableTarget},
    error::RuntimeError,
    frame::ExecutionFrame,
    gc::roots::RootedValue,
    module::{
        ExecutableModule, Program,
        dispatch::{CallRoute, CallRouteKind},
    },
    source::{
        FunctionSourceTarget, ProgramSourceSnapshot, SourceDeclaration, SourceQuery,
        SourceSelection, SourceUnavailableReason,
    },
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_contract::source::{AuthoredDeclaration, ExecutableOrigin, ExecutableSource};
use std::{ptr, sync::Arc};

impl FunctionSourceTarget {
    pub(crate) fn current(route: CallRoute) -> Self {
        Self {
            selection: SourceSelection::Current(route),
        }
    }

    pub(crate) fn exact(owner: &ExecutableModule, target: CallableTarget) -> Self {
        let snapshot = owner.source_snapshot();
        let observation = CallableObservation::entry(owner, target);
        Self {
            selection: SourceSelection::Exact {
                runtime: snapshot.data.runtime,
                query: Arc::new(snapshot.query_entry(owner, observation)),
            },
        }
    }
}

impl RootedInterfaceMethod {
    /// Follow the bound implementation's current route, or snapshot an admitted
    /// computation or body-owned adapter's exact target. Neither token retains a
    /// receiver root or code lease, and querying a live route does not repeat
    /// implementation lookup.
    pub fn source_target(&self, runtime: &Runtime) -> Result<FunctionSourceTarget, RuntimeError> {
        if let Some(route) = self.live_call_route(runtime)? {
            match route.kind() {
                CallRouteKind::Named | CallRouteKind::Shared | CallRouteKind::Native => {
                    return Ok(FunctionSourceTarget::current(route));
                }
                CallRouteKind::BodyOwned => {}
            }
        }
        let selected = self.selected_target(runtime)?;
        Ok(FunctionSourceTarget::exact(
            selected.owner(),
            selected.target(),
        ))
    }
}

impl Runtime {
    /// Capture the current publication once without retaining executable code.
    pub fn source_snapshot(
        &self,
        program: &Program,
    ) -> Result<ProgramSourceSnapshot, RuntimeError> {
        Ok(self.select_program(program)?.source_snapshot())
    }

    /// Resolve an opaque authored edit locator against the current verified package.
    /// The returned diagnostic offsets/ID belong to that package's scope; they are
    /// not themselves authority to call or update any declaration.
    pub fn resolve_source_declaration(
        &self,
        program: &Program,
        declaration: &SourceDeclaration,
    ) -> Result<AuthoredDeclaration, RuntimeError> {
        let snapshot = self.source_snapshot(program)?;
        let index = snapshot.declaration_index(declaration).map_err(|reason| {
            RuntimeError::module_validation(format!("source declaration locator: {reason:?}"))
        })?;
        snapshot
            .source_package()
            .map(|package| package.records().declarations[index].clone())
            .ok_or_else(|| {
                RuntimeError::module_validation("source declaration package is not retained")
            })
    }

    /// A current route selects now. An exact observation never selects newer code,
    /// and remains diagnostic after its original computation/installation is gone.
    pub fn function_source(
        &self,
        target: &FunctionSourceTarget,
    ) -> Result<SourceQuery, RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        match &target.selection {
            SourceSelection::Current(route) => {
                let selected = self.select_call_route(route)?;
                let owner = selected.owner();
                Ok(owner
                    .source_snapshot()
                    .query_entry(owner, CallableObservation::entry(owner, selected.target())))
            }
            SourceSelection::Exact { runtime, query } => {
                if *runtime != self.host.owner() {
                    return Err(RuntimeError::module_validation(
                        "foreign source observation",
                    ));
                }
                Ok(query.as_ref().clone())
            }
        }
    }

    /// Observe without polling/claiming a Future or retaining the root in the token.
    pub fn value_source_target(
        &self,
        root: &RootedValue,
    ) -> Result<FunctionSourceTarget, RuntimeError> {
        let observation = self.observe_callable_value(root)?;
        let query = if let Some(observation) = observation {
            let owner = self
                .modules
                .source_observation_owner(observation.publication, observation.module)?
                .ok_or_else(|| {
                    RuntimeError::module_validation("observed callable publication was released")
                })?;
            self.validate_executable_module(&owner)?;
            owner.source_snapshot().query_entry(&owner, observation)
        } else {
            let Some(Value::GcHandle(id)) = root.value(self.gc()) else {
                return Err(RuntimeError::module_validation(
                    "unavailable callable observation root",
                ));
            };
            let (owner, _, _) = self.gc().future_contract(id)?;
            owner
                .source_snapshot()
                .unavailable(SourceUnavailableReason::NoObservedBody)
        };
        Ok(FunctionSourceTarget {
            selection: SourceSelection::Exact {
                runtime: self.host.owner(),
                query: Arc::new(query),
            },
        })
    }

    /// Only a frame borrowed from the active session's existing safe frame view is
    /// accepted. Matching public module/target fields alone are not evidence.
    pub fn frame_source_target(
        &self,
        frame: &ExecutionFrame,
    ) -> Result<FunctionSourceTarget, RuntimeError> {
        self.gc().ensure_no_storage_borrow()?;
        self.resources().ensure_execution_allowed()?;
        let session = self.resources().active_session().ok_or_else(|| {
            RuntimeError::module_validation("source frame requires an active observation session")
        })?;
        let frames = self
            .resources()
            .sessions
            .frames(session.id)
            .ok_or_else(|| {
                RuntimeError::module_validation(
                    "source frame is not at a safe observation boundary",
                )
            })?;
        if !frames.iter().any(|observed| ptr::eq(observed, frame)) {
            return Err(RuntimeError::module_validation(
                "foreign or expired observed frame",
            ));
        }
        self.validate_executable_module(frame.loaded())?;
        Ok(FunctionSourceTarget::exact(frame.loaded(), frame.target()))
    }
}

impl ProgramSourceSnapshot {
    fn query_entry(
        &self,
        owner: &ExecutableModule,
        observation: CallableObservation,
    ) -> SourceQuery {
        let (application, native_binding) = match observation.target {
            ObservedCallableTarget::Entry(CallableTarget::Script(id)) => (
                owner
                    .bytecode
                    .functions
                    .get(id.index())
                    .and_then(|function| function.identity.as_ref()),
                None,
            ),
            ObservedCallableTarget::Entry(CallableTarget::Native(id)) => {
                let import = owner.bytecode.native_imports.get(id.index());
                (
                    import.map(|import| &import.instance),
                    import.map(|import| import.binding),
                )
            }
            ObservedCallableTarget::NativeBinding(id) => (None, Some(id)),
        };
        let association = application
            .and_then(|application| self.data.associations.get(application))
            .cloned();
        let native = native_binding.is_some()
            || association.as_ref().is_some_and(|association| {
                matches!(
                    association.origin,
                    ExecutableOrigin::Native | ExecutableOrigin::Intrinsic
                )
            });
        let mut result = if native {
            self.unavailable(SourceUnavailableReason::NativeOrigin)
        } else if self.source_package().is_none() {
            self.unavailable(SourceUnavailableReason::MissingPackage)
        } else if let Some(authored) = association
            .as_ref()
            .and_then(|association| association.authored)
        {
            match self
                .data
                .scope
                .stable_declaration(authored)
                .ok()
                .and_then(|stable| self.data.indices.get(&stable).copied().flatten())
                .map(|index| &self.data.declarations[index])
            {
                Some(declaration) => self.function(declaration),
                None => self.unavailable(SourceUnavailableReason::UnknownDeclaration),
            }
        } else {
            self.unavailable(SourceUnavailableReason::NotRetained)
        };
        // Native imports retain their checked identity even in stripped products.
        let association = association.or_else(|| {
            application
                .filter(|_| native)
                .map(|target| ExecutableSource {
                    target: target.clone(),
                    authored: None,
                    native_binding,
                    origin: ExecutableOrigin::Native,
                })
        });
        match &mut result {
            SourceQuery::Available(source) => {
                source.target = Some(observation);
                source.executable = association;
            }
            SourceQuery::Unavailable(unavailable) => {
                unavailable.target = Some(observation);
                unavailable.application = application.cloned();
                unavailable.native_binding = native_binding.or_else(|| {
                    association
                        .as_ref()
                        .and_then(|association| association.native_binding)
                });
                unavailable.executable = association;
            }
        }
        result
    }
}
