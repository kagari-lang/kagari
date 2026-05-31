pub mod builtin;
pub mod error;
pub mod gc;
pub mod host;
pub mod metadata;
pub mod module;
pub mod reflection;
pub mod reload;
pub mod resource;
pub mod security;
pub mod value;

use kagari_ir::bytecode::{ArtifactCompatibility, BuiltinMethod, BytecodeModule, KbcArtifact};

pub use error::{RuntimeError, RuntimeErrorKind};
pub use host::{
    BorrowEpoch, DynamicPathArgSlot, DynamicPathArgument, DynamicPathArguments,
    DynamicPathParameter, FrameHostBorrowToken, HostBorrowKind, HostBorrowTable, HostCallGuard,
    HostFrameId, HostFunctionEffects, HostFunctionId, HostFunctionMetadata, HostObjectId,
    HostPathAdapter, HostPathContext, HostPathDescriptor, HostPathDescriptorId,
    HostPathDescriptorRegistration, HostPathMutationRecord, HostPathOperation, HostPathSegment,
    HostPathViewHandle, HostReflectionPolicy, HostRootHandle, HostSchemaEpoch, HostTypeInfo,
    HostTypeOwnership, HostTypeRegistration,
};
pub use metadata::{
    AbiFingerprint, FieldInfo, FieldMetadataId, MethodInfo, MethodMetadataId, MethodOrigin,
    ParameterInfo, PathAccess, TraitInfo, TypeId, TypeInfo, TypeKind, TypeRegistration,
    TypeRegistry, VariantInfo, VariantMetadataId, Visibility,
};
pub use module::{
    LoadedModule, ModuleId, ModuleInitializationState, ModuleInstance, ModuleKey, ModuleStore,
};
pub use reload::ReloadValidationError;
pub use resource::{ResourceCounters, ResourcePolicy, ResourceState};
pub use security::{CapabilitySet, LanguageProfile, SecurityContext};

use crate::{
    builtin::BuiltinError,
    gc::{GcHeap, GcHeapConfig, GcRootId, HeapObjectId},
    host::{HostError, HostFunction, HostRegistry},
    reflection::ReflectionError,
    reload::{
        HotReloadCoordinator, validate_load_candidate, validate_reload_artifact_candidate,
        validate_reload_candidate,
    },
};

#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeConfig {
    pub gc: GcHeapConfig,
    pub security: SecurityContext,
    pub resources: ResourcePolicy,
}

#[derive(Debug)]
pub struct Runtime {
    gc: GcHeap,
    types: TypeRegistry,
    host: HostRegistry,
    host_borrows: HostBorrowTable,
    security: SecurityContext,
    resources: ResourceState,
    reloads: HotReloadCoordinator,
    modules: ModuleStore,
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        let mut gc_config = config.gc;
        gc_config.max_heap_units = gc_config.max_heap_units.or(config.resources.max_heap_units);
        Self {
            gc: GcHeap::new(gc_config),
            types: TypeRegistry::default(),
            host: HostRegistry::default(),
            host_borrows: HostBorrowTable::default(),
            security: config.security,
            resources: ResourceState::new(config.resources),
            reloads: HotReloadCoordinator::default(),
            modules: ModuleStore::default(),
        }
    }

    pub fn gc(&self) -> &GcHeap {
        &self.gc
    }

    pub fn host(&self) -> &HostRegistry {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut HostRegistry {
        &mut self.host
    }

    pub fn host_borrows(&self) -> &HostBorrowTable {
        &self.host_borrows
    }

    pub fn enter_host_call(&self) -> HostCallGuard<'_> {
        self.host_borrows.enter_frame()
    }

    pub fn register_host_function(
        &mut self,
        function: HostFunction,
    ) -> Result<HostFunctionId, RuntimeError> {
        self.host.register(function)
    }

    pub fn register_host_type(
        &mut self,
        registration: HostTypeRegistration,
    ) -> Result<TypeId, RuntimeError> {
        let type_id = self.types.register(registration.to_type_registration())?;
        self.host
            .register_type(HostTypeInfo::from_registration(type_id, registration))?;
        Ok(type_id)
    }

    pub fn register_host_root(
        &mut self,
        object_id: host::HostObjectId,
        type_id: TypeId,
        schema_epoch: host::HostSchemaEpoch,
    ) -> Result<host::HostRootHandle, RuntimeError> {
        self.host.register_root(object_id, type_id, schema_epoch)
    }

    pub fn register_host_path_descriptor(
        &mut self,
        registration: host::HostPathDescriptorRegistration,
    ) -> Result<host::HostPathDescriptorId, RuntimeError> {
        self.host.register_path_descriptor(registration)
    }

    pub fn register_host_path_adapter(
        &mut self,
        descriptor_id: host::HostPathDescriptorId,
        adapter: host::HostPathAdapter,
    ) -> Result<(), RuntimeError> {
        self.host.register_path_adapter(descriptor_id, adapter)
    }

    pub fn make_host_path_view(
        &self,
        root: host::HostRootHandle,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: host::DynamicPathArguments,
    ) -> Result<host::HostPathViewHandle, RuntimeError> {
        self.host.make_path_view(root, descriptor_id, dynamic_args)
    }

    pub fn make_host_path_view_from_value(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
    ) -> Result<host::HostPathViewHandle, RuntimeError> {
        self.validate_host_path_capabilities(descriptor_id)?;
        self.host
            .make_path_view_from_value(root_or_view, descriptor_id, dynamic_args)
    }

    pub fn read_host_path(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_host_path_capabilities(descriptor_id)?;
        self.host
            .read_path(root_or_view, descriptor_id, dynamic_args)
    }

    pub fn set_host_path(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
        value: value::Value,
    ) -> Result<(), RuntimeError> {
        self.validate_host_path_capabilities(descriptor_id)?;
        self.host
            .set_path(root_or_view, descriptor_id, dynamic_args, value)
    }

    pub fn modify_host_path(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
        op: kagari_ir::bytecode::BinaryOp,
        value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_host_path_capabilities(descriptor_id)?;
        self.host
            .modify_path(root_or_view, descriptor_id, dynamic_args, op, value)
    }

    pub fn host_dirty_paths(&self) -> Vec<host::HostPathMutationRecord> {
        self.host.dirty_paths()
    }

    pub fn clear_host_dirty_paths(&self) {
        self.host.clear_dirty_paths();
    }

    fn validate_host_path_capabilities(
        &self,
        descriptor_id: host::HostPathDescriptorId,
    ) -> Result<(), RuntimeError> {
        let Some(descriptor) = self.host.path_descriptor(descriptor_id) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor is not registered",
            ));
        };
        self.validate_capabilities(descriptor.capability_requirements)
    }

    fn validate_capabilities(&self, required: CapabilitySet) -> Result<(), RuntimeError> {
        let granted = self.security.capabilities;
        if required.fs_read && !granted.fs_read {
            return Err(RuntimeError::capability_denied("fs_read"));
        }
        if required.fs_write && !granted.fs_write {
            return Err(RuntimeError::capability_denied("fs_write"));
        }
        if required.net && !granted.net {
            return Err(RuntimeError::capability_denied("net"));
        }
        if required.clock && !granted.clock {
            return Err(RuntimeError::capability_denied("clock"));
        }
        if required.random && !granted.random {
            return Err(RuntimeError::capability_denied("random"));
        }
        if required.reflection_read && !granted.reflection_read {
            return Err(RuntimeError::capability_denied("reflection_read"));
        }
        if required.reflection_write && !granted.reflection_write {
            return Err(RuntimeError::capability_denied("reflection_write"));
        }
        if required.dynamic_load && !granted.dynamic_load {
            return Err(RuntimeError::capability_denied("dynamic_load"));
        }
        Ok(())
    }

    pub fn validate_host_function_boundary(&self, symbol: &str) -> Result<(), RuntimeError> {
        let Some(function) = self.host.function(symbol) else {
            return Ok(());
        };
        let metadata = function.metadata();
        self.validate_capabilities(metadata.capability_requirements)?;
        if let Some(cost) = metadata.resource_cost_hint {
            self.resources.consume_instruction_steps(cost)?;
        }
        Ok(())
    }

    pub fn validate_reflection_read_boundary(&self) -> Result<(), RuntimeError> {
        if self.security.allows_reflection_read() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_read"))
        }
    }

    pub fn validate_reflection_write_boundary(&self) -> Result<(), RuntimeError> {
        if self.security.allows_reflection_write() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_write"))
        }
    }

    pub fn types(&self) -> &TypeRegistry {
        &self.types
    }

    pub fn security(&self) -> SecurityContext {
        self.security
    }

    pub fn set_security_context(&mut self, security: SecurityContext) {
        self.security = security;
    }

    pub fn resources(&self) -> &ResourceState {
        &self.resources
    }

    pub fn modules(&self) -> &ModuleStore {
        &self.modules
    }

    pub fn module_instance_snapshot(&self, module: &LoadedModule) -> Option<ModuleInstance> {
        self.modules.instance_snapshot(module.key())
    }

    pub fn module_instance_mut(
        &self,
        module: &LoadedModule,
    ) -> Option<std::cell::RefMut<'_, ModuleInstance>> {
        self.modules.instance_mut(module.key())
    }

    pub fn root_value(&self, value: value::Value) -> Option<GcRootId> {
        self.gc.root_value(value)
    }

    pub fn root_snapshot(&self, id: GcRootId) -> Option<value::Value> {
        self.gc.root_snapshot(id)
    }

    pub fn update_root(&self, id: GcRootId, value: value::Value) -> Option<()> {
        self.gc.update_root(id, value)
    }

    pub fn release_root(&self, id: GcRootId) -> Option<value::Value> {
        self.gc.release_root(id)
    }

    pub fn trace_roots(&self) -> Vec<HeapObjectId> {
        self.gc.trace_roots()
    }

    pub fn consume_instruction_step(&self) -> Result<(), RuntimeError> {
        self.resources.consume_instruction_step()
    }

    pub fn enter_call(&self) -> Result<(), RuntimeError> {
        self.resources.enter_call()
    }

    pub fn leave_call(&self) {
        self.resources.leave_call();
    }

    pub fn sync_heap_accounting(&self) -> Result<(), RuntimeError> {
        let stats = self.gc.stats();
        self.resources
            .record_heap_units(stats.current_heap_units, stats.peak_heap_units)
    }

    pub fn invoke_host(
        &self,
        symbol: &str,
        args: &[value::Value],
    ) -> Result<value::Value, HostError> {
        self.host.invoke(symbol, args)
    }

    pub fn reflect_type_of(&self, value: &value::Value) -> value::Value {
        reflection::type_of(&self.gc, value)
    }

    pub fn reflect_get_field(
        &self,
        value: &value::Value,
        field_name: &str,
    ) -> Result<value::Value, ReflectionError> {
        reflection::get_field(&self.gc, value, field_name)
    }

    pub fn reflect_set_field(
        &self,
        value: &value::Value,
        field_name: &str,
        next_value: value::Value,
    ) -> Result<value::Value, ReflectionError> {
        reflection::set_field(&self.gc, value, field_name, next_value)
    }

    pub fn reflect_set_index(
        &self,
        value: &value::Value,
        index: &value::Value,
        next_value: value::Value,
    ) -> Result<value::Value, ReflectionError> {
        reflection::set_index(&self.gc, value, index, next_value)
    }

    pub fn invoke_builtin(
        &self,
        method: BuiltinMethod,
        args: &[value::Value],
    ) -> Result<value::Value, BuiltinError> {
        let value = builtin::invoke(&self.gc, method, args)?;
        let _ = self.sync_heap_accounting();
        Ok(value)
    }

    pub fn load_module(
        &mut self,
        name: impl Into<String>,
        bytecode: BytecodeModule,
    ) -> Result<LoadedModule, RuntimeError> {
        let name = name.into();
        validate_load_candidate(&bytecode).map_err(|error| match error {
            ReloadValidationError::Bytecode(error) => {
                RuntimeError::module_validation(format!("bytecode validation failed: {error:?}"))
            }
            error => RuntimeError::module_validation(error.to_string()),
        })?;
        self.resources
            .record_loaded_modules(self.modules.loaded_count() + 1)?;
        let epoch = self.reloads.publish(&name);
        let module = self.modules.load(name, epoch, bytecode);
        self.resources
            .record_loaded_modules(self.modules.loaded_count())?;
        Ok(module)
    }

    pub fn reload_module(
        &mut self,
        active: &LoadedModule,
        name: impl Into<String>,
        bytecode: BytecodeModule,
    ) -> Result<LoadedModule, ReloadValidationError> {
        let name = name.into();
        let latest = self.modules.latest(&active.name);
        validate_reload_candidate(active, &name, &bytecode, latest.as_ref())?;
        self.publish_validated_reload(name, bytecode)
    }

    pub fn reload_artifact(
        &mut self,
        active: &LoadedModule,
        name: impl Into<String>,
        artifact: KbcArtifact,
        compatibility: &ArtifactCompatibility,
    ) -> Result<LoadedModule, ReloadValidationError> {
        let name = name.into();
        let latest = self.modules.latest(&active.name);
        validate_reload_artifact_candidate(
            active,
            &name,
            &artifact,
            compatibility,
            latest.as_ref(),
        )?;
        self.publish_validated_reload(name, artifact.module)
    }

    fn publish_validated_reload(
        &mut self,
        name: String,
        bytecode: BytecodeModule,
    ) -> Result<LoadedModule, ReloadValidationError> {
        self.resources
            .record_loaded_modules(self.modules.loaded_count() + 1)
            .map_err(ReloadValidationError::Runtime)?;
        let epoch = self.reloads.publish(&name);
        let module = self.modules.load(name, epoch, bytecode);
        self.resources
            .record_loaded_modules(self.modules.loaded_count())
            .map_err(ReloadValidationError::Runtime)?;
        Ok(module)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_ir::{
        bytecode::{
            ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, BytecodeModule,
            DependencyFingerprint, KbcArtifact,
        },
        module::{FunctionAbi, PublicAbiItem},
    };

    fn module_with_public_function(return_type: &str) -> BytecodeModule {
        BytecodeModule {
            public_items: vec![PublicAbiItem::Function(FunctionAbi {
                name: "main".to_owned(),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                params: Vec::new(),
                return_type: return_type.to_owned(),
            })],
            ..BytecodeModule::default()
        }
    }

    fn artifact_with_loader_fingerprints() -> KbcArtifact {
        KbcArtifact::from_module(
            module_with_public_function("i32"),
            ArtifactBuildOptions {
                dependency_fingerprints: vec![DependencyFingerprint {
                    module_id: "pkg/dependency".to_owned(),
                    fingerprint: ArtifactFingerprint::of_str("dependency-v1"),
                }],
                host_registry_fingerprint: ArtifactFingerprint::of_str("host-v1"),
                security_profile: Some("dev".to_owned()),
                ..ArtifactBuildOptions::default()
            },
        )
    }

    fn compatibility_for_artifact(artifact: &KbcArtifact) -> ArtifactCompatibility {
        ArtifactCompatibility {
            module_identity: Some(artifact.header.module_identity.clone()),
            dependency_fingerprints: artifact.verification.loader.dependency_fingerprints.clone(),
            host_registry_fingerprint: artifact.verification.loader.host_registry_fingerprint,
            security_profile: artifact.verification.loader.security_profile.clone(),
            ..ArtifactCompatibility::default()
        }
    }

    #[test]
    fn load_module_reports_module_resource_limit() {
        let mut runtime = Runtime::new(RuntimeConfig {
            resources: ResourcePolicy {
                max_modules: Some(1),
                ..ResourcePolicy::default()
            },
            ..RuntimeConfig::default()
        });

        runtime
            .load_module("first", BytecodeModule::default())
            .unwrap();
        let error = runtime
            .load_module("second", BytecodeModule::default())
            .unwrap_err();

        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert_eq!(runtime.resources().counters().loaded_modules, 1);
    }

    #[test]
    fn reload_publishes_valid_candidate_after_validation() {
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_module("reloadable", module_with_public_function("i32"))
            .expect("module should load");

        let reloaded = runtime
            .reload_module(&loaded, "reloadable", module_with_public_function("i32"))
            .expect("compatible module should reload");

        assert_eq!(reloaded.id, loaded.id);
        assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().epoch,
            reloaded.epoch
        );
    }

    #[test]
    fn reload_rejects_public_abi_changes_before_publication() {
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_module("reloadable", module_with_public_function("i32"))
            .expect("module should load");
        let before_count = runtime.modules().loaded_count();

        let error = runtime
            .reload_module(&loaded, "reloadable", module_with_public_function("String"))
            .expect_err("public ABI change should reject reload");

        assert!(matches!(
            error,
            ReloadValidationError::PublicAbiFingerprintMismatch
        ));
        assert_eq!(runtime.modules().loaded_count(), before_count);
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().epoch,
            loaded.epoch
        );
    }

    #[test]
    fn reload_rejects_stale_active_epoch_before_publication() {
        let mut runtime = Runtime::default();
        let first = runtime
            .load_module("reloadable", module_with_public_function("i32"))
            .expect("module should load");
        let second = runtime
            .reload_module(&first, "reloadable", module_with_public_function("i32"))
            .expect("compatible module should reload");
        let before_count = runtime.modules().loaded_count();

        let error = runtime
            .reload_module(&first, "reloadable", module_with_public_function("i32"))
            .expect_err("stale active epoch should reject reload");

        assert!(matches!(
            error,
            ReloadValidationError::ModuleNotActive {
                expected,
                active: Some(active),
                ..
            } if expected == first.epoch && active == second.epoch
        ));
        assert_eq!(runtime.modules().loaded_count(), before_count);
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().epoch,
            second.epoch
        );
    }

    #[test]
    fn reload_resource_failure_preserves_active_epoch() {
        let mut runtime = Runtime::new(RuntimeConfig {
            resources: ResourcePolicy {
                max_modules: Some(1),
                ..ResourcePolicy::default()
            },
            ..RuntimeConfig::default()
        });
        let loaded = runtime
            .load_module("reloadable", module_with_public_function("i32"))
            .expect("module should load");

        let error = runtime
            .reload_module(&loaded, "reloadable", module_with_public_function("i32"))
            .expect_err("resource limit should reject reload before publication");

        assert!(matches!(
            error,
            ReloadValidationError::Runtime(ref error)
                if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
        ));
        assert_eq!(runtime.modules().loaded_count(), 1);
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().epoch,
            loaded.epoch
        );
    }

    #[test]
    fn reload_artifact_validates_loader_compatibility_before_publication() {
        let artifact = artifact_with_loader_fingerprints();
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_module("reloadable", artifact.module.clone())
            .expect("module should load");
        let before_count = runtime.modules().loaded_count();

        let error = runtime
            .reload_artifact(
                &loaded,
                "reloadable",
                artifact,
                &ArtifactCompatibility::default(),
            )
            .expect_err("loader compatibility mismatch should reject reload");

        assert!(matches!(
            error,
            ReloadValidationError::Artifact(
                kagari_ir::bytecode::ArtifactValidationError::DependencyFingerprintMismatch
            )
        ));
        assert_eq!(runtime.modules().loaded_count(), before_count);
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().epoch,
            loaded.epoch
        );
    }

    #[test]
    fn reload_artifact_publishes_after_loader_and_reload_validation() {
        let artifact = artifact_with_loader_fingerprints();
        let compatibility = compatibility_for_artifact(&artifact);
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_module("reloadable", artifact.module.clone())
            .expect("module should load");

        let reloaded = runtime
            .reload_artifact(&loaded, "reloadable", artifact, &compatibility)
            .expect("compatible artifact should reload");

        assert_eq!(reloaded.id, loaded.id);
        assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().epoch,
            reloaded.epoch
        );
    }

    #[test]
    fn syncs_heap_accounting_into_runtime_resource_counters() {
        let runtime = Runtime::default();
        let array = runtime
            .gc()
            .alloc_array(vec![value::Value::I32(1)])
            .unwrap();
        runtime
            .gc()
            .array_push(array, value::Value::I32(2))
            .unwrap();

        runtime.sync_heap_accounting().unwrap();
        let counters = runtime.resources().counters();

        assert_eq!(counters.current_heap_units, 3);
        assert_eq!(counters.peak_heap_units, 3);
    }

    #[test]
    fn exposes_runtime_type_registry_and_security_context() {
        let runtime = Runtime::new(RuntimeConfig {
            security: SecurityContext {
                capabilities: CapabilitySet {
                    reflection_read: true,
                    ..CapabilitySet::default()
                },
                profile: LanguageProfile {
                    allow_reflection: true,
                    ..LanguageProfile::default()
                },
            },
            ..RuntimeConfig::default()
        });
        let type_id = runtime
            .types()
            .register(TypeRegistration {
                abi_fingerprint: AbiFingerprint(99),
                ..TypeRegistration::new("Player", TypeKind::Struct)
            })
            .unwrap();

        assert_eq!(runtime.types().id_by_name("Player"), Some(type_id));
        assert!(runtime.security().allows_reflection_read());
    }
}
