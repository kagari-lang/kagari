//! Installation-stable callable routes and exact, checked executable selection.
//!
//! Linking captures a route once. Ordinary calls resolve that route against the
//! installation's current table; generated bodies retain their exact publication.
//! Neither routes nor publication tokens retain an executable lease. A selected
//! target must be retained/published by the existing frame or computation owner
//! before crossing a collection boundary.
pub mod identity;
mod tables;
#[cfg(test)]
mod tests;

use crate::{
    Runtime, StagedReload,
    error::RuntimeError,
    host::HostRegistryId,
    module::{
        ExecutableModule, InstallationId, ModuleKey, Program,
        dispatch::{
            identity::{
                DeclarationIndex, PublicationDefinitions, PublicationMapping, StableDeclarationId,
            },
            tables::{DispatchTables, RouteSelection},
        },
    },
    reload::ModuleEpoch,
    session::ExecutionPhase,
};
use kagari_bytecode::{module::CallableTarget, program::ModuleRef};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_contract::{
    callable::generic::GenericBody, native_import::NativeImport, types::ConcreteFunctionIdentity,
    updates::BodyClassification,
};
use kagari_types::callable::Signature;
use std::sync::Arc;

/// Exact installation publication identity, available before publication.
/// This copyable diagnostic/association key contains no code, GC root or lease;
/// possessing it never admits execution or revives an abandoned candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicationToken {
    pub(super) registry: HostRegistryId,
    pub(super) installation: InstallationId,
    epoch: ModuleEpoch,
}

impl PublicationToken {
    pub(super) fn new(
        registry: HostRegistryId,
        installation: InstallationId,
        epoch: ModuleEpoch,
    ) -> Self {
        Self {
            registry,
            installation,
            epoch,
        }
    }

    pub fn installation_id(self) -> InstallationId {
        self.installation
    }

    pub fn epoch(self) -> ModuleEpoch {
        self.epoch
    }
}

/// Selection policy established by verified body/import evidence, not capture count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallRouteKind {
    Named,
    Shared,
    Native,
    BodyOwned,
}

/// A checked binding to one installation's declaration/application or exact body.
/// Public path lookup happens before construction; later alias changes cannot retarget
/// this binding. The metadata scope is not an executable owner or a runtime root.
#[derive(Debug, Clone)]
pub struct CallRoute {
    scope: Arc<PublicationDefinitions>,
    origin: ModuleKey,
    target: CallableTarget,
    selection: RouteSelection,
}

impl CallRoute {
    pub fn kind(&self) -> CallRouteKind {
        match &self.selection {
            RouteSelection::Named { shared: false, .. } => CallRouteKind::Named,
            RouteSelection::Named { shared: true, .. } => CallRouteKind::Shared,
            RouteSelection::Native { .. } => CallRouteKind::Native,
            RouteSelection::BodyOwned => CallRouteKind::BodyOwned,
        }
    }

    pub fn definitions(&self) -> &Arc<PublicationDefinitions> {
        &self.scope
    }

    pub fn declaration(&self) -> Result<Option<StableDeclarationId>, RuntimeError> {
        let id = match &self.selection {
            RouteSelection::Named { application, .. } => application.declaration,
            RouteSelection::Native { import, .. } => import.instance.declaration,
            RouteSelection::BodyOwned => return Ok(None),
        };
        self.scope.stable_declaration(id).map(Some)
    }

    /// Original checked application. Shared entry arguments/witnesses are separate
    /// call-site evidence; these identity arguments must not be used as an environment.
    pub fn application(&self) -> Option<&ConcreteFunctionIdentity> {
        match &self.selection {
            RouteSelection::Named { application, .. } => Some(application),
            RouteSelection::Native { import, .. } => Some(&import.instance),
            RouteSelection::BodyOwned => None,
        }
    }
}

/// Exact selection plus transport from the binding's original declaration scope.
/// Signature/binders remain in `owner().definitions()`. Environment construction
/// consumes caller/receiver evidence separately and must retain its original origins.
#[derive(Debug, Clone)]
pub struct SelectedTarget {
    owner: ExecutableModule,
    target: CallableTarget,
    mapping: PublicationMapping,
}

impl SelectedTarget {
    pub fn owner(&self) -> &ExecutableModule {
        &self.owner
    }

    pub fn target(&self) -> CallableTarget {
        self.target
    }

    pub fn publication_token(&self) -> PublicationToken {
        self.owner.publication_token()
    }

    pub fn mapping(&self) -> &PublicationMapping {
        &self.mapping
    }

    pub fn signature(&self) -> &Signature {
        &self.record().signature
    }

    pub fn generic(&self) -> Option<&GenericBody> {
        self.record().generic.as_ref()
    }

    pub fn classification(&self) -> Option<&BodyClassification> {
        self.record().classification.as_ref()
    }

    pub fn native_import(&self) -> Option<&NativeImport> {
        match self.target {
            CallableTarget::Native(id) => self.owner.bytecode.native_imports.get(id.index()),
            CallableTarget::Script(_) => None,
        }
    }

    /// Transport another retained caller/receiver scope to this same selected entry.
    /// Target binder preferences are kept; no source/trait lookup is performed.
    pub fn mapping_from(
        &self,
        source: Arc<PublicationDefinitions>,
    ) -> Result<PublicationMapping, RuntimeError> {
        PublicationMapping::new(
            source,
            self.owner.publication_definitions(),
            self.record().binders.iter().copied(),
        )
    }

    fn record(&self) -> &tables::TargetRecord {
        self.owner
            .program
            .dispatch
            .target(self.owner.slot(), self.target)
            .expect("selected checked dispatch target")
    }
}

#[derive(Debug)]
pub(super) struct PublicationDispatch {
    index: DeclarationIndex,
    pub(super) scope: Arc<PublicationDefinitions>,
    tables: DispatchTables,
}

impl PublicationDispatch {
    fn target(&self, module: ModuleRef, target: CallableTarget) -> Option<&tables::TargetRecord> {
        self.tables.target(module, target)
    }
}

impl ExecutableModule {
    pub fn publication_token(&self) -> PublicationToken {
        self.program.dispatch.scope.publication_token()
    }

    /// Copy the original-to-stable declaration map without retaining executable code.
    pub fn publication_definitions(&self) -> Arc<PublicationDefinitions> {
        self.program.dispatch.scope.clone()
    }

    /// Capture verified target policy once, including private/internal checked slots.
    /// This is backend linking metadata, not public-name/visibility admission.
    /// Calling still requires Runtime validation and existing argument/access checks.
    pub fn bind_call_route(&self, target: CallableTarget) -> Result<CallRoute, RuntimeError> {
        let record = self
            .program
            .dispatch
            .target(self.slot, target)
            .ok_or_else(|| invalid("missing checked callable route"))?;
        Ok(CallRoute {
            scope: self.publication_definitions(),
            origin: self.key(),
            target,
            selection: record.selection.clone(),
        })
    }

    pub fn stable_declaration(
        &self,
        id: DefinitionId,
    ) -> Result<StableDeclarationId, RuntimeError> {
        self.program.dispatch.scope.stable_declaration(id)
    }
}

impl StagedReload {
    /// Association identity for SDK precommit work; it does not publish the candidate.
    pub fn publication_token(&self) -> PublicationToken {
        self.module().publication_token()
    }
}

impl Runtime {
    /// Capture current association identity without returning an executable lease.
    pub fn publication_token(&self, program: &Program) -> Result<PublicationToken, RuntimeError> {
        self.select_program(program)
            .map(|owner| owner.publication_token())
    }

    /// Admit a previously linked route and select its current or retained body.
    /// Candidate sessions select only their own staged table; they cannot escape to
    /// installed code through a retained external binding. No environment is inferred.
    pub fn select_call_route(&self, route: &CallRoute) -> Result<SelectedTarget, RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        let token = route.scope.token;
        let installation = self
            .modules
            .installations
            .get(&token.installation)
            .ok_or_else(|| invalid("retired callable installation"))?;
        self.modules
            .validate_installation(installation, self.host.owner())?;
        if token.registry != self.host.owner() {
            return Err(invalid("foreign callable installation"));
        }
        let candidate = self.resources().active_session().and_then(|session| {
            (session.options.phase == ExecutionPhase::CandidateInitialization)
                .then(|| session.root.clone())
        });
        let root = if let Some(candidate) = candidate {
            if candidate.publication_token() != token {
                return Err(RuntimeError::execution_phase_violation(
                    "external route in candidate execution",
                ));
            }
            candidate
        } else {
            let current = installation.capture_current()?;
            if !route.scope.is_published() {
                return Err(RuntimeError::execution_phase_violation(
                    "staged callable requires candidate execution",
                ));
            }
            current
        };
        // A linked route already names the exact target in its publication. Only
        // crossing publications needs declaration correspondence and selection.
        let (owner, target) = if root.publication_token() == token {
            let owner = self
                .modules
                .loaded(route.origin)
                .filter(|owner| owner.publication_token() == token)
                .ok_or_else(|| invalid("linked callable body was released"))?;
            (owner, route.target)
        } else {
            match &route.selection {
                RouteSelection::BodyOwned => {
                    let owner = self
                        .modules
                        .loaded(route.origin)
                        .filter(|owner| owner.publication_token() == token)
                        .ok_or_else(|| invalid("retained callable body was released"))?;
                    (owner, route.target)
                }
                selection => {
                    let (module, target) = root.program.dispatch.tables.select(
                        &root.program.dispatch.scope,
                        &route.scope,
                        selection,
                        &CancellationToken::default(),
                    )?;
                    let owner = root
                        .member(module)
                        .ok_or_else(|| invalid("current target module"))?;
                    (owner, target)
                }
            }
        };
        self.validate_executable_module(&owner)?;
        let record = owner
            .program
            .dispatch
            .target(owner.slot(), target)
            .ok_or_else(|| invalid("current target contract"))?;
        let mapping = PublicationMapping::new(
            route.scope.clone(),
            owner.publication_definitions(),
            record.binders.iter().copied(),
        )?;
        Ok(SelectedTarget {
            owner,
            target,
            mapping,
        })
    }
}

pub(super) fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::module_validation(message)
}
