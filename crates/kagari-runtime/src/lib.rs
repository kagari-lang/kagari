mod authority;
mod loading;
mod objects;
mod observer;
use crate::{
    builtin::BuiltinError,
    cache::{
        InterpreterCacheId, InterpreterCacheRecord, InterpreterCacheRegistry,
        ReloadDependencySnapshot,
    },
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{applications::ApplicationId, links::MethodSelection},
    frame::{ExecutionStack, types::TypeEnvironment},
    gc::{GcCollection, GcHeap, GcHeapConfig, HeapObjectId, roots::RootedValue},
    host::{
        FrameHostBorrowToken, HostBorrowKind, HostBorrowTable, HostCallContext, HostFunction,
        HostFunctionId, HostRegistry, HostTypeRegistration,
    },
    host_scope::HostResourceScope,
    metadata::{TypeId, TypeRegistry},
    module::{
        LoadedModule, ModuleEpochRetention, ModuleKey, ModuleStore, VerifiedProgram,
        staging::StagedProgram,
    },
    native::{
        callable::PreparedClosure, completion::CompletionRegistry,
        function_handle::cache::FunctionCache, interfaces::binding::InterfaceCache,
        objects::cache::BindingCache, registry::NativeRegistry,
    },
    objects::method::BoundReceiver,
    reflection::ReflectionError,
    resource::{AsyncLimits, ResourceState, RuntimeLimits},
    session::{
        CandidateSession, ExecutionEntry, ExecutionObserver, ExecutionOptions, ExecutionPhase,
        ExecutionSession,
    },
    value::Value,
};
use kagari_bytecode::instruction::BinaryOp;
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::{ids::FunctionRef, standard::RuntimePrimitive};
use kagari_types::host_interface::path::HostPathDeclaration;
use std::{cell::RefCell, sync::OnceLock};

pub mod error_trace;
#[cfg(test)]
extern crate self as kagari_runtime;
pub mod backend;
pub mod builtin;
pub mod cache;
pub mod closure;
pub mod error;
mod execution_metadata;
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
pub mod numeric;
pub mod range;
pub mod reflection;
pub mod reload;
pub mod resource;
mod value_check;

pub mod session;
pub mod value;
pub mod value_semantics;

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
    program: StagedProgram,
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

    pub limits: RuntimeLimits,
    pub async_limits: AsyncLimits,
}

/// An exclusively driven script runtime. Move ownership between threads outside
/// active execution and borrow scopes; shared concurrent heap access is forbidden.
///
/// ```compile_fail
/// use kagari_runtime::Runtime;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<Runtime>();
/// ```
#[derive(Debug)]
pub struct Runtime {
    operations: OnceLock<Result<CompletionRegistry, RuntimeError>>,
    async_limits: AsyncLimits,
    gc: GcHeap,
    types: TypeRegistry,
    host: HostRegistry,
    native_entries: NativeRegistry,
    host_borrows: HostBorrowTable,

    modules: ModuleStore,
    interpreter_caches: InterpreterCacheRegistry,
    object_bindings: RefCell<BindingCache>,
    function_bindings: RefCell<FunctionCache>,
    interface_bindings: RefCell<InterfaceCache>,
    observer: RefCell<Option<Box<dyn ExecutionObserver>>>,
}

/// A resolved dynamic method whose interface receiver stays rooted across
/// safepoints and synchronous host reentry.
#[derive(Clone)]
pub struct RootedInterfaceMethod {
    selection: MethodSelection,
    bound_receiver: BoundReceiver,
    environment: Option<TypeEnvironment>,
    application: Option<ApplicationId>,
    _root: RootedValue,
}

impl Runtime {
    pub(crate) fn definition_context(&self) -> &DefinitionContext {
        self.native_entries.catalog.types.context()
    }

    pub fn new(config: RuntimeConfig) -> Self {
        let resources = ResourceState::new(config.limits);
        Self {
            operations: OnceLock::new(),
            async_limits: config.async_limits,
            gc: GcHeap::new(config.gc, resources),
            types: TypeRegistry::default(),
            host: HostRegistry::default(),
            native_entries: NativeRegistry::default(),
            host_borrows: HostBorrowTable::default(),

            modules: ModuleStore::default(),
            interpreter_caches: InterpreterCacheRegistry::default(),
            object_bindings: RefCell::default(),
            function_bindings: RefCell::default(),
            interface_bindings: RefCell::default(),
            observer: RefCell::new(None),
        }
    }

    pub fn is_quarantined(&self) -> bool {
        self.resources().is_quarantined()
    }

    /// Report an invariant failure detected by an execution backend.
    pub fn quarantine_execution_invariant(&self, reason: &'static str) -> RuntimeError {
        self.resources().quarantine(reason)
    }

    pub fn execution_root(&self) -> Option<LoadedModule> {
        self.resources()
            .active_session()
            .map(|session| session.root.clone())
    }

    pub fn enter_execution_stack(
        &self,
        module: &LoadedModule,
    ) -> Result<ExecutionStack<'_>, RuntimeError> {
        self.gc.ensure_no_native_borrow()?;
        let session = self.begin_execution(module, self.execution_options())?;
        ExecutionStack::new(session)
    }

    /// A live closure carries its own generation-pinned dependency program.
    /// Nested closure calls share the current session and its counters, while
    /// ordinary entries still require the current caller's checked dependency graph.
    pub fn enter_closure_execution_stack(
        &self,
        closure: &PreparedClosure,
    ) -> Result<ExecutionStack<'_>, RuntimeError> {
        closure.validate(self)?;
        let owner = closure.snapshot(self)?.implementation.clone();
        let session = self.begin_execution_inner(
            &owner,
            self.execution_options(),
            ExecutionEntry::RetainedClosure,
        )?;
        ExecutionStack::new(session)
    }

    pub fn execution_options(&self) -> ExecutionOptions {
        if let Some(session) = self.resources().active_session() {
            return session.options.clone();
        }
        ExecutionOptions {
            phase: ExecutionPhase::Ordinary,

            cancellation: Default::default(),
            inputs: Default::default(),
            record_host_calls: false,
        }
    }

    pub fn execution_time_millis(&self) -> Result<i64, RuntimeError> {
        let session = self.resources().active_session().ok_or_else(|| {
            RuntimeError::module_validation("execution time requires an active session")
        })?;
        Ok(session.options.inputs.unix_time_millis)
    }

    pub fn next_execution_random_u64(&self) -> Result<u64, RuntimeError> {
        let session = self.resources().active_session().ok_or_else(|| {
            RuntimeError::module_validation("execution random requires an active session")
        })?;
        Ok(session.next_random_u64())
    }

    pub fn begin_candidate_initialization<'candidate>(
        &self,
        candidate: &'candidate StagedReload,
    ) -> Result<CandidateSession<'_, 'candidate>, RuntimeError> {
        if let Some(error) = candidate.initialization_error() {
            return Err(error);
        }
        self.validate_loaded_module(candidate.module())?;
        if self.is_candidate_initialization() {
            return Err(RuntimeError::execution_phase_violation(
                "nested candidate initialization",
            ));
        }
        let mut options = self.execution_options();
        options.phase = ExecutionPhase::CandidateInitialization;
        let previous = self.resources().replace_session(None);
        let mut guard = CandidateSession {
            candidate,
            execution: None,
            previous,
            resources: self.resources(),
        };
        match self.begin_execution_inner(candidate.module(), options, ExecutionEntry::Candidate) {
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
    ) -> Result<ExecutionSession<'_>, RuntimeError> {
        self.begin_execution_inner(module, options, ExecutionEntry::Program)
    }

    fn begin_execution_inner(
        &self,
        module: &LoadedModule,
        options: ExecutionOptions,
        entry: ExecutionEntry,
    ) -> Result<ExecutionSession<'_>, RuntimeError> {
        self.gc.ensure_no_native_borrow()?;
        if self.modules.is_staged(module)
            && self.resources().active_session().is_none()
            && !matches!(entry, ExecutionEntry::Candidate)
        {
            return Err(RuntimeError::execution_phase_violation(
                "staged modules require the candidate session entry",
            ));
        }
        self.validate_loaded_module(module)?;
        let phase = self
            .resources()
            .active_session()
            .map_or(options.phase, |session| session.options.phase);
        if self.modules.is_staged(module) && phase != ExecutionPhase::CandidateInitialization {
            return Err(RuntimeError::execution_phase_violation(
                "staged modules require candidate initialization execution",
            ));
        }
        let id = if let Some(session) = self.resources().active_session() {
            if options.phase == ExecutionPhase::CandidateInitialization
                && session.options.phase != ExecutionPhase::CandidateInitialization
            {
                return Err(RuntimeError::execution_phase_violation(
                    "candidate initialization requires an isolated root session",
                ));
            }
            let root_dependency = session
                .root
                .members()
                .any(|member| member.key() == module.key());
            let caller_dependency = self
                .resources()
                .sessions
                .frames(session.id)
                .ok_or_else(|| {
                    self.resources()
                        .quarantine("frame stack is borrowed across execution")
                })?
                .last()
                .is_some_and(|frame| {
                    frame
                        .loaded()
                        .members()
                        .any(|member| member.key() == module.key())
                });
            if !root_dependency
                && !caller_dependency
                && !matches!(
                    entry,
                    ExecutionEntry::RetainedClosure | ExecutionEntry::RetainedFunction
                )
            {
                return Err(RuntimeError::module_validation(
                    "nested execution must use the pinned dependency program",
                ));
            }
            session.id
        } else {
            let program = self
                .retain_module(module, ModuleEpochRetention::ActiveCall)
                .ok_or_else(|| RuntimeError::module_validation("execution program was released"))?;
            let id = self.resources().sessions.insert(
                module.clone(),
                options,
                self.resources().counters(),
                program,
            )?;
            self.resources().start_execution(id);
            id
        };
        let state = self
            .resources()
            .sessions
            .get(id)
            .expect("entered execution session");
        let scopes = state.scopes.get().checked_add(1).ok_or_else(|| {
            self.resources()
                .quarantine("execution scope count overflow")
        })?;
        state.scopes.set(scopes);
        drop(state);
        let guard = ExecutionSession {
            id,
            resources: self.resources(),
        };
        self.resources().poll_execution()?;
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
        self.resources().ensure_execution_allowed()?;
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
        self.resources().ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
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
        self.resources().ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
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
        self.resources().ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
        let result = self
            .host
            .read_path(self, root_or_view, descriptor_id, dynamic_args);
        self.resources().ensure_execution_allowed()?;
        result
    }

    pub fn set_host_path(
        &self,
        root_or_view: &value::Value,
        descriptor_id: host::HostPathDescriptorId,
        dynamic_args: Vec<value::Value>,
        value: value::Value,
    ) -> Result<(), RuntimeError> {
        self.resources().poll_execution()?;
        self.reject_candidate_external_access()?;
        let result = self
            .host
            .set_path(self, root_or_view, descriptor_id, dynamic_args, value);
        self.resources().ensure_execution_allowed()?;
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
        self.resources().poll_execution()?;
        self.reject_candidate_external_access()?;
        let result =
            self.host
                .modify_path(self, root_or_view, descriptor_id, dynamic_args, op, value);
        self.resources().ensure_execution_allowed()?;
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

    pub fn resources(&self) -> &ResourceState {
        self.gc.resources()
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
        self.modules.loaded(artifact.module)?;
        Some(artifact)
    }

    pub fn root_value(&self, value: value::Value) -> Option<RootedValue> {
        self.gc.root_value(value)
    }

    fn validate_heap_payloads(&self, values: &[Value]) -> Result<(), RuntimeError> {
        self.resources().ensure_execution_allowed()?;
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

    /// Collect unreachable heap objects and module instances in one graph traversal.
    pub fn collect_garbage(&self) -> Result<GcCollection, RuntimeError> {
        self.gc.ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        self.gc.collect(&self.modules, &self.host.gc_roots())
    }

    pub fn gc_safepoint(&self) -> Result<(), RuntimeError> {
        self.resources().poll_execution()?;
        if self.gc.collection_due()
            || (self.gc.automatic_collection_enabled() && self.modules.has_abandoned_programs()?)
        {
            self.collect_garbage()?;
        }
        Ok(())
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
            return Err(RuntimeError::execution_phase_violation(
                "external object in candidate host arguments",
            ));
        }
        self.validate_bound_host_boundary(function.symbol(), Some(function))?;
        let session = self.resources().active_session();
        let trace_index = session
            .as_ref()
            .and_then(|session| session.begin_host_call(function.symbol(), args));
        let session = session.map(|session| session.id);
        let result = (|| {
            let context = HostCallContext::new(self, args)?;
            let result = function.invoke(&context, args);
            self.resources().poll_execution()?;
            let value = result?;
            HostBorrowTable::validate_no_escape(&value)?;
            if !self.gc.validate_value(&value) {
                return Err(RuntimeError::host_call_failure(
                    "invalid heap reference in host result",
                ));
            }
            Ok(value)
        })();
        if let (Some(id), Some(index)) = (session, trace_index)
            && let Some(session) = self.resources().sessions.get(id)
        {
            session.finish_host_call(index, &result);
        }
        result
    }

    pub fn reflect_type_of(&self, value: &value::Value) -> Result<value::Value, RuntimeError> {
        self.resources().poll_execution()?;
        Ok(reflection::type_of(&self.gc, value))
    }

    pub fn reflect_get_field(
        &self,
        value: &value::Value,
        field_name: &str,
    ) -> Result<value::Value, RuntimeError> {
        self.resources().poll_execution()?;
        reflection::get_field(&self.gc, value, field_name)
            .map_err(|error| RuntimeError::invalid_reflective_read(error.message()))
    }

    pub fn reflect_set_field(
        &self,
        value: &value::Value,
        field_name: &str,
        next_value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.resources().poll_execution()?;
        reflection::set_field(&self.gc, value, field_name, next_value)
            .map_err(ReflectionError::into_write_error)
    }

    pub fn reflect_set_index(
        &self,
        value: &value::Value,
        index: &value::Value,
        next_value: value::Value,
    ) -> Result<value::Value, RuntimeError> {
        self.resources().poll_execution()?;
        reflection::set_index(&self.gc, value, index, next_value)
            .map_err(ReflectionError::into_write_error)
    }

    pub fn invoke_standard_builtin(
        &self,
        owner: &LoadedModule,
        intrinsic: RuntimePrimitive,
        args: &[value::Value],
    ) -> Result<value::Value, BuiltinError> {
        self.resources().ensure_execution_allowed()?;
        let value = builtin::invoke_standard(self, owner, intrinsic, args)?;
        Ok(value)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}

#[cfg(test)]
mod tests;
