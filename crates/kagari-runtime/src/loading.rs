use crate::{
    PreparedReload, Runtime, StagedLoad, StagedReload,
    error::RuntimeError,
    module::{
        ExecutableModule, Installation, LinkedHostBindings, LinkedProgramBindings,
        ModuleEpochRetention, Program, VerifiedProgram, retention::ProgramLease,
        staging::StagedProgram,
    },
    publication::PublicationGuard,
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
use std::sync::Arc;

impl Runtime {
    /// Keep one exact module version reachable until the last lease is dropped.
    /// The lease does not own runtime storage and may outlive the runtime.
    pub fn retain_module(
        &self,
        module: &ExecutableModule,
        kind: ModuleEpochRetention,
    ) -> Option<ProgramLease> {
        self.resources().ensure_execution_allowed().ok()?;
        self.modules.retain_module(module, kind)
    }

    pub(crate) fn retain_program(
        &self,
        module: &ExecutableModule,
        kind: ModuleEpochRetention,
    ) -> Option<ProgramLease> {
        self.resources().ensure_execution_allowed().ok()?;
        self.modules.retain_program(module, kind)
    }

    fn link_native_module(
        &self,
        module: &BytecodeModule,
        program: &VerifiedProgram,
    ) -> Result<LinkedHostBindings, RuntimeError> {
        self.native_entries.validate_installed_contracts(module)?;
        let mut bindings = self
            .host
            .link_module(module, &self.types, program.definitions())?;
        bindings.native = module
            .native_imports
            .iter()
            .enumerate()
            .map(|(index, import)| {
                self.native_entries
                    .link(import, NativeImportId::new(index), program)
            })
            .collect::<Result<_, _>>()?;
        Ok(bindings)
    }

    fn stage_linked_program(
        &self,
        installation: Arc<Installation>,
        name: String,
        epoch: ModuleEpoch,
        program: VerifiedProgram,
        bindings: Vec<LinkedHostBindings>,
        admission: Option<ReloadAdmission>,
    ) -> Result<StagedProgram, RuntimeError> {
        // Capture declaration-level provider authority in this publication. The
        // catalog also contains script requirements, even under provided modules.
        let shared_traits = self.native_entries.owned_traits.clone();
        let staged = self.modules.stage_verified_program(
            installation,
            name,
            epoch,
            program,
            LinkedProgramBindings {
                members: bindings,
                shared_traits,
            },
            admission,
        )?;
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
        for owner in staged.module().members() {
            self.link_execution(&owner)?;
        }
        Ok(staged)
    }

    pub fn load_program(
        &mut self,
        name: impl Into<String>,
        bytecode: DefinitionMetadata<BytecodeProgram>,
    ) -> Result<Program, RuntimeError> {
        let verified = VerifiedProgram::new(bytecode, self.definition_context())?;
        self.load_verified_program(name, verified)
    }

    /// Stage and publish shared verified code with this runtime's bindings and state.
    /// Use the two-step initial-load API when host associations must precede commit.
    pub fn load_verified_program(
        &mut self,
        name: impl Into<String>,
        program: VerifiedProgram,
    ) -> Result<Program, RuntimeError> {
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
        let program = program.normalized(
            self.definition_context(),
            &self.execution_options().cancellation,
        )?;
        let name = name.into();

        let bindings = program
            .modules()
            .iter()
            .map(|module| self.link_native_module(module, &program))
            .collect::<Result<Vec<_>, _>>()?;
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
    pub fn publish_staged_load(&mut self, candidate: StagedLoad) -> Result<Program, RuntimeError> {
        self.check_initial_publication()?;
        let StagedLoad {
            installation,
            program,
        } = candidate;
        self.modules
            .validate_installation(&installation, self.host.owner())?;
        self.validate_executable_module(program.module())?;
        self.validate_publication_bindings(program.module())?;
        program.publish(&self.modules)?;
        Ok(Program { installation })
    }

    fn validate_publication_bindings(
        &self,
        program: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        for member in program.members() {
            let current = self.link_native_module(&member.bytecode, member.verified_program())?;
            if current.paths != member.host_paths {
                return Err(RuntimeError::module_validation(
                    "host bindings changed after candidate staging",
                ));
            }
        }
        Ok(())
    }

    pub fn stage_reload_program(
        &self,
        target: &Program,
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
        &self,
        target: &Program,
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
        &self,
        target: &Program,
        program_name: Option<String>,
        program: VerifiedProgram,
    ) -> Result<StagedReload, ReloadValidationError> {
        let candidate = self.prepare_reload(target, program_name, program)?;
        self.stage_prepared_reload(candidate)
    }

    pub(super) fn prepare_reload(
        &self,
        target: &Program,
        program_name: Option<String>,
        program: VerifiedProgram,
    ) -> Result<PreparedReload, ReloadValidationError> {
        let baseline = self
            .select_program(target)
            .map_err(ReloadValidationError::Runtime)?;
        let name = program_name.unwrap_or_else(|| baseline.name.clone());
        let program = program
            .normalized(
                self.definition_context(),
                &self.execution_options().cancellation,
            )
            .map_err(ReloadValidationError::Runtime)?;
        let admission = validate_verified_reload_candidate(&baseline, &program)?;
        let bindings = program
            .modules()
            .iter()
            .map(|module| self.link_native_module(module, &program))
            .collect::<Result<Vec<_>, _>>()
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
        &self,
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
        for (module, prepared) in program.modules().iter().zip(&bindings) {
            let current = self
                .link_native_module(module, &program)
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
        let target = Program {
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
            .bind_reload(&self.modules, &baseline)
            .map_err(ReloadValidationError::Runtime)?;
        Ok(StagedReload {
            target,
            initialization_error: Default::default(),
            baseline,
            program,
        })
    }

    /// Admit and publish a previously staged candidate at the host boundary.
    /// Call `PublicationGuard::publish` instead when admission is already held.
    pub fn publish_staged_reload(
        &self,
        candidate: StagedReload,
    ) -> Result<(), ReloadValidationError> {
        self.begin_publication(&candidate.target)
            .map_err(ReloadValidationError::Runtime)?
            .publish(candidate)
    }

    pub(crate) fn publish_admitted_reload(
        &self,
        candidate: StagedReload,
        guard: &PublicationGuard<'_>,
    ) -> Result<(), ReloadValidationError> {
        guard.recheck().map_err(ReloadValidationError::Runtime)?;
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
        guard.recheck().map_err(ReloadValidationError::Runtime)?;
        program
            .publish(&self.modules)
            .map_err(ReloadValidationError::Runtime)?;
        invalidation.commit();
        Ok(())
    }
}
