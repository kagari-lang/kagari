use crate::{
    PreparedReload, Runtime, StagedReload,
    cache::{InterpreterCacheId, ReloadDependencySnapshot, ReloadInvalidation},
    error::RuntimeError,
    module::{
        LinkedHostBindings, LoadedModule, ModuleEpochRetention, ModuleKey, VerifiedProgram,
        retention::ProgramLease,
    },
    native::registry::link_host,
    reload::{
        ReloadValidationError, validate_reload_artifact_candidate, validate_reload_candidate,
        validate_verified_reload_candidate,
    },
    session::ExecutionPhase,
};
use kagari_bytecode::{
    artifact::{
        ArtifactCompatibility, ArtifactFingerprint, KbcArtifact, validate_program_resource_limits,
    },
    module::BytecodeModule,
    program::BytecodeProgram,
};
use kagari_common::identity::table::DefinitionId;

impl Runtime {
    /// Keep one exact module version reachable until the last lease is dropped.
    /// The lease does not own runtime storage and may outlive the runtime.
    pub fn retain_module(
        &self,
        module: &LoadedModule,
        kind: ModuleEpochRetention,
    ) -> Option<ProgramLease> {
        self.resources().ensure_execution_allowed().ok()?;
        self.modules.retain_module(module, kind)
    }

    pub(crate) fn retain_program(
        &self,
        module: &LoadedModule,
        kind: ModuleEpochRetention,
    ) -> Option<ProgramLease> {
        self.resources().ensure_execution_allowed().ok()?;
        self.modules.retain_program(module, kind)
    }

    pub(crate) fn allows_instance_access(&self, key: ModuleKey) -> bool {
        self.resources().active_session().is_none_or(|session| {
            session.options.phase != ExecutionPhase::CandidateInitialization
                || session.root.members().any(|member| member.key() == key)
        })
    }

    fn link_native_module(
        &self,
        module: &BytecodeModule<DefinitionId>,
        program: &VerifiedProgram,
    ) -> Result<LinkedHostBindings, RuntimeError> {
        self.native_entries.validate_installed_traits(module)?;
        let mut bindings = self
            .host
            .link_module(module, &self.types, program.definitions())?;
        bindings.native = module
            .native_imports
            .iter()
            .map(|import| {
                if let Some(required) = &import.host {
                    let slot = module
                        .host_interface
                        .functions
                        .iter()
                        .position(|host| host == required)
                        .ok_or_else(|| {
                            RuntimeError::module_validation("missing host authority contract")
                        })?;
                    let binding = *bindings.functions.get(slot).ok_or_else(|| {
                        RuntimeError::module_validation("missing host native entry")
                    })?;
                    Ok(link_host(import, binding))
                } else {
                    self.native_entries.link(import, program)
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(bindings)
    }

    pub fn validate_loaded_module(&self, module: &LoadedModule) -> Result<(), RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        if !module.belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation(
                "loaded module belongs to another runtime or has been released",
            ));
        }
        let loaded = self.modules.contains_module(module).map_err(|_| {
            self.resources()
                .quarantine("module store is borrowed across execution")
        })?;
        if !loaded {
            return Err(RuntimeError::module_validation(
                "loaded module belongs to another runtime or has been released",
            ));
        }
        Ok(())
    }

    pub fn load_program(
        &mut self,
        name: impl Into<String>,
        bytecode: BytecodeProgram,
    ) -> Result<LoadedModule, RuntimeError> {
        self.load_verified_program(name, VerifiedProgram::new(bytecode)?)
    }

    /// Link shared verified code with this runtime's own host bindings and state.
    pub fn load_verified_program(
        &mut self,
        name: impl Into<String>,
        program: VerifiedProgram,
    ) -> Result<LoadedModule, RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        let program = program.normalized(self.definition_context())?;
        let name = name.into();
        let dependencies = program.dependencies().clone();
        let bindings = program
            .modules()
            .iter()
            .map(|module| self.link_native_module(module, &program))
            .collect::<Result<Vec<_>, _>>()?;
        if self.modules.has_abandoned_programs()? {
            self.gc_safepoint()?;
        }
        let epoch = self.modules.reserve_epoch(&name)?;
        let module = self
            .modules
            .stage_verified_program(name, epoch, program, self.host.owner(), bindings)?
            .publish(&self.modules)?;
        self.invalidate_interpreter_caches_for_reload(&module, dependencies);
        Ok(module)
    }

    pub fn stage_reload_program(
        &self,
        active: &LoadedModule,
        name: impl Into<String>,
        bytecode: BytecodeProgram,
    ) -> Result<StagedReload, ReloadValidationError> {
        let name = name.into();
        validate_program_resource_limits(&bytecode).map_err(ReloadValidationError::Artifact)?;
        self.validate_loaded_module(active)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&active.name);
        validate_reload_candidate(active, &name, &bytecode, latest.as_ref())?;
        let program = VerifiedProgram::new(bytecode).map_err(ReloadValidationError::Runtime)?;
        self.stage_reload_verified_program(active, name, program)
    }

    pub fn stage_reload_artifact(
        &self,
        active: &LoadedModule,
        name: impl Into<String>,
        artifact: KbcArtifact,
        compatibility: &ArtifactCompatibility,
    ) -> Result<StagedReload, ReloadValidationError> {
        let name = name.into();
        self.validate_loaded_module(active)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&active.name);
        validate_reload_artifact_candidate(
            active,
            &name,
            &artifact,
            compatibility,
            latest.as_ref(),
        )?;
        let program =
            VerifiedProgram::new(artifact.program).map_err(ReloadValidationError::Runtime)?;
        self.stage_reload_verified_program(active, name, program)
    }

    /// Stage shared verified code without changing its immutable program identity.
    /// Host bindings, candidate state, permissions and publication remain runtime-local.
    pub fn stage_reload_verified_program(
        &self,
        active: &LoadedModule,
        name: impl Into<String>,
        program: VerifiedProgram,
    ) -> Result<StagedReload, ReloadValidationError> {
        let candidate = self.prepare_reload(active, name.into(), program)?;
        self.stage_prepared_reload(candidate)
    }

    pub(super) fn prepare_reload(
        &self,
        baseline: &LoadedModule,
        name: String,
        program: VerifiedProgram,
    ) -> Result<PreparedReload, ReloadValidationError> {
        let program = program
            .normalized(self.definition_context())
            .map_err(ReloadValidationError::Runtime)?;
        self.validate_loaded_module(baseline)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&baseline.name);
        validate_verified_reload_candidate(baseline, &name, &program, latest.as_ref())?;
        let bindings = program
            .modules()
            .iter()
            .map(|module| self.link_native_module(module, &program))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ReloadValidationError::Runtime)?;
        Ok(PreparedReload {
            baseline: baseline.clone(),
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
            baseline,
            name,
            program,
            bindings,
        } = candidate;
        self.validate_loaded_module(&baseline)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&baseline.name);
        validate_verified_reload_candidate(&baseline, &name, &program, latest.as_ref())?;
        for (module, prepared) in program.modules().iter().zip(&bindings) {
            let current = self
                .link_native_module(module, &program)
                .map_err(ReloadValidationError::Runtime)?;
            if current.functions != prepared.functions || current.paths != prepared.paths {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::module_validation(
                        "host bindings changed after reload preparation",
                    ),
                ));
            }
        }
        if self
            .modules
            .has_abandoned_programs()
            .map_err(ReloadValidationError::Runtime)?
        {
            self.gc_safepoint()
                .map_err(ReloadValidationError::Runtime)?;
        }
        let epoch = self
            .modules
            .reserve_epoch(&name)
            .map_err(ReloadValidationError::Runtime)?;
        let program = self
            .modules
            .stage_verified_program(name, epoch, program, self.host.owner(), bindings)
            .map_err(ReloadValidationError::Runtime)?;
        Ok(StagedReload {
            initialization_error: Default::default(),
            baseline,
            program,
        })
    }

    pub fn publish_staged_reload(
        &self,
        candidate: StagedReload,
    ) -> Result<LoadedModule, ReloadValidationError> {
        if let Some(error) = candidate.initialization_error() {
            return Err(ReloadValidationError::Runtime(error));
        }
        let StagedReload {
            initialization_error: _,
            baseline,
            program,
        } = candidate;
        self.validate_loaded_module(&baseline)
            .map_err(ReloadValidationError::Runtime)?;
        self.validate_loaded_module(program.module())
            .map_err(ReloadValidationError::Runtime)?;
        if self
            .execution_root()
            .is_some_and(|root| root.program_root().key() == program.module().program_root().key())
        {
            return Err(ReloadValidationError::Runtime(
                RuntimeError::module_validation(
                    "candidate execution must finish before publication",
                ),
            ));
        }
        let latest = self.modules.latest(&baseline.name);
        if latest.as_ref().map(LoadedModule::key) != Some(baseline.key()) {
            return Err(ReloadValidationError::ModuleNotActive {
                module_name: baseline.name.clone(),
                expected: baseline.epoch,
                active: latest.map(|module| module.epoch),
            });
        }
        for member in program.module().members() {
            let Some(instance) = self.module_instance_snapshot(&member) else {
                return Err(ReloadValidationError::Runtime(self.resources().quarantine(
                    "candidate module instance disappeared at publication",
                )));
            };
            if !instance.module_slots.iter().all(|value| {
                self.gc
                    .validate_candidate_value_for(program.module().key(), value)
            }) {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::execution_phase_violation(
                        "external object in candidate module state at publication",
                    ),
                ));
            }
            let current = self
                .link_native_module(&member.bytecode, member.verified_program())
                .map_err(ReloadValidationError::Runtime)?;
            if current.functions != member.host_functions || current.paths != member.host_paths {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::module_validation(
                        "host bindings changed during candidate initialization",
                    ),
                ));
            }
        }
        let dependencies = program.module().verified_program().dependencies().clone();
        let module = program
            .publish(&self.modules)
            .map_err(ReloadValidationError::Runtime)?;
        self.invalidate_interpreter_caches_for_reload(&module, dependencies);
        Ok(module)
    }

    pub(super) fn invalidate_interpreter_caches_for_reload(
        &self,
        module: &LoadedModule,
        dependencies: ReloadDependencySnapshot,
    ) -> Vec<InterpreterCacheId> {
        let invalidated = self
            .interpreter_caches
            .invalidate_for_reload(&ReloadInvalidation {
                module_name: module.name.clone(),
                module_identity: module.bytecode.identity.clone(),
                module_fingerprint: ArtifactFingerprint::of_serialized(
                    &module
                        .verified_program()
                        .paths(module.bytecode.as_ref())
                        .expect("verified module identity scope"),
                ),
                module_id: module.id,
                published: module.key(),
                dependencies,
            });
        invalidated
            .into_iter()
            .map(|artifact| artifact.id)
            .collect()
    }
}
