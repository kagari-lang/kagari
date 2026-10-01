mod authority;
mod loading;
mod objects;
use host::HostCallContext;
use kagari_abi::{
    budget::LogicalBudgetCharge,
    ids::FunctionRef,
    standard::RuntimePrimitive,
    types::{AbiType, NominalAbiType},
};
use kagari_bytecode::{instruction::BinaryOp, module::CallableTarget};
use kagari_common::host_interface::path::HostPathDeclaration;
use reflection::ReflectionError;
use session::SessionState;
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};
use value::{EphemeralValue, Value};
pub mod error_trace;
#[cfg(test)]
extern crate self as kagari_runtime;
pub mod backend;
pub mod builtin;
pub mod cache;
pub mod error;
mod execution_state;
pub mod frame;
pub mod gc;
pub mod host;
pub mod host_scope;
pub mod jit_abi;
#[cfg(test)]
#[path = "../tests/support/layouts.rs"]
mod layout_fixtures;
pub mod metadata;
pub mod module;
pub mod native;
pub mod native_module;
pub mod native_value;
pub mod numeric;
pub mod range;
pub mod reflection;
pub mod reload;
pub mod resource;
pub mod security;
pub mod session;
pub mod value;
pub mod value_semantics;

use crate::{
    builtin::BuiltinError,
    cache::{
        InterpreterCacheId, InterpreterCacheRecord, InterpreterCacheRegistry,
        ReloadDependencySnapshot,
    },
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionStack,
    gc::{GcCollection, GcHeap, GcHeapConfig, HeapObjectId, RootedValue},
    host::{
        FrameHostBorrowToken, HostBorrowKind, HostBorrowTable, HostFunction, HostFunctionId,
        HostPathOperation, HostRegistry, HostTypeRegistration,
    },
    host_scope::HostResourceScope,
    metadata::{TypeId, TypeRegistry},
    module::{
        LoadedModule, ModuleEpochRetention, ModuleInstance, ModuleKey, ModuleStore, VerifiedProgram,
    },
    native::registration::NativeRegistry,
    reload::ModuleEpochAllocator,
    resource::{ResourcePolicy, ResourceState},
    security::{DebugVisibilityPolicy, HostExposurePolicy, SecurityContext},
    session::{
        ExecutionEvent, ExecutionObserver, ExecutionOptions, ExecutionPhase, ExecutionSession,
    },
};

/// Verified and linked reload data that has not changed any runtime entry.
struct PreparedReload {
    baseline: LoadedModule,
    name: String,
    program: VerifiedProgram,
    bindings: Vec<module::LinkedHostBindings>,
}

/// Installed candidate whose entry has not been activated.
#[derive(Debug)]
pub struct StagedReload {
    initialization_error: RefCell<Option<RuntimeError>>,
    baseline: LoadedModule,
    program: module::StagedProgram,
}

impl StagedReload {
    pub fn initialization_error(&self) -> Option<RuntimeError> {
        self.initialization_error.borrow().clone()
    }

    pub(crate) fn record_initialization_error(&self, error: RuntimeError) {
        self.initialization_error.borrow_mut().get_or_insert(error);
    }

    pub fn module(&self) -> &LoadedModule {
        self.program.module()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    pub gc: GcHeapConfig,
    pub security: SecurityContext,
    pub host_exposure: HostExposurePolicy,
    pub debug_visibility: DebugVisibilityPolicy,
    pub resources: ResourcePolicy,
}

#[derive(Debug)]
pub struct Runtime {
    gc: Rc<GcHeap>,
    types: TypeRegistry,
    host: HostRegistry,
    native_entries: NativeRegistry,
    host_borrows: HostBorrowTable,
    security: SecurityContext,
    host_exposure: Rc<HostExposurePolicy>,
    debug_visibility: DebugVisibilityPolicy,
    resources: Rc<ResourceState>,
    epochs: ModuleEpochAllocator,
    modules: ModuleStore,
    interpreter_caches: InterpreterCacheRegistry,
}

/// A resolved dynamic method whose interface receiver stays rooted across
/// safepoints and synchronous host reentry.
pub struct RootedInterfaceMethod {
    _root: RootedValue,
    receiver: value::Value,
    concrete_type: AbiType,
    interface_type: NominalAbiType,
    implementation: LoadedModule,
    target: CallableTarget,
    parameter_types: Vec<AbiType>,
    return_type: AbiType,
}

impl RootedInterfaceMethod {
    pub fn receiver(&self) -> &value::Value {
        &self.receiver
    }
    pub fn concrete_type(&self) -> &AbiType {
        &self.concrete_type
    }
    pub fn interface_type(&self) -> &NominalAbiType {
        &self.interface_type
    }
    pub fn implementation(&self) -> &LoadedModule {
        &self.implementation
    }
    pub fn target(&self) -> CallableTarget {
        self.target
    }
    pub fn parameter_types(&self) -> &[AbiType] {
        &self.parameter_types
    }
    pub fn return_type(&self) -> &AbiType {
        &self.return_type
    }
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        let resources = Rc::new(ResourceState::new(config.resources));
        Self {
            gc: Rc::new(GcHeap::new(config.gc, resources.clone())),
            types: TypeRegistry::default(),
            host: HostRegistry::default(),
            native_entries: NativeRegistry::default(),
            host_borrows: HostBorrowTable::with_resources(&resources),
            security: config.security,
            host_exposure: Rc::new(config.host_exposure),
            debug_visibility: config.debug_visibility,
            modules: ModuleStore::new(resources.clone()),
            resources,
            epochs: ModuleEpochAllocator::default(),
            interpreter_caches: InterpreterCacheRegistry::default(),
        }
    }

    pub fn is_quarantined(&self) -> bool {
        self.resources.is_quarantined()
    }

    /// Report an invariant failure detected by an execution backend.
    pub fn quarantine_execution_invariant(&self, reason: &'static str) -> RuntimeError {
        self.resources.quarantine(reason)
    }

    pub fn execution_root(&self) -> Option<LoadedModule> {
        self.resources
            .active_session()
            .map(|session| session.root.clone())
    }

    /// Install once for the root call. Nested drivers inherit the same observer.
    pub fn attach_execution_observer(
        &self,
        observer: Rc<dyn ExecutionObserver>,
    ) -> Result<bool, RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let session = self.resources.active_session().ok_or_else(|| {
            RuntimeError::module_validation("execution observer requires an active session")
        })?;
        let mut active = session.observer.borrow_mut();
        if let Some(existing) = active.as_ref() {
            if !Rc::ptr_eq(existing, &observer) {
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
            phase: ExecutionPhase::Ordinary,
            security: self.security,
            host_exposure: self.host_exposure.clone(),
            resources: self.resources.policy(),
            cancellation: Default::default(),
            inputs: Default::default(),
            record_host_calls: false,
        }
    }

    pub fn execution_time_millis(&self) -> Result<i64, RuntimeError> {
        let session = self.resources.active_session().ok_or_else(|| {
            RuntimeError::module_validation("execution time requires an active session")
        })?;
        Ok(session.options.inputs.unix_time_millis)
    }

    pub fn next_execution_random_u64(&self) -> Result<u64, RuntimeError> {
        let session = self.resources.active_session().ok_or_else(|| {
            RuntimeError::module_validation("execution random requires an active session")
        })?;
        Ok(session.next_random_u64())
    }

    pub fn begin_candidate_initialization<'candidate>(
        &self,
        candidate: &'candidate StagedReload,
    ) -> Result<session::CandidateSession<'candidate>, RuntimeError> {
        if let Some(error) = candidate.initialization_error() {
            return Err(error);
        }
        self.validate_loaded_module(candidate.module())?;
        if self.is_candidate_initialization() {
            return Err(RuntimeError::capability_denied(
                "nested candidate initialization",
            ));
        }
        let mut options = self.execution_options();
        options.phase = ExecutionPhase::CandidateInitialization;
        let previous = self.resources.replace_session(None);
        let mut guard = session::CandidateSession {
            candidate,
            execution: None,
            previous,
            resources: self.resources.clone(),
        };
        match self.begin_execution_inner(candidate.module(), options, true) {
            Ok(execution) => guard.execution = Some(execution),
            Err(error) => {
                candidate.record_initialization_error(error.clone());
                return Err(error);
            }
        }
        Ok(guard)
    }

    pub fn begin_execution(
        &self,
        module: &LoadedModule,
        options: ExecutionOptions,
    ) -> Result<ExecutionSession, RuntimeError> {
        self.begin_execution_inner(module, options, false)
    }

    fn begin_execution_inner(
        &self,
        module: &LoadedModule,
        options: ExecutionOptions,
        allow_staged_root: bool,
    ) -> Result<ExecutionSession, RuntimeError> {
        if self.modules.is_staged(module)
            && self.resources.active_session().is_none()
            && !allow_staged_root
        {
            return Err(RuntimeError::capability_denied(
                "staged modules require the candidate session entry",
            ));
        }
        self.validate_loaded_module(module)?;
        let phase = self
            .resources
            .active_session()
            .map_or(options.phase, |session| session.options.phase);
        if self.modules.is_staged(module) && phase != ExecutionPhase::CandidateInitialization {
            return Err(RuntimeError::capability_denied(
                "staged modules require candidate initialization execution",
            ));
        }
        let state = if let Some(session) = self.resources.active_session() {
            if options.phase == ExecutionPhase::CandidateInitialization
                && session.options.phase != ExecutionPhase::CandidateInitialization
            {
                return Err(RuntimeError::capability_denied(
                    "candidate initialization requires an isolated root session",
                ));
            }
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
            let session = Rc::new(SessionState::new(
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
            gc: self.gc.clone(),
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

    pub fn register_host_path(
        &mut self,
        declaration: &HostPathDeclaration,
    ) -> Result<host::HostPathDescriptorId, RuntimeError> {
        self.host.register_path(declaration, &self.types)
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
        self.validate_host_path_exposure(descriptor_id, HostPathOperation::MakeView)?;
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
        self.validate_host_path_exposure(descriptor_id, HostPathOperation::MakeView)?;
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
        self.validate_host_path_exposure(descriptor_id, HostPathOperation::Read)?;
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
        self.validate_host_path_exposure(descriptor_id, HostPathOperation::Set)?;
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
        op: BinaryOp,
        value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.validate_host_path_exposure(descriptor_id, HostPathOperation::Modify(op))?;
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

    pub fn host_exposure(&self) -> Rc<HostExposurePolicy> {
        self.resources.active_session().map_or_else(
            || self.host_exposure.clone(),
            |session| session.options.host_exposure.clone(),
        )
    }

    pub fn set_host_exposure_policy(&mut self, policy: HostExposurePolicy) {
        self.host_exposure = Rc::new(policy);
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

    /// Register runtime-local interpreter metadata without retaining the code version.
    /// Native code must use `install_native_function` with its executable owner.
    pub fn register_interpreter_cache(
        &self,
        module: ModuleKey,
        function: Option<FunctionRef>,
        dependencies: ReloadDependencySnapshot,
    ) -> Option<InterpreterCacheId> {
        self.modules.loaded(module)?;
        Some(
            self.interpreter_caches
                .register(module, function, dependencies),
        )
    }

    pub fn interpreter_cache(&self, id: InterpreterCacheId) -> Option<InterpreterCacheRecord> {
        let artifact = self.interpreter_caches.get(id)?;
        if !self.modules.is_reachable(artifact.module) {
            return None;
        }
        Some(artifact)
    }

    pub fn module_instance_snapshot(&self, module: &LoadedModule) -> Option<ModuleInstance> {
        self.validate_loaded_module(module).ok()?;
        self.modules.instance_snapshot(module.key())
    }

    pub fn module_instance_mut(
        &self,
        module: &LoadedModule,
    ) -> Result<RefMut<'_, ModuleInstance>, RuntimeError> {
        self.validate_loaded_module(module)?;
        if !self.modules.allows_instance_access(module.key()) {
            return Err(RuntimeError::capability_denied(
                "candidate cannot access external module state",
            ));
        }
        self.modules.instance_mut(module.key()).ok_or_else(|| {
            self.resources
                .quarantine("loaded module instance disappeared")
        })
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
            self.resources
                .quarantine("invalid heap reference in collection roots")
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

    pub fn consume_logical_charge(&self, charge: LogicalBudgetCharge) -> Result<(), RuntimeError> {
        self.resources
            .consume_instruction_steps(charge.instruction_steps())
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
        if !args
            .iter()
            .all(|value| self.gc.validate_candidate_value(value))
        {
            return Err(RuntimeError::capability_denied(
                "external object in candidate host arguments",
            ));
        }
        self.validate_bound_host_boundary(function.symbol(), Some(function))?;
        let session = self.resources.active_session();
        let trace_index = session
            .as_ref()
            .and_then(|session| session.begin_host_call(function.symbol(), args));
        let result = (|| {
            let context = HostCallContext::new(self, args)?;
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
        })();
        if let (Some(session), Some(index)) = (session, trace_index) {
            session.finish_host_call(index, &result);
        }
        result
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
            .map_err(ReflectionError::into_write_error)
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
            .map_err(ReflectionError::into_write_error)
    }

    pub fn invoke_standard_builtin(
        &self,
        intrinsic: RuntimePrimitive,
        args: &[value::Value],
    ) -> Result<value::Value, BuiltinError> {
        self.resources.ensure_execution_allowed()?;
        let value = builtin::invoke_standard(&self.gc, intrinsic, args)?;
        Ok(value)
    }
}

fn value_contains_host_owned_data(value: &value::Value) -> bool {
    match value {
        Value::Tuple(elements) => elements.iter().any(value_contains_host_owned_data),
        Value::HostRoot(_) | Value::HostPathView(_) => true,
        Value::Ephemeral(EphemeralValue::HostRef(_) | EphemeralValue::HostMut(_)) => true,
        _ => false,
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}

#[cfg(test)]
mod tests;
