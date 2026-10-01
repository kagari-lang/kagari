use crate::{
    PreparedReload, Runtime, StagedReload,
    cache::{InterpreterCacheId, ReloadDependencySnapshot, ReloadInvalidation},
    error::RuntimeError,
    module::{LinkedHostBindings, LoadedModule, VerifiedProgram},
    native::{NativeRegistration, registration::host_registration},
    reload::{
        ReloadValidationError, validate_reload_artifact_candidate, validate_reload_candidate,
        validate_verified_reload_candidate,
    },
};
use kagari_bytecode as bytecode;
use kagari_bytecode::{
    ArtifactCompatibility, ArtifactFingerprint, BytecodeModule, BytecodeProgram, KbcArtifact,
};

impl Runtime {
    pub fn register_native(
        &mut self,
        registration: NativeRegistration,
    ) -> Result<(), RuntimeError> {
        self.providers.install(registration)
    }
    fn link_native_module(
        &self,
        module: &BytecodeModule,
    ) -> Result<LinkedHostBindings, RuntimeError> {
        let mut bindings = self.host.link_module(module, &self.types)?;
        bindings.native = module
            .native_imports
            .iter()
            .map(|import| {
                if let Some(required) = &import.contract.host {
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
                    Ok(host_registration(import, binding))
                } else {
                    self.providers.link(import)
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(bindings)
    }

    pub fn validate_loaded_module(&self, module: &LoadedModule) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if !module.belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation(
                "loaded module belongs to another runtime or has been released",
            ));
        }
        let loaded = self.modules.try_loaded(module.key()).map_err(|_| {
            self.resources
                .quarantine("module store is borrowed across execution")
        })?;
        if loaded.is_none() {
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
        let name = name.into();
        let dependencies = program.dependencies().clone();
        let bindings = program
            .modules()
            .iter()
            .map(|module| self.link_native_module(module))
            .collect::<Result<Vec<_>, _>>()?;
        let epoch = self.epochs.reserve(&name)?;
        let module = self
            .modules
            .stage_verified_program(name, epoch, program, self.host.owner(), bindings)?
            .publish();
        self.invalidate_interpreter_caches_for_reload(&module, dependencies);
        Ok(module)
    }

    pub fn stage_reload_program(
        &mut self,
        active: &LoadedModule,
        name: impl Into<String>,
        bytecode: BytecodeProgram,
    ) -> Result<StagedReload, ReloadValidationError> {
        let name = name.into();
        bytecode::validate_program_resource_limits(&bytecode)
            .map_err(ReloadValidationError::Artifact)?;
        self.validate_loaded_module(active)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&active.name);
        validate_reload_candidate(active, &name, &bytecode, latest.as_ref())?;
        let program = VerifiedProgram::new(bytecode).map_err(ReloadValidationError::Runtime)?;
        self.stage_reload_verified_program(active, name, program)
    }

    pub fn stage_reload_artifact(
        &mut self,
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
        &mut self,
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
        self.validate_loaded_module(baseline)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&baseline.name);
        validate_verified_reload_candidate(baseline, &name, &program, latest.as_ref())?;
        let bindings = program
            .modules()
            .iter()
            .map(|module| self.link_native_module(module))
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
        &mut self,
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
                .link_native_module(module)
                .map_err(ReloadValidationError::Runtime)?;
            if current.functions != prepared.functions || current.paths != prepared.paths {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::module_validation(
                        "host bindings changed after reload preparation",
                    ),
                ));
            }
        }
        let epoch = self
            .epochs
            .reserve(&name)
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
        &mut self,
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
            let Some(instance) = self.modules.instance_snapshot(member.key()) else {
                return Err(ReloadValidationError::Runtime(self.resources.quarantine(
                    "candidate module instance disappeared at publication",
                )));
            };
            if !instance.module_slots.iter().all(|value| {
                self.gc
                    .validate_candidate_value_for(program.module().key(), value)
            }) {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::capability_denied(
                        "external object in candidate module state at publication",
                    ),
                ));
            }
            let current = self
                .link_native_module(&member.bytecode)
                .map_err(ReloadValidationError::Runtime)?;
            if current.functions != member.host_bindings.functions
                || current.paths != member.host_bindings.paths
            {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::module_validation(
                        "host bindings changed during candidate initialization",
                    ),
                ));
            }
        }
        let dependencies = program.module().verified_program().dependencies().clone();
        let module = program.publish();
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
                module_fingerprint: ArtifactFingerprint::of_serialized(module.bytecode.as_ref()),
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
