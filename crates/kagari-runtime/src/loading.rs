use crate::{
    PreparedReload, Runtime, StagedLoad, StagedReload,
    error::{
        RuntimeError,
        fault::EngineFault,
        reason::{CancellationReason, StaleHandleReason},
    },
    module::{
        ExecutableModule, Installation, InstalledProgram, LinkedHostBindings,
        LinkedProgramBindings, ModuleEpochRetention, VerifiedProgram,
        admission::DefinitionAdmission,
        managed::{revision::RevisionState, source::RevisionSource},
        retention::{ProgramLease, RetentionOrigin},
        staging::{ManagedProgramError, StagedProgram},
    },
    native::registry::DependencyValidation,
    program::Program,
    publication::PublicationPermit,
    reload::{
        ModuleEpoch, ReloadValidationError, updates::ReloadAdmission, validate_load_candidate,
        validate_reload_target, validate_verified_reload_candidate,
    },
};

use kagari_bytecode::{
    artifact::{ArtifactCompatibility, KbcArtifact, validate_program_resource_limits},
    instruction::NativeImportId,
    module::BytecodeModule,
    program::BytecodeProgram,
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_gc::root::Root;
use std::sync::Arc;

impl Runtime {
    /// Retain an exact module version from publication, an admitted execution or
    /// an existing lease. Clones share retention; release needs no Runtime borrow.
    /// The handle can outlive Runtime shutdown as an inert, non-accessing token.
    /// Foreign, expired, unavailable or mismatched origins return None. A bare
    /// descriptor cannot resurrect retired state; use Retained for an old lease.
    pub fn retain_module(
        &mut self,
        module: &ExecutableModule,
        kind: ModuleEpochRetention,
        origin: RetentionOrigin<'_>,
    ) -> Option<ProgramLease> {
        let origin = origin.resolve(self, module).ok()?;
        let revision = self.retain_revision(module, origin).ok()?;
        self.modules.retain_module(module, kind, revision)
    }

    pub(crate) fn retain_program(
        &mut self,
        module: &ExecutableModule,
        kind: ModuleEpochRetention,
        origin: RevisionSource<'_>,
    ) -> Option<ProgramLease> {
        let revision = self.retain_revision(module, origin).ok()?;
        self.modules.retain_program(module, kind, revision)
    }

    fn retain_revision(
        &mut self,
        module: &ExecutableModule,
        origin: RevisionSource<'_>,
    ) -> Result<Root<RevisionState>, RuntimeError> {
        self.with_revision_origin(module, origin, |slots, revision| match origin {
            // Admit before sharing. Even another diagnostic retention kind shares
            // the actual Root's existing GC registration.
            RevisionSource::Host(root) => Ok(root.clone()),
            RevisionSource::Current
            | RevisionSource::Execution(_)
            | RevisionSource::Application(_)
            | RevisionSource::HostApplication(_)
            | RevisionSource::RootedClosure(_)
            | RevisionSource::Cold(_)
            | RevisionSource::Closure(_)
            | RevisionSource::StaticRecipe { .. } => slots.scope().retain(revision),
        })
    }

    fn link_native_module(
        &self,
        module: &BytecodeModule,
        program: &VerifiedProgram,
        admission: &DefinitionAdmission,
        validated: &mut DependencyValidation,
    ) -> Result<LinkedHostBindings, RuntimeError> {
        self.native_entries
            .validate_installed_contracts(module, admission)?;
        let mut bindings = self.host.link_module(module, &self.types, admission)?;
        bindings.native = module
            .native_imports
            .iter()
            .map(|import| {
                self.native_entries
                    .link(import, program, admission, validated)
            })
            .collect::<Result<_, _>>()?;
        Ok(bindings)
    }

    fn link_program(
        &self,
        program: &VerifiedProgram,
    ) -> Result<LinkedProgramBindings, RuntimeError> {
        let definitions = DefinitionAdmission::prepare(
            program,
            self.definition_context(),
            &self.execution_options().cancellation,
        )?;
        let mut validated = DependencyValidation::default();
        let members = program
            .modules()
            .iter()
            .map(|module| self.link_native_module(module, program, &definitions, &mut validated))
            .collect::<Result<_, _>>()?;
        // Provider authority comes from installed registrations, never merely
        // from declarations or requirements carried by the product.
        let shared_traits = self
            .native_entries
            .owned_traits
            .iter()
            .filter_map(|id| definitions.product_id(*id))
            .collect();
        Ok(LinkedProgramBindings {
            definitions,
            members,
            shared_traits,
        })
    }

    fn stage_linked_program(
        &mut self,
        installation: Arc<Installation>,
        name: String,
        epoch: ModuleEpoch,
        program: VerifiedProgram,
        bindings: LinkedProgramBindings,
        admission: Option<ReloadAdmission>,
    ) -> Result<StagedProgram, RuntimeError> {
        let staged = self.modules.stage_verified_program(
            installation,
            name,
            epoch,
            program,
            bindings,
            admission,
        )?;
        let staged = staged
            .into_managed(&self.modules, &mut self.memory, &self.resources)
            .map_err(|error| self.managed_memory_error(error))?;
        // Exact program provenance now exists, but the candidate is not published.
        // On failure the lease drops; normal abandoned-candidate collection owns cleanup.
        for owner in staged.module().members() {
            for (index, import) in owner.bytecode.native_imports.iter().enumerate() {
                if import.generic.is_none() {
                    self.modules
                        .native_binding(&owner, NativeImportId::new(index))
                        .ok_or_else(|| {
                            RuntimeError::module_validation("unlinked native signature")
                        })?
                        .prepare_signature(self, &owner)?;
                }
            }
        }
        self.link_staged_execution(&staged)?;
        if staged.is_initial() {
            staged
                .finish_bindings(&self.modules, &mut self.memory, &self.resources)
                .map_err(|error| self.managed_program_error(error))?;
        }
        Ok(staged)
    }

    fn managed_program_error(&self, error: ManagedProgramError) -> RuntimeError {
        match error {
            ManagedProgramError::Validation(error) => error,
            ManagedProgramError::Memory(error) => self.managed_memory_error(error),
        }
    }

    pub fn validate_executable_module(
        &self,
        module: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        if !module.belongs_to(self.host.owner()) {
            return Err(RuntimeError::from(
                StaleHandleReason::ExecutableModuleBelongsToAnotherRuntime,
            ));
        }
        let loaded = self.modules.contains_module(module).map_err(|_| {
            self.resources()
                .quarantine(EngineFault::ModuleStoreIsBorrowedAcrossExecution)
        })?;
        if !loaded {
            return Err(RuntimeError::from(
                StaleHandleReason::ExecutableModuleHasBeenReleased,
            ));
        }
        // Entry/resume admitted this installation before activating the session.
        // Synchronous scopes exclude publication/close, so inner calls reuse that
        // authority without reading current or comparing publication revisions.
        // Explicit retirement still revokes it before cleanup, even with borrowed
        // executable storage. Heap values and native loans remain checked below
        // their own operation boundaries, independently of this code admission.
        if self
            .resources()
            .active_session()
            .is_some_and(|session| session.root.installation_id() == module.installation_id())
            && self.modules.activation_is_live()
        {
            return Ok(());
        }
        self.modules.validate_executable_installation(module)
    }

    /// Stage and publish shared verified code with this runtime's bindings and state.
    /// Use the two-step initial-load API when host associations must precede commit.
    pub fn load_verified_program(
        &mut self,
        name: impl Into<String>,
        program: VerifiedProgram,
    ) -> Result<InstalledProgram, RuntimeError> {
        let candidate = self.stage_load_verified_program(name, program)?;
        self.publish_staged_load(candidate)
    }

    /// Prepare a fresh installation without exposing a live Program.
    ///
    /// Completes executable/source metadata, current target tables and native linking
    /// before returning the candidate token. Runs no script, initializer, host callback
    /// or GC cleanup. Foreign metadata, linking, resource and admission errors leave
    /// the installation unpublished; abandoned storage follows candidate-lease cleanup.
    pub fn stage_load_verified_program(
        &mut self,
        name: impl Into<String>,
        program: VerifiedProgram,
    ) -> Result<StagedLoad, RuntimeError> {
        self.check_initial_publication()?;
        let name = name.into();

        let bindings = self.link_program(&program)?;
        let installation = self.modules.reserve_installation(
            self.host.owner(),
            program.modules()[program.root().index()].identity.clone(),
        )?;
        let epoch = self.modules.reserve_epoch(installation.id)?;
        let program =
            self.stage_linked_program(installation.clone(), name, epoch, program, bindings, None)?;
        Ok(StagedLoad {
            installation,
            program,
        })
    }

    /// Publish a fully staged initial installation after host association succeeds.
    ///
    /// Rejects foreign candidates, closed/faulted runtimes, synchronous scopes and
    /// active publication guards. The store validates the candidate lease and empty
    /// installation baseline under its commit guards. No callbacks or fallible work
    /// follow visibility; the returned Program has the candidate's original token.
    pub fn publish_staged_load(
        &mut self,
        candidate: StagedLoad,
    ) -> Result<InstalledProgram, RuntimeError> {
        self.check_initial_publication()?;
        let StagedLoad {
            installation,
            program,
        } = candidate;
        self.modules
            .validate_installation(&installation, self.host.owner())?;
        self.validate_executable_module(program.module())?;
        self.validate_publication_bindings(program.module())?;
        program
            .publish(&self.modules, &mut self.memory, &self.resources)
            .map_err(|error| self.managed_program_error(error))?;
        Ok(InstalledProgram { installation })
    }

    fn validate_publication_bindings(
        &self,
        program: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        let mut validated = DependencyValidation::default();
        for member in program.members() {
            let current = self.link_native_module(
                &member.bytecode,
                member.verified_program(),
                member.identity_admission(),
                &mut validated,
            )?;
            if current.paths != member.host_paths {
                return Err(RuntimeError::module_validation(
                    "host bindings changed after candidate staging",
                ));
            }
        }
        Ok(())
    }

    pub fn stage_reload_program(
        &mut self,
        target: &InstalledProgram,
        program_name: Option<String>,
        bytecode: DefinitionMetadata<BytecodeProgram>,
    ) -> Result<StagedReload, ReloadValidationError> {
        self.select_program(target)
            .map_err(ReloadValidationError::Runtime)?;
        validate_program_resource_limits(
            bytecode.records(),
            bytecode.definitions(),
            &Default::default(),
        )
        .map_err(ReloadValidationError::Artifact)?;
        let bytecode = bytecode
            .import_into(self.definition_context(), &CancellationToken::default())
            .map_err(|error| {
                ReloadValidationError::Runtime(RuntimeError::module_validation(error.to_string()))
            })?;

        let verified = validate_load_candidate(bytecode.into_records(), self.definition_context())?;
        let program = VerifiedProgram::from_bytecode(verified);
        self.stage_reload_verified_program(target, program_name, program)
    }

    pub fn stage_reload_artifact(
        &mut self,
        target: &InstalledProgram,
        program_name: Option<String>,
        artifact: DefinitionMetadata<KbcArtifact>,
        compatibility: &ArtifactCompatibility,
    ) -> Result<StagedReload, ReloadValidationError> {
        self.select_program(target)
            .map_err(ReloadValidationError::Runtime)?;
        artifact
            .records()
            .validate_resource_limits(artifact.definitions(), &Default::default())
            .map_err(ReloadValidationError::Artifact)?;
        let artifact = artifact
            .import_into(self.definition_context(), &CancellationToken::default())
            .map_err(|error| {
                ReloadValidationError::Runtime(RuntimeError::module_validation(error.to_string()))
            })?;
        let verified = artifact
            .into_records()
            .into_verified(
                compatibility,
                self.definition_context(),
                &Default::default(),
            )
            .map_err(ReloadValidationError::Artifact)?;
        let (bytecode, _, source) = verified.into_parts();
        let mut program = VerifiedProgram::from_bytecode(bytecode);
        if let Some(source) = source {
            program = program
                .with_source_package(source, &self.execution_options().cancellation)
                .map_err(ReloadValidationError::Runtime)?;
        }
        self.stage_reload_verified_program(target, program_name, program)
    }

    /// Stage shared verified code without changing its immutable program identity.
    /// Host bindings, candidate state, permissions and publication remain runtime-local.
    pub fn stage_reload_verified_program(
        &mut self,
        target: &InstalledProgram,
        program_name: Option<String>,
        program: VerifiedProgram,
    ) -> Result<StagedReload, ReloadValidationError> {
        let candidate = self.prepare_reload(target, program_name, program)?;
        self.stage_prepared_reload(candidate)
    }

    pub(super) fn prepare_reload(
        &mut self,
        target: &InstalledProgram,
        program_name: Option<String>,
        program: VerifiedProgram,
    ) -> Result<PreparedReload, ReloadValidationError> {
        let baseline = self
            .select_program(target)
            .map_err(ReloadValidationError::Runtime)?;
        let name = program_name.unwrap_or_else(|| baseline.name.clone());
        let admission = validate_verified_reload_candidate(&baseline, &program)?;
        let bindings = self
            .link_program(&program)
            .map_err(ReloadValidationError::Runtime)?;
        Ok(PreparedReload {
            installation: target.installation.clone(),
            admission,
            baseline,
            name,
            program,
            bindings,
        })
    }

    pub(super) fn stage_prepared_reload(
        &mut self,
        candidate: PreparedReload,
    ) -> Result<StagedReload, ReloadValidationError> {
        let PreparedReload {
            installation,
            admission,
            baseline,
            name,
            program,
            bindings,
        } = candidate;
        self.validate_executable_module(&baseline)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self
            .modules
            .current_for(&baseline)
            .map_err(ReloadValidationError::Runtime)?;
        // Only admission may have changed; do not rebuild immutable checked evidence.
        validate_reload_target(&baseline, Some(&latest))?;
        let mut validated = DependencyValidation::default();
        for (module, prepared) in program.modules().iter().zip(&bindings.members) {
            let current = self
                .link_native_module(module, &program, &bindings.definitions, &mut validated)
                .map_err(ReloadValidationError::Runtime)?;
            if current.paths != prepared.paths {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::module_validation(
                        "host bindings changed after reload preparation",
                    ),
                ));
            }
        }
        // Collection can drop host payloads. Guarded updates only link metadata;
        // leave abandoned storage to the next ordinary driver safepoint.
        if !self.resources().publication_active() && self.modules.abandonment_pending() {
            self.gc_safepoint()
                .map_err(ReloadValidationError::Runtime)?;
        }
        let epoch = self
            .modules
            .reserve_epoch(installation.id)
            .map_err(ReloadValidationError::Runtime)?;
        let target = InstalledProgram {
            installation: installation.clone(),
        };
        let program = self
            .stage_linked_program(
                installation,
                name,
                epoch,
                program,
                bindings,
                Some(admission),
            )
            .map_err(ReloadValidationError::Runtime)?;
        let program = program
            .bind_reload(&self.modules, &baseline, &mut self.memory, &self.resources)
            .map_err(|error| ReloadValidationError::Runtime(self.managed_program_error(error)))?;
        Ok(StagedReload {
            target,
            initialization_error: Default::default(),
            baseline,
            program,
        })
    }

    /// Admit and publish a previously staged candidate at the host boundary.
    /// Call `PublicationGuard::publish` instead when admission is already held.
    ///
    /// Publication requires the exclusive host owner, including from callbacks:
    /// ```compile_fail
    /// fn publish(runtime: &kagari_runtime::Runtime, candidate: kagari_runtime::StagedReload) {
    ///     runtime.publish_staged_reload(candidate).unwrap();
    /// }
    /// ```
    pub fn publish_staged_reload(
        &mut self,
        candidate: StagedReload,
    ) -> Result<(), ReloadValidationError> {
        self.begin_publication(&candidate.target)
            .map_err(ReloadValidationError::Runtime)?
            .publish(candidate)
    }

    pub(crate) fn publish_admitted_reload(
        &mut self,
        candidate: StagedReload,
        permit: &PublicationPermit,
    ) -> Result<(), ReloadValidationError> {
        permit
            .recheck(self)
            .map_err(ReloadValidationError::Runtime)?;
        if let Some(error) = candidate.initialization_error() {
            return Err(ReloadValidationError::Runtime(error));
        }
        let StagedReload {
            target: _,
            initialization_error: _,
            baseline,
            program,
        } = candidate;
        self.validate_executable_module(&baseline)
            .map_err(ReloadValidationError::Runtime)?;
        self.validate_executable_module(program.module())
            .map_err(ReloadValidationError::Runtime)?;

        let latest = self
            .modules
            .current_for(&baseline)
            .map_err(ReloadValidationError::Runtime)?;
        validate_reload_target(&baseline, Some(&latest))?;
        self.validate_publication_bindings(program.module())
            .map_err(ReloadValidationError::Runtime)?;
        let invalidation = self
            .interpreter_caches
            .prepare_invalidation(program.module().installation_id(), program.module().epoch)
            .map_err(ReloadValidationError::Runtime)?;
        permit
            .recheck(self)
            .map_err(ReloadValidationError::Runtime)?;
        program
            .publish(&self.modules, &mut self.memory, &self.resources)
            .map_err(|error| ReloadValidationError::Runtime(self.managed_program_error(error)))?;
        invalidation.commit();
        Ok(())
    }
}

/// Local installation naming; executable code remains shared by the Program.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub program_name: Option<String>,
}

/// Local replacement naming and cooperative publication cancellation.
#[derive(Debug, Clone, Default)]
pub struct ReloadOptions {
    pub program_name: Option<String>,
    pub cancellation: CancellationToken,
}

impl Runtime {
    /// Link and publish an immutable product with this Runtime's providers and state.
    /// Does not compile, run script code or initialize statics; failed admission
    /// leaves no live installation. Use staged loading for precommit host associations.
    pub fn load_program(
        &mut self,
        program: &Program,
        options: LoadOptions,
    ) -> Result<InstalledProgram, RuntimeError> {
        let program_name = options.program_name.unwrap_or_else(|| {
            program.bytecode().modules()[program.bytecode().root().index()]
                .source_name
                .clone()
        });
        let candidate =
            self.stage_load_verified_program(program_name, program.clone().into_verified())?;
        let loaded = self.publish_staged_load(candidate)?;
        Ok(loaded)
    }

    /// Publish a candidate checked against this installation's exact current
    /// shared base. No compiler/query cache or source preparation is accessed.
    /// Cancellation or any admission failure leaves current code/state unchanged.
    pub fn replace_program(
        &mut self,
        previous: &InstalledProgram,
        program: &Program,
        options: ReloadOptions,
    ) -> Result<(), ReloadValidationError> {
        options.cancellation.check().map_err(|_| {
            ReloadValidationError::Runtime(RuntimeError::from(
                CancellationReason::ExecutionCancelled,
            ))
        })?;
        let base = self
            .program(previous)
            .map_err(ReloadValidationError::Runtime)?;
        if !program.expects_base(&base) {
            return Err(ReloadValidationError::Runtime(RuntimeError::from(
                StaleHandleReason::InstallationPublicationBaselineChangedOrClosed,
            )));
        }
        let mut guard = self
            .begin_publication(previous)
            .map_err(ReloadValidationError::Runtime)?;
        let candidate = guard
            .stage_reload_verified_program(options.program_name, program.clone().into_verified())?;
        options.cancellation.check().map_err(|_| {
            ReloadValidationError::Runtime(RuntimeError::from(
                CancellationReason::ExecutionCancelled,
            ))
        })?;
        guard.publish(candidate)?;
        Ok(())
    }
}

impl Runtime {
    /// Acquire the stable shared executable owner of the current installation.
    /// Clones retain code and checked history, without compiler inputs or live state.
    /// Rejects foreign or retired installations before exposing their product.
    pub fn program(&self, installed: &InstalledProgram) -> Result<Program, RuntimeError> {
        let current = self.select_program(installed)?;
        current
            .verified_program()
            .product()
            .cloned()
            .ok_or_else(|| {
                self.quarantine_execution_invariant(EngineFault::HostInvariant(
                    "installed code has no canonical Program owner",
                ))
            })
    }
}
