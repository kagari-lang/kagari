#[cfg(test)]
extern crate self as kagari_runtime;
pub mod backend;
pub mod builtin;
pub mod cache;
pub mod error;
mod execution_state;
mod frame;
pub mod gc;
pub mod host;
mod host_scope;
pub mod jit_abi;
#[cfg(test)]
#[path = "../tests/support/layouts.rs"]
mod layout_fixtures;
pub mod metadata;
pub mod module;
pub mod numeric;
pub mod reflection;
pub mod reload;
pub mod resource;
pub mod security;
pub mod session;
pub mod value;
pub mod value_semantics;

use kagari_ir::{
    builtin::surface::StandardIntrinsic,
    bytecode::{ArtifactCompatibility, BytecodeProgram, KbcArtifact},
};

pub use backend::{
    BackendCompileError, BackendDiagnostic, BackendDiagnosticKind, BackendFunctionInput, BackendId,
    BackendInvocationError, BackendTarget, CodegenBackend, ExecutableDebugInfo,
    ExecutableDebugPoint, ExecutableEntryPoint, ExecutableFunctionArtifact, ExecutableSafepoint,
    ExecutableSafepointKind, ExecutableStackMap, ExecutableStackMapLocation,
    ExecutableStackMapSlot, ExecutableStackValueKind, ExecutableTrap,
};
pub use cache::{
    ExecutionArtifactId, ExecutionArtifactKind, ExecutionArtifactRecord, ExecutionArtifactRegistry,
    ReloadDependencySnapshot, ReloadInvalidation,
};
pub use error::{RuntimeError, RuntimeErrorKind};
pub use frame::{ExecutionFrame, ExecutionStack};
pub use host::{
    BorrowEpoch, DynamicPathArgSlot, DynamicPathArgument, DynamicPathArguments,
    DynamicPathParameter, FrameHostBorrowToken, HostBorrowKind, HostBorrowTable, HostCallGuard,
    HostFrameId, HostFunctionDeclaration, HostFunctionEffects, HostFunctionId, HostObjectId,
    HostPathAdapter, HostPathContext, HostPathDescriptor, HostPathDescriptorId,
    HostPathDescriptorRegistration, HostPathMutationRecord, HostPathOperation, HostPathSegment,
    HostPathSegmentRegistration, HostPathViewHandle, HostReflectionPolicy, HostRootHandle,
    HostSchemaEpoch, HostTypeInfo, HostTypeOwnership, HostTypeRegistration,
};
pub use host_scope::HostResourceScope;
pub use metadata::{
    AbiFingerprint, FieldInfo, FieldMetadataId, MethodInfo, MethodMetadataId, MethodOrigin,
    ParameterInfo, PathAccess, TraitInfo, TypeId, TypeInfo, TypeKind, TypeRegistration,
    TypeRegistry, VariantInfo, VariantMetadataId, Visibility,
};
pub use module::{
    LoadedModule, ModuleEpochRetention, ModuleEpochRetentionCounts, ModuleId,
    ModuleInitializationState, ModuleInstance, ModuleKey, ModuleStore,
};
pub use reload::ReloadValidationError;
pub use resource::{ResourceCounters, ResourcePolicy, ResourceState};
pub use security::{
    CapabilitySet, DebugVisibilityPolicy, HostExposurePolicy, LanguageProfile, SecurityContext,
};
pub use session::{
    ExecutionCounters, ExecutionEvent, ExecutionObserver, ExecutionOptions, ExecutionSession,
};

use crate::{
    builtin::BuiltinError,
    gc::{GcCollection, GcHeap, GcHeapConfig, HeapObjectId, RootedValue},
    host::{HostFunction, HostRegistry},
    reload::{HotReloadCoordinator, validate_reload_artifact_candidate, validate_reload_candidate},
};
use value::Value;

/// Verified and linked reload data that has not changed any runtime entry.
struct PreparedReload {
    baseline: LoadedModule,
    name: String,
    bytecode: BytecodeProgram,
    bindings: Vec<module::LinkedHostBindings>,
    dependencies: ReloadDependencySnapshot,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    pub gc: GcHeapConfig,
    pub security: SecurityContext,
    pub host_exposure: HostExposurePolicy,
    pub debug_visibility: DebugVisibilityPolicy,
    pub resources: ResourcePolicy,
}

/// Owns initialization state and version retention without holding a store borrow.
/// Dropping an unfinished initialization records failure, even after quarantine.
#[must_use]
pub struct ModuleInitializationGuard<'a> {
    runtime: &'a Runtime,
    module: LoadedModule,
    finished: bool,
}

impl ModuleInitializationGuard<'_> {
    pub fn finish(mut self, value: Value) -> Result<Value, RuntimeError> {
        self.runtime
            .validate_heap_payloads(std::slice::from_ref(&value))?;
        let result = value.clone();
        let mut instance = self
            .runtime
            .modules
            .instance_mut(self.module.key())
            .ok_or_else(|| {
                self.runtime
                    .resources
                    .quarantine("initializing module instance disappeared")
            })?;
        if instance.state != ModuleInitializationState::Initializing {
            return Err(self
                .runtime
                .resources
                .quarantine("module initialization state changed before completion"));
        }
        instance.finish_initialization(result);
        self.finished = true;
        Ok(value)
    }
}

impl Drop for ModuleInitializationGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            // Failure cleanup is permitted after execution has been disabled.
            let _ = self.runtime.fail_module_initialization(&self.module);
        }
        self.runtime
            .modules
            .release_epoch(self.module.key(), ModuleEpochRetention::ActiveCall);
    }
}

#[derive(Debug)]
pub struct Runtime {
    gc: std::rc::Rc<GcHeap>,
    types: TypeRegistry,
    host: HostRegistry,
    host_borrows: HostBorrowTable,
    security: SecurityContext,
    host_exposure: std::rc::Rc<HostExposurePolicy>,
    debug_visibility: DebugVisibilityPolicy,
    resources: std::rc::Rc<ResourceState>,
    reloads: HotReloadCoordinator,
    modules: ModuleStore,
    execution_artifacts: ExecutionArtifactRegistry,
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        let resources = std::rc::Rc::new(ResourceState::new(config.resources));
        Self {
            gc: std::rc::Rc::new(GcHeap::new(config.gc, resources.clone())),
            types: TypeRegistry::default(),
            host: HostRegistry::default(),
            host_borrows: HostBorrowTable::with_resources(&resources),
            security: config.security,
            host_exposure: std::rc::Rc::new(config.host_exposure),
            debug_visibility: config.debug_visibility,
            resources,
            reloads: HotReloadCoordinator::default(),
            modules: ModuleStore::default(),
            execution_artifacts: ExecutionArtifactRegistry::default(),
        }
    }

    pub fn is_quarantined(&self) -> bool {
        self.resources.is_quarantined()
    }

    pub fn execution_root(&self) -> Option<LoadedModule> {
        self.resources
            .active_session()
            .map(|session| session.root.clone())
    }

    /// Install once for the root call. Nested drivers inherit the same observer.
    pub fn attach_execution_observer(
        &self,
        observer: std::rc::Rc<dyn ExecutionObserver>,
    ) -> Result<bool, RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let session = self.resources.active_session().ok_or_else(|| {
            RuntimeError::module_validation("execution observer requires an active session")
        })?;
        let mut active = session.observer.borrow_mut();
        if let Some(existing) = active.as_ref() {
            if !std::rc::Rc::ptr_eq(existing, &observer) {
                return Err(RuntimeError::module_validation(
                    "nested execution cannot replace the root observer",
                ));
            }
            return Ok(false);
        }
        if !session
            .frames
            .try_borrow()
            .map_err(|_| {
                self.resources
                    .quarantine("observer installation encountered a borrowed stack")
            })?
            .is_empty()
        {
            return Err(RuntimeError::module_validation(
                "cannot attach an observer during frame execution",
            ));
        }
        *active = Some(observer);
        Ok(true)
    }

    pub fn observe_execution(&self, event: ExecutionEvent) -> Result<(), RuntimeError> {
        let Some(session) = self.resources.active_session() else {
            return Ok(());
        };
        let Some(observer) = session.observer.borrow().clone() else {
            return Ok(());
        };
        let frames = session.frames.try_borrow().map_err(|_| {
            self.resources
                .quarantine("observer encountered a borrowed execution stack")
        })?;
        let result = observer.observe(self, event, &frames);
        if result
            .as_ref()
            .is_err_and(|error| error.kind() == RuntimeErrorKind::EngineFault)
        {
            self.resources
                .quarantine("execution observer encountered an engine fault");
        }
        result
    }

    pub fn enter_execution_stack(
        &self,
        module: &LoadedModule,
    ) -> Result<ExecutionStack, RuntimeError> {
        let session = self.begin_execution(module, self.execution_options())?;
        ExecutionStack::new(session, self.gc.clone())
    }

    pub fn execution_options(&self) -> ExecutionOptions {
        if let Some(session) = self.resources.active_session() {
            return session.options.clone();
        }
        ExecutionOptions {
            security: self.security,
            host_exposure: self.host_exposure.clone(),
            resources: self.resources.policy(),
            cancellation: Default::default(),
        }
    }

    pub fn begin_execution(
        &self,
        module: &LoadedModule,
        options: ExecutionOptions,
    ) -> Result<ExecutionSession, RuntimeError> {
        self.validate_loaded_module(module)?;
        let state = if let Some(session) = self.resources.active_session() {
            if !session
                .root
                .members()
                .any(|member| member.key() == module.key())
            {
                return Err(RuntimeError::module_validation(
                    "nested execution must use the pinned dependency program",
                ));
            }
            session
        } else {
            let session = std::rc::Rc::new(session::SessionState::new(
                module.clone(),
                options,
                self.resources.counters(),
            ));
            self.modules
                .retain_epoch(module.key(), ModuleEpochRetention::ActiveCall);
            self.resources.start_execution(session.clone());
            session
        };
        let scopes = state
            .scopes
            .get()
            .checked_add(1)
            .ok_or_else(|| self.resources.quarantine("execution scope count overflow"))?;
        state.scopes.set(scopes);
        let guard = ExecutionSession {
            state,
            resources: self.resources.clone(),
            modules: self.modules.clone(),
        };
        self.resources.poll_execution()?;
        Ok(guard)
    }

    pub fn gc(&self) -> &GcHeap {
        &self.gc
    }

    pub fn alloc_array(&self, elements: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&elements)?;
        self.gc.alloc_array(elements)
    }

    pub fn alloc_map(&self, entries: Vec<(Value, Value)>) -> Result<HeapObjectId, RuntimeError> {
        for (key, value) in &entries {
            self.validate_heap_payloads(std::slice::from_ref(key))?;
            self.validate_heap_payloads(std::slice::from_ref(value))?;
        }
        self.gc.alloc_map(entries)
    }

    pub fn alloc_set(&self, values: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&values)?;
        self.gc.alloc_set(values)
    }

    pub fn alloc_struct(
        &self,
        layout: module::StructLayoutRef,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&fields)?;
        // The layout retains a verified generation; allocation does not depend on
        // whether that generation remains a current module-store entry.
        if !layout.module().belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation(
                "struct layout belongs to a different runtime",
            ));
        }
        if fields.len() != layout.layout().fields.len()
            || !fields
                .iter()
                .zip(&layout.layout().fields)
                .all(|(value, field)| {
                    value.is_default_heap_payload() && value.has_representation(field.ty)
                })
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "struct initializer does not match its layout",
            ));
        }
        self.gc.alloc_struct(layout, fields)
    }

    pub fn alloc_enum(
        &self,
        tag: value::EnumTag,
        fields: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.validate_heap_payloads(&fields)?;
        if let value::EnumTag::Declared(layout) = &tag
            && !layout.module().belongs_to(self.host.owner())
        {
            return Err(RuntimeError::module_validation(
                "enum layout belongs to a different runtime",
            ));
        }
        self.gc.alloc_enum(tag, fields)
    }

    pub fn host(&self) -> &HostRegistry {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut HostRegistry {
        &mut self.host
    }

    pub fn validate_host_borrow(
        &self,
        token: FrameHostBorrowToken,
        required: HostBorrowKind,
    ) -> Result<(), RuntimeError> {
        self.host_borrows.validate(token, required)
    }

    pub fn host_scope(&self, values: &[Value]) -> Result<HostResourceScope<'_>, RuntimeError> {
        HostResourceScope::new(self, values)
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
        Ok(self.register_host_types(vec![registration])?[0])
    }

    pub fn register_host_types(
        &mut self,
        registrations: Vec<HostTypeRegistration>,
    ) -> Result<Vec<TypeId>, RuntimeError> {
        let bindings = self.types.register_host_types(&registrations, &self.host)?;
        let ids = bindings.iter().map(|binding| binding.type_id).collect();
        self.host.install_types(bindings);
        Ok(ids)
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
        self.host
            .register_path_descriptor(registration, &self.types)
    }

    pub fn register_host_field_path(
        &mut self,
        declaration: &kagari_common::host_interface::HostFieldPathDeclaration,
    ) -> Result<host::HostPathDescriptorId, RuntimeError> {
        self.host.register_field_path(declaration, &self.types)
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
        self.validate_host_path_exposure(descriptor_id, host::HostPathOperation::MakeView)?;
        if !dynamic_args
            .as_slice()
            .iter()
            .all(|arg| self.gc.validate_value(&arg.value))
        {
            return Err(RuntimeError::typed_path_validation(
                "invalid heap reference in path arguments",
            ));
        }
        self.host.make_path_view(root, descriptor_id, dynamic_args)
    }

    pub fn make_host_path_view_from_value(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
    ) -> Result<host::HostPathViewHandle, RuntimeError> {
        self.validate_host_path_exposure(descriptor_id, host::HostPathOperation::MakeView)?;
        self.validate_host_path_capabilities(descriptor_id)?;
        if !self.gc.validate_value(root_or_view)
            || !dynamic_args.iter().all(|arg| self.gc.validate_value(arg))
        {
            return Err(RuntimeError::typed_path_validation(
                "invalid heap reference in path arguments",
            ));
        }
        self.host
            .make_path_view_from_value(root_or_view, descriptor_id, dynamic_args)
    }

    pub fn read_host_path(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_host_path_exposure(descriptor_id, host::HostPathOperation::Read)?;
        self.validate_host_path_capabilities(descriptor_id)?;
        let result = self
            .host
            .read_path(self, root_or_view, descriptor_id, dynamic_args);
        self.resources.ensure_execution_allowed()?;
        result
    }

    pub fn set_host_path(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
        value: value::Value,
    ) -> Result<(), RuntimeError> {
        self.validate_host_path_exposure(descriptor_id, host::HostPathOperation::Set)?;
        self.validate_path_mutation_boundary()?;
        self.validate_host_path_capabilities(descriptor_id)?;
        let result = self
            .host
            .set_path(self, root_or_view, descriptor_id, dynamic_args, value);
        self.resources.ensure_execution_allowed()?;
        result
    }

    pub fn modify_host_path(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
        op: kagari_ir::bytecode::BinaryOp,
        value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_host_path_exposure(descriptor_id, host::HostPathOperation::Modify(op))?;
        self.validate_path_mutation_boundary()?;
        self.validate_host_path_capabilities(descriptor_id)?;
        let result =
            self.host
                .modify_path(self, root_or_view, descriptor_id, dynamic_args, op, value);
        self.resources.ensure_execution_allowed()?;
        result
    }

    pub fn host_dirty_paths(&self) -> Vec<host::HostPathMutationRecord> {
        self.host.dirty_paths()
    }

    pub fn clear_host_dirty_paths(&self) {
        self.host.clear_dirty_paths();
    }

    fn validate_host_path_exposure(
        &self,
        descriptor_id: host::HostPathDescriptorId,
        operation: host::HostPathOperation,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let Some(descriptor) = self.host.path_descriptor(descriptor_id) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor is not registered",
            ));
        };
        let Some(root_type) = self.host.host_type(descriptor.root_type) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type is not registered",
            ));
        };
        if !self
            .host_exposure()
            .exposes_host_type(&root_type.declaration.symbol)
        {
            return Err(RuntimeError::capability_denied(format!(
                "host type `{}`",
                root_type.declaration.symbol
            )));
        }
        if operation.writes() {
            if self.host_exposure().exposes_host_path_mutation() {
                Ok(())
            } else {
                Err(RuntimeError::capability_denied("host path mutation"))
            }
        } else if self.host_exposure().exposes_host_path_read() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("host path read"))
        }
    }

    fn validate_host_path_capabilities(
        &self,
        descriptor_id: host::HostPathDescriptorId,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let Some(descriptor) = self.host.path_descriptor(descriptor_id) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor is not registered",
            ));
        };
        self.validate_capabilities(descriptor.capability_requirements)
    }

    fn validate_capabilities(&self, required: CapabilitySet) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let granted = self.security().capabilities;
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
        if required.host_calls && !self.security().allows_host_calls() {
            return Err(RuntimeError::capability_denied("host_calls"));
        }
        if required.path_mutation && !self.security().allows_path_mutation() {
            return Err(RuntimeError::capability_denied("path_mutation"));
        }
        if required.reflection_metadata && !self.security().allows_reflection_metadata() {
            return Err(RuntimeError::capability_denied("reflection_metadata"));
        }
        if required.reflection_read && !self.security().allows_reflection_read() {
            return Err(RuntimeError::capability_denied("reflection_read"));
        }
        if required.reflection_write && !self.security().allows_reflection_write() {
            return Err(RuntimeError::capability_denied("reflection_write"));
        }
        if required.dynamic_invocation && !self.security().allows_dynamic_invocation() {
            return Err(RuntimeError::capability_denied("dynamic_invocation"));
        }
        if required.downcast && !self.security().allows_downcast() {
            return Err(RuntimeError::capability_denied("downcast"));
        }
        if required.module_loading && !self.security().allows_module_loading() {
            return Err(RuntimeError::capability_denied("module_loading"));
        }
        if required.jit && !self.security().allows_jit() {
            return Err(RuntimeError::capability_denied("jit"));
        }
        if required.debug_attach && !self.security().allows_debug_attach() {
            return Err(RuntimeError::capability_denied("debug_attach"));
        }
        if required.debug_breakpoints && !self.security().allows_debug_breakpoints() {
            return Err(RuntimeError::capability_denied("debug_breakpoints"));
        }
        if required.debug_pause && !self.security().allows_debug_pause() {
            return Err(RuntimeError::capability_denied("debug_pause"));
        }
        if required.debug_stack_inspection && !self.security().allows_debug_stack_inspection() {
            return Err(RuntimeError::capability_denied("debug_stack_inspection"));
        }
        if required.debug_value_inspection && !self.security().allows_debug_value_inspection() {
            return Err(RuntimeError::capability_denied("debug_value_inspection"));
        }
        if required.debug_host_value_inspection
            && !self.security().allows_debug_host_value_inspection()
        {
            return Err(RuntimeError::capability_denied(
                "debug_host_value_inspection",
            ));
        }
        if required.debug_watch_evaluation && !self.security().allows_debug_watch_evaluation() {
            return Err(RuntimeError::capability_denied("debug_watch_evaluation"));
        }
        if required.debug_side_effecting_evaluation
            && !self.security().allows_debug_side_effecting_evaluation()
        {
            return Err(RuntimeError::capability_denied(
                "debug_side_effecting_evaluation",
            ));
        }
        Ok(())
    }

    pub fn validate_host_function_boundary(&self, symbol: &str) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let function = self.host.function(symbol);
        self.validate_bound_host_boundary(symbol, function)
    }

    fn validate_bound_host_boundary(
        &self,
        symbol: &str,
        function: Option<&HostFunction>,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if !self.host_exposure().exposes_host_function(symbol) {
            return Err(RuntimeError::capability_denied(format!(
                "host function `{symbol}`"
            )));
        }
        if !self.security().allows_host_calls() {
            return Err(RuntimeError::capability_denied("host_calls"));
        }
        let Some(function) = function else {
            return Ok(());
        };
        let metadata = function.declaration();
        self.validate_capabilities(metadata.capability_requirements)?;
        self.resources.consume_host_call()?;
        if let Some(cost) = metadata.resource_cost_hint {
            self.resources.consume_instruction_steps(cost)?;
        }
        Ok(())
    }

    pub fn validate_reflection_metadata_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_reflection_metadata() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_metadata"))
        }
    }

    pub fn validate_reflection_read_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_reflection_read() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_read"))
        }
    }

    pub fn validate_reflection_write_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_reflection_write() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_write"))
        }
    }

    pub fn validate_dynamic_invocation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_dynamic_invocation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("dynamic_invocation"))
        }
    }

    pub fn validate_downcast_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_downcast() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("downcast"))
        }
    }

    pub fn validate_path_mutation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_path_mutation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("path_mutation"))
        }
    }

    pub fn validate_module_loading_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_module_loading() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("module_loading"))
        }
    }

    pub fn validate_jit_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_jit() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("jit"))
        }
    }

    pub fn validate_debug_attach_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_attach() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_attach"))
        }
    }

    pub fn validate_debug_breakpoint_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_breakpoints() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_breakpoints"))
        }
    }

    pub fn validate_debug_pause_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_pause() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_pause"))
        }
    }

    pub fn validate_debug_stack_inspection_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_stack_inspection() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_stack_inspection"))
        }
    }

    pub fn validate_debug_value_inspection_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_value_inspection() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_value_inspection"))
        }
    }

    pub fn validate_debug_host_value_inspection_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_host_value_inspection()
            && self.debug_visibility.exposes_host_values()
        {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied(
                "debug_host_value_inspection",
            ))
        }
    }

    pub fn validate_debug_watch_evaluation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_watch_evaluation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_watch_evaluation"))
        }
    }

    pub fn validate_debug_side_effecting_evaluation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_side_effecting_evaluation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied(
                "debug_side_effecting_evaluation",
            ))
        }
    }

    pub fn validate_debug_module_visible(&self, module_name: &str) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.debug_visibility.exposes_module(module_name) {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied(format!(
                "debug module `{module_name}`"
            )))
        }
    }

    pub fn validate_debug_value_visible(&self, value: &value::Value) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.validate_debug_value_inspection_boundary()?;
        if !self.gc.validate_value(value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap reference in debug value",
            ));
        }
        if value_contains_host_owned_data(value) {
            self.validate_debug_host_value_inspection_boundary()?;
        }
        Ok(())
    }

    pub fn types(&self) -> &TypeRegistry {
        &self.types
    }

    pub fn security(&self) -> SecurityContext {
        self.resources
            .active_session()
            .map_or(self.security, |session| session.options.security)
    }

    pub fn set_security_context(&mut self, security: SecurityContext) {
        self.security = security;
    }

    pub fn host_exposure(&self) -> std::rc::Rc<HostExposurePolicy> {
        self.resources.active_session().map_or_else(
            || self.host_exposure.clone(),
            |session| session.options.host_exposure.clone(),
        )
    }

    pub fn set_host_exposure_policy(&mut self, policy: HostExposurePolicy) {
        self.host_exposure = std::rc::Rc::new(policy);
    }

    pub fn debug_visibility(&self) -> &DebugVisibilityPolicy {
        &self.debug_visibility
    }

    pub fn set_debug_visibility_policy(&mut self, policy: DebugVisibilityPolicy) {
        self.debug_visibility = policy;
    }

    pub fn resources(&self) -> &ResourceState {
        &self.resources
    }

    pub fn modules(&self) -> &ModuleStore {
        &self.modules
    }

    pub fn register_execution_artifact(
        &self,
        kind: ExecutionArtifactKind,
        module: ModuleKey,
        function: Option<kagari_ir::bytecode::FunctionRef>,
        dependencies: ReloadDependencySnapshot,
    ) -> Option<ExecutionArtifactId> {
        self.modules.loaded(module)?;
        if kind == ExecutionArtifactKind::Jit {
            self.modules
                .retain_epoch(module, ModuleEpochRetention::CompiledArtifact);
        }
        Some(
            self.execution_artifacts
                .register(kind, module, function, dependencies),
        )
    }

    pub fn register_executable_function_artifact(
        &self,
        module: ModuleKey,
        dependencies: ReloadDependencySnapshot,
        artifact: ExecutableFunctionArtifact,
    ) -> Option<ExecutionArtifactId> {
        let loaded = self.modules.loaded(module)?;
        loaded
            .bytecode
            .functions
            .iter()
            .any(|function| function.id == artifact.function)
            .then_some(())?;
        self.modules
            .retain_epoch(module, ModuleEpochRetention::CompiledArtifact);
        Some(
            self.execution_artifacts
                .register_executable_function(module, dependencies, artifact),
        )
    }

    pub fn execution_artifact(&self, id: ExecutionArtifactId) -> Option<ExecutionArtifactRecord> {
        let artifact = self.execution_artifacts.get(id)?;
        if !artifact.valid || !self.modules.is_reachable(artifact.module) {
            return None;
        }
        Some(artifact)
    }

    pub fn module_instance_snapshot(&self, module: &LoadedModule) -> Option<ModuleInstance> {
        self.validate_loaded_module(module).ok()?;
        self.modules.instance_snapshot(module.key())
    }

    pub fn begin_module_initialization(
        &self,
        module: &LoadedModule,
    ) -> Result<ModuleInitializationGuard<'_>, RuntimeError> {
        self.validate_loaded_module(module)?;
        {
            let mut instance = self.modules.instance_mut(module.key()).ok_or_else(|| {
                self.resources
                    .quarantine("loaded module instance disappeared")
            })?;
            if instance.state != ModuleInitializationState::Uninitialized {
                return Err(RuntimeError::module_validation(
                    "module initialization can only begin once",
                ));
            }
            instance.begin_initialization();
        }
        self.modules
            .retain_epoch(module.key(), ModuleEpochRetention::ActiveCall);
        Ok(ModuleInitializationGuard {
            runtime: self,
            module: module.clone(),
            finished: false,
        })
    }

    /// Records failed initialization during unwinding, without reopening execution.
    pub fn fail_module_initialization(&self, module: &LoadedModule) -> Result<(), RuntimeError> {
        if !module.belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation(
                "module belongs to another runtime",
            ));
        }
        let mut instance = self.modules.instance_mut(module.key()).ok_or_else(|| {
            self.resources
                .quarantine("failed module instance disappeared during cleanup")
        })?;
        if instance.state == ModuleInitializationState::Initialized {
            return Err(RuntimeError::module_validation(
                "completed initialization cannot be failed by cleanup",
            ));
        }
        instance.fail_initialization();
        Ok(())
    }

    pub fn module_instance_mut(
        &self,
        module: &LoadedModule,
    ) -> Option<std::cell::RefMut<'_, ModuleInstance>> {
        self.validate_loaded_module(module).ok()?;
        self.modules.instance_mut(module.key())
    }

    pub fn root_value(&self, value: value::Value) -> Option<RootedValue> {
        self.gc.root_value(value)
    }

    fn validate_heap_payloads(&self, values: &[Value]) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if !values
            .iter()
            .all(|value| value.is_default_heap_payload() && self.gc.validate_value(value))
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid or foreign heap payload",
            ));
        }
        Ok(())
    }

    pub fn trace_roots(&self) -> Option<Vec<HeapObjectId>> {
        self.gc.trace_roots()
    }

    pub fn collect_garbage(&self) -> Result<GcCollection, RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let mut roots = self.modules.gc_roots();
        roots.extend(self.host.gc_roots());
        let result = self.gc.collect(&roots).ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap reference in collection roots",
            )
        })?;
        Ok(result)
    }

    pub fn gc_safepoint(&self) -> Result<(), RuntimeError> {
        self.resources.poll_execution()?;
        if self.gc.collection_due() {
            self.collect_garbage()?;
        }
        Ok(())
    }

    pub fn consume_instruction_step(&self) -> Result<(), RuntimeError> {
        self.resources.consume_instruction_step()
    }

    pub fn invoke_host(
        &self,
        symbol: &str,
        args: &[value::Value],
    ) -> Result<value::Value, RuntimeError> {
        let function = self.host.function(symbol).ok_or_else(|| {
            RuntimeError::host_call_failure(format!("unknown host function `{symbol}`"))
        })?;
        self.invoke_bound_host(
            function.id().expect("registered function has an identity"),
            args,
        )
    }

    pub fn invoke_bound_host(
        &self,
        id: HostFunctionId,
        args: &[value::Value],
    ) -> Result<value::Value, RuntimeError> {
        let function = self.host.bound_function(id).ok_or_else(|| {
            RuntimeError::module_validation(
                "host binding belongs to another registry or is missing",
            )
        })?;
        self.validate_bound_host_boundary(function.symbol(), Some(function))?;
        let context = host::HostCallContext::new(self, args)?;
        let result = function.invoke(&context, args);
        self.resources.ensure_execution_allowed()?;
        let value = result?;
        HostBorrowTable::validate_no_escape(&value)?;
        if !self.gc.validate_value(&value) {
            return Err(RuntimeError::host_call_failure(
                "invalid heap reference in host result",
            ));
        }
        Ok(value)
    }

    pub fn reflect_type_of(&self, value: &value::Value) -> Result<value::Value, RuntimeError> {
        self.validate_reflection_metadata_boundary()?;
        self.resources.consume_reflection_operation()?;
        Ok(reflection::type_of(&self.gc, value))
    }

    pub fn reflect_get_field(
        &self,
        value: &value::Value,
        field_name: &str,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_reflection_read_boundary()?;
        self.resources.consume_reflection_operation()?;
        reflection::get_field(&self.gc, value, field_name)
            .map_err(|error| RuntimeError::invalid_reflective_read(error.message()))
    }

    pub fn reflect_set_field(
        &self,
        value: &value::Value,
        field_name: &str,
        next_value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_reflection_write_boundary()?;
        self.resources.consume_reflection_operation()?;
        reflection::set_field(&self.gc, value, field_name, next_value)
            .map_err(|error| RuntimeError::invalid_reflective_write(error.message()))
    }

    pub fn reflect_set_index(
        &self,
        value: &value::Value,
        index: &value::Value,
        next_value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_reflection_write_boundary()?;
        self.resources.consume_reflection_operation()?;
        reflection::set_index(&self.gc, value, index, next_value)
            .map_err(|error| RuntimeError::invalid_reflective_write(error.message()))
    }

    pub fn invoke_standard_builtin(
        &self,
        intrinsic: StandardIntrinsic,
        args: &[value::Value],
    ) -> Result<value::Value, BuiltinError> {
        self.resources.ensure_execution_allowed()?;
        let value = builtin::invoke_standard(&self.gc, intrinsic, args)?;
        Ok(value)
    }

    pub fn validate_loaded_module(&self, module: &LoadedModule) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if !module.belongs_to(self.host.owner()) || self.modules.loaded(module.key()).is_none() {
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
        let name = name.into();
        kagari_ir::bytecode::verify_program(&bytecode).map_err(|error| {
            RuntimeError::module_validation(format!("bytecode validation failed: {error}"))
        })?;
        let dependencies = ReloadDependencySnapshot::from_program(&bytecode);
        let bindings = bytecode
            .modules
            .iter()
            .map(|module| self.host.link_module(module, &self.types))
            .collect::<Result<Vec<_>, _>>()?;
        self.resources
            .record_loaded_modules(self.modules.loaded_count() + bytecode.modules.len())?;
        let epoch = self.reloads.publish(&name);
        let module = self
            .modules
            .load_program(name, epoch, bytecode, self.host.owner(), bindings);
        self.invalidate_execution_artifacts_for_reload(&module, dependencies);
        Ok(module)
    }
    pub fn reload_program(
        &mut self,
        active: &LoadedModule,
        name: impl Into<String>,
        bytecode: BytecodeProgram,
    ) -> Result<LoadedModule, ReloadValidationError> {
        let name = name.into();
        let dependencies = ReloadDependencySnapshot::from_program(&bytecode);
        let candidate = self.prepare_reload(active, name, bytecode, dependencies)?;
        self.publish_prepared_reload(candidate)
    }

    pub fn reload_artifact(
        &mut self,
        active: &LoadedModule,
        name: impl Into<String>,
        artifact: KbcArtifact,
        compatibility: &ArtifactCompatibility,
    ) -> Result<LoadedModule, ReloadValidationError> {
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
        let dependencies = ReloadDependencySnapshot::from_artifact(&artifact);
        let candidate = self.prepare_reload(active, name, artifact.program, dependencies)?;
        self.publish_prepared_reload(candidate)
    }

    fn prepare_reload(
        &self,
        baseline: &LoadedModule,
        name: String,
        bytecode: BytecodeProgram,
        dependencies: ReloadDependencySnapshot,
    ) -> Result<PreparedReload, ReloadValidationError> {
        self.validate_loaded_module(baseline)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&baseline.name);
        validate_reload_candidate(baseline, &name, &bytecode, latest.as_ref())?;
        let bindings = bytecode
            .modules
            .iter()
            .map(|module| self.host.link_module(module, &self.types))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ReloadValidationError::Runtime)?;
        Ok(PreparedReload {
            baseline: baseline.clone(),
            name,
            bytecode,
            bindings,
            dependencies,
        })
    }

    fn publish_prepared_reload(
        &mut self,
        candidate: PreparedReload,
    ) -> Result<LoadedModule, ReloadValidationError> {
        let PreparedReload {
            baseline,
            name,
            bytecode,
            bindings,
            dependencies,
        } = candidate;
        self.validate_loaded_module(&baseline)
            .map_err(ReloadValidationError::Runtime)?;
        let latest = self.modules.latest(&baseline.name);
        validate_reload_candidate(&baseline, &name, &bytecode, latest.as_ref())?;
        for (module, prepared) in bytecode.modules.iter().zip(&bindings) {
            let current = self
                .host
                .link_module(module, &self.types)
                .map_err(ReloadValidationError::Runtime)?;
            if current.functions != prepared.functions || current.paths != prepared.paths {
                return Err(ReloadValidationError::Runtime(
                    RuntimeError::module_validation(
                        "host bindings changed after reload preparation",
                    ),
                ));
            }
        }
        self.resources
            .record_loaded_modules(self.modules.loaded_count() + bytecode.modules.len())
            .map_err(ReloadValidationError::Runtime)?;
        let epoch = self.reloads.publish(&name);
        let module = self
            .modules
            .load_program(name, epoch, bytecode, self.host.owner(), bindings);
        self.invalidate_execution_artifacts_for_reload(&module, dependencies);
        Ok(module)
    }

    fn invalidate_execution_artifacts_for_reload(
        &self,
        module: &LoadedModule,
        dependencies: ReloadDependencySnapshot,
    ) -> Vec<ExecutionArtifactId> {
        let invalidated = self
            .execution_artifacts
            .invalidate_for_reload(&ReloadInvalidation {
                module_name: module.name.clone(),
                module_identity: module.bytecode.identity.clone(),
                module_fingerprint: kagari_ir::bytecode::ArtifactFingerprint::of_serialized(
                    &module.bytecode,
                ),
                module_id: module.id,
                published: module.key(),
                dependencies,
            });
        for artifact in &invalidated {
            if artifact.kind == ExecutionArtifactKind::Jit {
                self.modules
                    .release_epoch(artifact.module, ModuleEpochRetention::CompiledArtifact);
            }
        }
        invalidated
            .into_iter()
            .map(|artifact| artifact.id)
            .collect()
    }
}

fn value_contains_host_owned_data(value: &value::Value) -> bool {
    match value {
        value::Value::Tuple(elements) => elements.iter().any(value_contains_host_owned_data),
        value::Value::HostRoot(_) | value::Value::HostPathView(_) => true,
        value::Value::Ephemeral(
            value::EphemeralValue::HostRef(_) | value::EphemeralValue::HostMut(_),
        ) => true,
        _ => false,
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
            ArtifactBuildOptions, ArtifactCompatibility, BytecodeFunction, BytecodeModule,
            ConstantOperand, DependencyFingerprint, FunctionMetadata, FunctionRef, KbcArtifact,
        },
        module::{FunctionAbi, PublicAbiItem, ValueType},
    };

    use kagari_ir::module::abi::{AbiType, BuiltinType};

    fn module_with_public_function(return_type: BuiltinType) -> BytecodeModule {
        BytecodeModule {
            public_items: vec![PublicAbiItem::Function(FunctionAbi {
                name: "main".to_owned(),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                params: Vec::new(),
                return_type: AbiType::Builtin(return_type),
            })],
            ..BytecodeModule::default()
        }
    }

    fn module_with_public_function_and_constant(
        return_type: BuiltinType,
        value: i32,
    ) -> BytecodeModule {
        let mut module = module_with_public_function(return_type);
        module.constants.push(ConstantOperand::I32(value));
        module
    }

    fn module_with_executable_function() -> BytecodeModule {
        let metadata = FunctionMetadata {
            return_type: ValueType::Unit,
            ..FunctionMetadata::default()
        };
        BytecodeModule {
            types: vec![ValueType::Unit],
            function_table: vec![kagari_ir::bytecode::FunctionRecord {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                params: metadata.params.clone(),
                return_type: metadata.return_type,
                effects: metadata.effects,
            }],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                metadata,
                ..BytecodeFunction::default()
            }],
            ..BytecodeModule::default()
        }
    }

    fn artifact_with_loader_fingerprints() -> KbcArtifact {
        let dependency = BytecodeModule {
            identity: kagari_common::identity::ModuleIdentity::single_file("pkg/dependency"),
            ..Default::default()
        };
        let mut root = module_with_public_function(BuiltinType::I32);
        root.dependencies = vec![kagari_ir::bytecode::ModuleRef::new(0)];
        KbcArtifact::from_program(
            BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(1),
                modules: vec![dependency, root],
            },
            ArtifactBuildOptions {
                security_profile: Some("dev".into()),
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn compatibility_for_artifact(artifact: &KbcArtifact) -> ArtifactCompatibility {
        ArtifactCompatibility {
            module_identity: Some(artifact.header.module_identity.clone()),
            dependency_fingerprints: Some(
                artifact.verification.loader.dependency_fingerprints.clone(),
            ),
            security_profile: artifact.verification.loader.security_profile.clone(),
            ..ArtifactCompatibility::default()
        }
    }

    fn debug_security(capabilities: CapabilitySet) -> SecurityContext {
        SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities,
        }
    }

    struct FakeBackend {
        backend: BackendId,
        target: BackendTarget,
    }

    impl FakeBackend {
        fn new() -> Self {
            Self {
                backend: BackendId::new("test-baseline"),
                target: BackendTarget::new("test-target", 64),
            }
        }
    }

    impl CodegenBackend for FakeBackend {
        fn backend_id(&self) -> BackendId {
            self.backend.clone()
        }

        fn target(&self) -> BackendTarget {
            self.target.clone()
        }

        fn compile_function(
            &mut self,
            input: BackendFunctionInput<'_>,
        ) -> Result<ExecutableFunctionArtifact, BackendCompileError> {
            if input.function().name != "main" {
                return Err(BackendCompileError::unsupported(format!(
                    "unsupported function `{}`",
                    input.function().name
                )));
            }

            let mut artifact = ExecutableFunctionArtifact::new(
                self.backend_id(),
                self.target(),
                input.function_ref(),
            );
            artifact.entry = ExecutableEntryPoint::Symbol(format!(
                "{}::{}",
                input.module().name.as_str(),
                input.function().name
            ));
            artifact.safepoints.push(ExecutableSafepoint {
                instruction_offset: 0,
                kind: ExecutableSafepointKind::RuntimeHelperCall {
                    helper: "test.helper".to_owned(),
                },
                stack_map: ExecutableStackMap::empty(),
            });
            Ok(artifact)
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
            .load_program(
                "first",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
            )
            .unwrap();
        let error = runtime
            .load_program(
                "second",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
            )
            .unwrap_err();

        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert_eq!(runtime.resources().counters().loaded_modules, 1);
    }

    #[test]
    fn reload_publishes_valid_candidate_after_validation() {
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_program(
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect("module should load");

        let reloaded = runtime
            .reload_program(
                &loaded,
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
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
            .load_program(
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect("module should load");
        let before_count = runtime.modules().loaded_count();

        let error = runtime
            .reload_program(
                &loaded,
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::String)],
                },
            )
            .expect_err("public ABI change should reject reload");

        assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
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
            .load_program(
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect("module should load");
        let second = runtime
            .reload_program(
                &first,
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect("compatible module should reload");
        let before_count = runtime.modules().loaded_count();

        let error = runtime
            .reload_program(
                &first,
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect_err("stale active epoch should reject reload");

        assert_eq!(error.code(), "KG_RELOAD_MODULE_NOT_ACTIVE");
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
            .load_program(
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect("module should load");

        let error = runtime
            .reload_program(
                &loaded,
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect_err("resource limit should reject reload before publication");

        assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
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
            .load_program("reloadable", artifact.program.clone())
            .expect("module should load");
        let before_count = runtime.modules().loaded_count();

        let error = runtime
            .reload_artifact(
                &loaded,
                "reloadable",
                artifact,
                &ArtifactCompatibility {
                    dependency_fingerprints: Some(Vec::new()),
                    ..Default::default()
                },
            )
            .expect_err("loader compatibility mismatch should reject reload");

        assert_eq!(error.code(), "KG_ARTIFACT_DEPENDENCY_FINGERPRINT_MISMATCH");
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
            .load_program("reloadable", artifact.program.clone())
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
    fn prepared_reload_is_inert_and_rejects_a_stale_publication() {
        let artifact = artifact_with_loader_fingerprints();
        let mut runtime = Runtime::default();
        let baseline = runtime
            .load_program("reloadable", artifact.program.clone())
            .unwrap();
        let before_count = runtime.modules().loaded_count();
        let before_resources = runtime.resources().counters();
        let prepare = |runtime: &Runtime| {
            runtime
                .prepare_reload(
                    &baseline,
                    "reloadable".into(),
                    artifact.program.clone(),
                    ReloadDependencySnapshot::from_artifact(&artifact),
                )
                .unwrap()
        };
        let candidate = prepare(&runtime);
        let stale = prepare(&runtime);
        assert_eq!(runtime.modules().loaded_count(), before_count);
        assert_eq!(runtime.resources().counters(), before_resources);
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().key(),
            baseline.key()
        );
        let current = runtime.publish_prepared_reload(candidate).unwrap();
        let published_resources = runtime.resources().counters();
        let published_count = runtime.modules().loaded_count();
        assert!(matches!(
            runtime.publish_prepared_reload(stale),
            Err(ReloadValidationError::ModuleNotActive { .. })
        ));
        assert_eq!(
            runtime.modules().latest("reloadable").unwrap().key(),
            current.key()
        );
        assert_eq!(runtime.modules().loaded_count(), published_count);
        assert_eq!(runtime.resources().counters(), published_resources);
        runtime.validate_loaded_module(&baseline).unwrap();
    }

    #[test]
    fn reload_invalidates_artifacts_with_stale_dependency_fingerprints() {
        let dependency_v1 = KbcArtifact::from_program(
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function_and_constant(
                    BuiltinType::I32,
                    1,
                )],
            },
            ArtifactBuildOptions::default(),
        )
        .unwrap();
        let dependency_v2 = KbcArtifact::from_program(
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![module_with_public_function_and_constant(
                    BuiltinType::I32,
                    2,
                )],
            },
            ArtifactBuildOptions::default(),
        )
        .unwrap();
        let dependency_v1_snapshot = ReloadDependencySnapshot::from_artifact(&dependency_v1);
        let dependency_v2_compatibility = compatibility_for_artifact(&dependency_v2);
        let mut runtime = Runtime::default();
        let dependency = runtime
            .load_program("dependency", dependency_v1.program.clone())
            .expect("dependency should load");
        let consumer = runtime
            .load_program(
                "consumer",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect("consumer should load");
        let mut consumer_snapshot = ReloadDependencySnapshot::from_bytecode(&consumer.bytecode);
        consumer_snapshot
            .dependency_fingerprints
            .push(DependencyFingerprint {
                module_id: dependency_v1.header.module_identity.clone(),
                fingerprint: dependency_v1_snapshot.module_fingerprint,
            });

        let interpreter_cache = runtime
            .register_execution_artifact(
                ExecutionArtifactKind::InterpreterCache,
                consumer.key(),
                None,
                consumer_snapshot.clone(),
            )
            .expect("interpreter cache should register");
        let jit_artifact = runtime
            .register_execution_artifact(
                ExecutionArtifactKind::Jit,
                consumer.key(),
                None,
                consumer_snapshot,
            )
            .expect("jit artifact should register");

        assert!(runtime.execution_artifact(interpreter_cache).is_some());
        assert!(runtime.execution_artifact(jit_artifact).is_some());
        assert_eq!(
            runtime
                .modules()
                .retention_counts(consumer.key())
                .compiled_artifacts,
            1
        );

        runtime
            .reload_artifact(
                &dependency,
                "dependency",
                dependency_v2,
                &dependency_v2_compatibility,
            )
            .expect("compatible dependency implementation should reload");

        assert!(runtime.execution_artifact(interpreter_cache).is_none());
        assert!(runtime.execution_artifact(jit_artifact).is_none());
        assert_eq!(
            runtime
                .modules()
                .retention_counts(consumer.key())
                .compiled_artifacts,
            0
        );
    }

    #[test]
    fn reload_invalidates_jit_artifact_for_reloaded_module_epoch_even_when_public_abi_is_stable() {
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_program(
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function_and_constant(
                        BuiltinType::I32,
                        1,
                    )],
                },
            )
            .expect("module should load");
        let artifact = runtime
            .register_execution_artifact(
                ExecutionArtifactKind::Jit,
                loaded.key(),
                None,
                ReloadDependencySnapshot::from_bytecode(&loaded.bytecode),
            )
            .expect("jit artifact should register");

        assert!(runtime.execution_artifact(artifact).is_some());
        assert_eq!(
            runtime
                .modules()
                .retention_counts(loaded.key())
                .compiled_artifacts,
            1
        );

        let reloaded = runtime
            .reload_program(
                &loaded,
                "reloadable",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function_and_constant(
                        BuiltinType::I32,
                        2,
                    )],
                },
            )
            .expect("implementation-only reload should publish a new epoch");

        assert_eq!(reloaded.id, loaded.id);
        assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);
        assert!(runtime.execution_artifact(artifact).is_none());
        assert_eq!(
            runtime
                .modules()
                .retention_counts(loaded.key())
                .compiled_artifacts,
            0
        );
    }

    #[test]
    fn backend_boundary_registers_executable_function_artifacts() {
        let mut runtime = Runtime::default();
        let loaded = runtime
            .load_program(
                "backend_module",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_executable_function()],
                },
            )
            .expect("module should load");
        let dependencies = ReloadDependencySnapshot::from_bytecode(&loaded.bytecode);
        let mut backend = FakeBackend::new();
        let artifact = backend
            .compile_function(BackendFunctionInput::new(&loaded, FunctionRef::new(0)).unwrap())
            .expect("fake backend should compile main");

        let id = runtime
            .register_executable_function_artifact(loaded.key(), dependencies, artifact)
            .expect("executable artifact should register");
        let record = runtime
            .execution_artifact(id)
            .expect("registered artifact should be reachable");

        assert_eq!(record.kind, ExecutionArtifactKind::Jit);
        assert_eq!(record.module, loaded.key());
        assert_eq!(record.function, Some(FunctionRef::new(0)));
        assert_eq!(
            record
                .executable
                .as_ref()
                .expect("artifact should carry executable metadata")
                .backend,
            BackendId::new("test-baseline")
        );
        assert_eq!(
            runtime
                .modules()
                .retention_counts(loaded.key())
                .compiled_artifacts,
            1
        );

        let stale_function_artifact = ExecutableFunctionArtifact::new(
            BackendId::new("test-baseline"),
            BackendTarget::new("test-target", 64),
            FunctionRef::new(99),
        );
        assert!(
            runtime
                .register_executable_function_artifact(
                    loaded.key(),
                    ReloadDependencySnapshot::from_bytecode(&loaded.bytecode),
                    stale_function_artifact,
                )
                .is_none()
        );
        assert_eq!(
            runtime
                .modules()
                .retention_counts(loaded.key())
                .compiled_artifacts,
            1
        );
    }

    #[test]
    fn failed_reload_does_not_invalidate_registered_artifacts() {
        let dependency_v1 = artifact_with_loader_fingerprints();
        let dependency_v1_snapshot = ReloadDependencySnapshot::from_artifact(&dependency_v1);
        let mut dependency_v2 = dependency_v1.clone();
        dependency_v2.program.modules[dependency_v2.program.root.index()]
            .constants
            .push(ConstantOperand::I32(2));
        let mut runtime = Runtime::default();
        let dependency = runtime
            .load_program("dependency", dependency_v1.program.clone())
            .expect("dependency should load");
        let consumer = runtime
            .load_program(
                "consumer",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![module_with_public_function(BuiltinType::I32)],
                },
            )
            .expect("consumer should load");
        let mut consumer_snapshot = ReloadDependencySnapshot::from_bytecode(&consumer.bytecode);
        consumer_snapshot
            .dependency_fingerprints
            .push(DependencyFingerprint {
                module_id: dependency_v1.header.module_identity.clone(),
                fingerprint: dependency_v1_snapshot.module_fingerprint,
            });

        let artifact = runtime
            .register_execution_artifact(
                ExecutionArtifactKind::Jit,
                consumer.key(),
                None,
                consumer_snapshot,
            )
            .expect("jit artifact should register");

        let error = runtime
            .reload_artifact(
                &dependency,
                "dependency",
                dependency_v2,
                &ArtifactCompatibility::default(),
            )
            .expect_err("loader compatibility mismatch should reject reload");

        assert!(matches!(
            error,
            ReloadValidationError::Artifact(
                kagari_ir::bytecode::ArtifactValidationError::ContentHashMismatch
            )
        ));
        assert!(runtime.execution_artifact(artifact).is_some());
        assert_eq!(
            runtime
                .modules()
                .retention_counts(consumer.key())
                .compiled_artifacts,
            1
        );
    }

    #[test]
    fn heap_mutations_update_runtime_resource_counters() {
        let runtime = Runtime::default();
        let array = runtime
            .gc()
            .alloc_array(vec![value::Value::I32(1)])
            .unwrap();
        runtime
            .gc()
            .array_push(array, value::Value::I32(2))
            .unwrap();

        let counters = runtime.resources().counters();

        assert_eq!(counters.current_heap_units, 3);
        assert_eq!(counters.peak_heap_units, 3);
    }

    #[test]
    fn builtin_map_and_set_allocations_update_resource_counters() {
        let runtime = Runtime::default();
        let map = runtime
            .alloc_map(vec![
                (value::Value::Str("hp".to_owned()), value::Value::I32(100)),
                (value::Value::Str("mp".to_owned()), value::Value::I32(20)),
            ])
            .unwrap();
        let set = runtime
            .alloc_set(vec![
                value::Value::Str("ready".to_owned()),
                value::Value::Str("visible".to_owned()),
            ])
            .unwrap();

        assert_eq!(runtime.gc().map_len(map), Some(2));
        assert_eq!(runtime.gc().set_len(set), Some(2));

        let counters = runtime.resources().counters();
        assert_eq!(counters.allocation_units, 6);
        assert_eq!(counters.current_heap_units, 6);
        assert_eq!(counters.peak_heap_units, 6);
    }

    #[test]
    fn exposes_runtime_type_registry_and_security_context() {
        let runtime = Runtime::new(RuntimeConfig {
            security: SecurityContext {
                capabilities: CapabilitySet {
                    reflection_metadata: true,
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

    #[test]
    fn security_context_requires_profile_and_capability_for_runtime_boundaries() {
        let default_runtime = Runtime::default();
        assert_eq!(
            default_runtime
                .validate_reflection_metadata_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime
                .validate_reflection_read_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime
                .validate_reflection_write_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime
                .validate_dynamic_invocation_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime
                .validate_downcast_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime
                .validate_host_function_boundary("host.missing")
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime
                .validate_path_mutation_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime
                .validate_module_loading_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
        assert_eq!(
            default_runtime.validate_jit_boundary().unwrap_err().kind(),
            RuntimeErrorKind::CapabilityDenied
        );

        let enabled = Runtime::new(RuntimeConfig {
            security: SecurityContext {
                profile: LanguageProfile {
                    allow_host_calls: true,
                    allow_path_mutation: true,
                    allow_module_loading: true,
                    allow_jit: true,
                    allow_reflection: true,
                    allow_reflection_write: true,
                    ..LanguageProfile::default()
                },
                capabilities: CapabilitySet {
                    host_calls: true,
                    path_mutation: true,
                    module_loading: true,
                    jit: true,
                    reflection_metadata: true,
                    reflection_read: true,
                    reflection_write: true,
                    dynamic_invocation: true,
                    downcast: true,
                    ..CapabilitySet::default()
                },
            },
            host_exposure: HostExposurePolicy {
                allow_host_functions: true,
                ..HostExposurePolicy::default()
            },
            ..RuntimeConfig::default()
        });

        assert!(
            enabled
                .validate_host_function_boundary("host.missing")
                .is_ok()
        );
        assert!(enabled.validate_reflection_metadata_boundary().is_ok());
        assert!(enabled.validate_reflection_read_boundary().is_ok());
        assert!(enabled.validate_reflection_write_boundary().is_ok());
        assert!(enabled.validate_dynamic_invocation_boundary().is_ok());
        assert!(enabled.validate_downcast_boundary().is_ok());
        assert!(enabled.validate_path_mutation_boundary().is_ok());
        assert!(enabled.validate_module_loading_boundary().is_ok());
        assert!(enabled.validate_jit_boundary().is_ok());
    }

    #[test]
    fn security_restricted_profiles_disable_runtime_boundaries_independently() {
        let capability_only = Runtime::new(RuntimeConfig {
            security: SecurityContext {
                profile: LanguageProfile {
                    allow_interface_values: false,
                    ..LanguageProfile::default()
                },
                capabilities: CapabilitySet {
                    host_calls: true,
                    path_mutation: true,
                    module_loading: true,
                    jit: true,
                    reflection_metadata: true,
                    reflection_read: true,
                    reflection_write: true,
                    dynamic_invocation: true,
                    downcast: true,
                    debug_attach: true,
                    debug_breakpoints: true,
                    debug_pause: true,
                    debug_stack_inspection: true,
                    debug_value_inspection: true,
                    debug_watch_evaluation: true,
                    ..CapabilitySet::default()
                },
            },
            host_exposure: HostExposurePolicy {
                allow_host_functions: true,
                ..HostExposurePolicy::default()
            },
            ..RuntimeConfig::default()
        });

        let denied = [
            capability_only.validate_host_function_boundary("host.open"),
            capability_only.validate_path_mutation_boundary(),
            capability_only.validate_module_loading_boundary(),
            capability_only.validate_jit_boundary(),
            capability_only.validate_reflection_metadata_boundary(),
            capability_only.validate_reflection_read_boundary(),
            capability_only.validate_reflection_write_boundary(),
            capability_only.validate_dynamic_invocation_boundary(),
            capability_only.validate_downcast_boundary(),
            capability_only.validate_debug_attach_boundary(),
            capability_only.validate_debug_breakpoint_boundary(),
            capability_only.validate_debug_pause_boundary(),
            capability_only.validate_debug_stack_inspection_boundary(),
            capability_only.validate_debug_value_inspection_boundary(),
            capability_only.validate_debug_watch_evaluation_boundary(),
        ];

        for result in denied {
            assert_eq!(
                result
                    .expect_err("restricted profile should deny boundary")
                    .kind(),
                RuntimeErrorKind::CapabilityDenied
            );
        }
    }

    #[test]
    fn downcast_gate_is_independent_from_reflection_gates() {
        let downcast_only = Runtime::new(RuntimeConfig {
            security: SecurityContext {
                capabilities: CapabilitySet {
                    downcast: true,
                    ..CapabilitySet::default()
                },
                ..SecurityContext::default()
            },
            ..RuntimeConfig::default()
        });

        assert!(downcast_only.validate_downcast_boundary().is_ok());
        assert_eq!(
            downcast_only
                .validate_reflection_metadata_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );

        let metadata_only = Runtime::new(RuntimeConfig {
            security: SecurityContext {
                profile: LanguageProfile {
                    allow_reflection: true,
                    ..LanguageProfile::default()
                },
                capabilities: CapabilitySet {
                    reflection_metadata: true,
                    ..CapabilitySet::default()
                },
            },
            ..RuntimeConfig::default()
        });

        assert!(
            metadata_only
                .validate_reflection_metadata_boundary()
                .is_ok()
        );
        assert_eq!(
            metadata_only
                .validate_downcast_boundary()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
    }

    #[test]
    fn debug_visibility_respects_host_value_policy() {
        let host_value = value::Value::HostRoot(HostRootHandle::new(
            Default::default(),
            HostObjectId(1),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        ));
        let runtime_without_host_debug = Runtime::new(RuntimeConfig {
            security: debug_security(CapabilitySet {
                debug_value_inspection: true,
                ..CapabilitySet::default()
            }),
            ..RuntimeConfig::default()
        });

        assert_eq!(
            runtime_without_host_debug
                .validate_debug_value_visible(&host_value)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );

        let runtime_with_host_debug = Runtime::new(RuntimeConfig {
            security: debug_security(CapabilitySet {
                debug_value_inspection: true,
                debug_host_value_inspection: true,
                ..CapabilitySet::default()
            }),
            debug_visibility: DebugVisibilityPolicy {
                allow_host_value_inspection: true,
                ..DebugVisibilityPolicy::default()
            },
            ..RuntimeConfig::default()
        });

        assert!(
            runtime_with_host_debug
                .validate_debug_value_visible(&host_value)
                .is_ok()
        );
    }
}
