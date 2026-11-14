// Recursive application/type witnesses need deeper Send/Sync evaluation on the
// pinned Miri compiler. This changes compiler evaluation, not execution depth.
#![recursion_limit = "256"]

use crate::{
    builtin::BuiltinError,
    cache::{
        InterpreterCacheId, InterpreterCacheRecord, InterpreterCacheRegistry,
        ReloadDependencySnapshot,
    },
    clock::Instant,
    closure::calls::SelectedClosure,
    context::ExecutionContext,
    error::{RuntimeError, fault::EngineFault, reason::CancellationReason},
    execution::{
        memory::{GcHeapConfig, ManagedMemory},
        roots::ExecutionRoots,
    },
    execution_metadata::{identity::MetadataIds, managed::applications::ApplicationObject},
    frame::ExecutionStack,
    host::{
        FrameHostBorrowToken, HostBorrowKind, HostBorrowTable, HostRegistry,
        installation::PreparedHostTypes,
    },
    host_scope::HostResourceScope,
    metadata::{TypeId, TypeRegistry},
    module::{
        ExecutableModule, Installation, InstalledProgram, ModuleKey, ModuleStore, VerifiedProgram,
        dispatch::PublicationToken, layout_storage::StructStorageLayouts,
        managed::source::RevisionSource, staging::StagedProgram,
    },
    native::{
        completion::CompletionRegistry, conversion::arguments::ArgumentValues,
        function_handle::cache::FunctionCache, interfaces::binding::InterfaceCache,
        objects::cache::BindingCache, registry::NativeRegistry,
    },
    objects::{
        interfaces::cache::DescriptionCache, invocation::MethodInvocation, method::BoundReceiver,
    },
    observer::values::ObservedValues,
    reload::updates::ReloadAdmission,
    resource::{AsyncLimits, ResourceState, RuntimeLimits},
    scheduler::Scheduler,
    session::{
        CandidateSession, ExecutionEntry, ExecutionObserver, ExecutionOptions, ExecutionPhase,
        ExecutionSession, SessionScope, dependencies::Dependencies, group::GroupStore,
        store::SessionStorage,
    },
    task::store::TaskStore,
    value::{
        carriers::RootedValue,
        place::{PublishedResult, ResultPlace},
    },
};
mod authority;
mod clock;
pub mod loading;
mod objects;
mod observer;

use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::{ids::FunctionRef, standard::RuntimePrimitive};
use kagari_gc::{
    error::GcError,
    heap::{CollectionReport, HeapStats, Phase},
    root::Root,
};
use kagari_types::declaration::path::HostPathDeclaration;
use std::{
    cell::RefCell,
    sync::{Arc, OnceLock},
};

pub mod error_trace;
#[cfg(test)]
extern crate self as kagari_runtime;
pub mod backend;
pub mod builtin;
pub mod cache;
pub mod closure;
pub mod context;
pub mod debug;
pub mod debug_protocol;
#[cfg(feature = "execution-diagnostics")]
pub mod diagnostics;
pub mod error;
pub mod execution;
mod execution_metadata;
mod execution_state;
mod executor;
pub mod frame;
pub mod host;
pub mod host_scope;
#[cfg(test)]
mod interpreter_tests;
pub mod jit_abi;
pub mod metadata;
pub mod module;
pub mod native;
pub mod numeric;
pub mod program;
pub mod publication;
pub mod range;
pub mod reflection;
pub mod reload;
pub mod resource;
pub mod scheduler;
#[cfg(test)]
mod test_support;
mod value_check;

pub mod session;
pub mod source;
pub mod task;
pub mod value;
pub mod value_semantics;

/// Verified and linked reload data that has not changed any runtime entry.
struct PreparedReload {
    installation: Arc<Installation>,
    admission: ReloadAdmission,
    baseline: ExecutableModule,
    name: String,
    program: VerifiedProgram,
    bindings: module::LinkedProgramBindings,
}

/// Fully linked initial installation that is not yet visible as a live Program.
///
/// Use its token for fallible host association work before `Runtime::publish_staged_load`.
/// Dropping it abandons the existing candidate lease; ordinary collection reclaims
/// its storage. It exposes neither executable entry authority nor a live Program.
#[derive(Debug)]
#[must_use]
pub struct StagedLoad {
    installation: Arc<Installation>,
    program: StagedProgram,
}

impl StagedLoad {
    /// Exact precommit association identity, without code ownership or entry authority.
    /// Publication preserves this token; an abandoned token never becomes live.
    pub fn publication_token(&self) -> PublicationToken {
        self.program.module().publication_token()
    }
}

/// Installed candidate whose entry has not been activated.
#[derive(Debug)]
pub struct StagedReload {
    target: InstalledProgram,
    initialization_error: RefCell<Option<RuntimeError>>,
    baseline: ExecutableModule,
    program: StagedProgram,
}

impl StagedReload {
    pub fn initialization_error(&self) -> Option<RuntimeError> {
        self.initialization_error.borrow().clone()
    }

    pub(crate) fn record_initialization_error(&self, error: RuntimeError) {
        self.initialization_error.borrow_mut().get_or_insert(error);
    }

    pub fn module(&self) -> &ExecutableModule {
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
    default_context: ExecutionContext,
    scheduler: Scheduler,
    groups: GroupStore,
    dependencies: Arc<Dependencies>,
    tasks: TaskStore,
    operations: OnceLock<Result<CompletionRegistry, RuntimeError>>,
    async_limits: AsyncLimits,
    memory: ManagedMemory,
    metadata_ids: MetadataIds,
    resources: ResourceState,
    types: TypeRegistry,
    host: HostRegistry,
    native_entries: NativeRegistry,
    host_borrows: HostBorrowTable,

    modules: ModuleStore,
    structure_layouts: StructStorageLayouts,
    interpreter_caches: InterpreterCacheRegistry,
    object_bindings: BindingCache,
    function_bindings: FunctionCache,
    interface_bindings: InterfaceCache,
    interface_descriptions: DescriptionCache,
    observer: RefCell<Option<Box<dyn ExecutionObserver>>>,
    observed_values: RefCell<Option<ObservedValues>>,
}

/// A resolved dynamic method whose interface receiver stays rooted across
/// safepoints and synchronous host reentry.
#[derive(Debug, Clone)]
pub struct RootedInterfaceMethod {
    invocation: MethodInvocation,
    bound_receiver: BoundReceiver,
    _root: RootedValue,
    application_root: Option<Root<ApplicationObject>>,
    computation_root: Option<Root<ApplicationObject>>,
}

impl Runtime {
    pub(crate) fn definition_context(&self) -> &DefinitionContext {
        self.native_entries.catalog.types.context()
    }

    pub fn new(config: RuntimeConfig) -> Self {
        Self::with_native_registry(config, NativeRegistry::default(), None)
    }

    fn with_native_registry(
        config: RuntimeConfig,
        native_entries: NativeRegistry,
        host_types: Option<&PreparedHostTypes>,
    ) -> Self {
        let host = HostRegistry::new(native_entries.catalog.types.context().clone(), host_types);
        let resources = ResourceState::new(config.limits);
        let types = native_entries.metadata.runtime_registry();
        Self {
            default_context: ExecutionContext::default(),
            scheduler: Scheduler::new(config.async_limits),
            groups: GroupStore::default(),
            dependencies: Arc::new(Dependencies::new(config.async_limits)),
            tasks: TaskStore::new(config.async_limits),
            operations: OnceLock::new(),
            async_limits: config.async_limits,
            memory: ManagedMemory::new(config.gc),
            metadata_ids: MetadataIds::default(),
            resources,
            types,
            host,
            native_entries,
            host_borrows: HostBorrowTable::default(),

            modules: ModuleStore::default(),
            structure_layouts: StructStorageLayouts::default(),
            interpreter_caches: InterpreterCacheRegistry::default(),
            object_bindings: BindingCache::default(),
            function_bindings: FunctionCache::default(),
            interface_bindings: InterfaceCache::default(),
            interface_descriptions: DescriptionCache::default(),
            observer: RefCell::new(None),
            observed_values: RefCell::new(None),
        }
    }

    pub fn is_quarantined(&self) -> bool {
        self.resources().is_quarantined()
    }

    /// Report an invariant failure detected by an execution backend.
    pub fn quarantine_execution_invariant(&self, reason: EngineFault) -> RuntimeError {
        self.resources().quarantine(reason)
    }

    pub fn execution_root(&self) -> Option<ExecutableModule> {
        self.resources()
            .active_session()
            .map(|session| session.root.clone())
    }

    pub fn enter_execution_stack(
        &mut self,
        module: &ExecutableModule,
    ) -> Result<ExecutionStack<'_>, RuntimeError> {
        let session = self.begin_execution(module, self.execution_options())?;
        ExecutionStack::new(session)
    }

    /// Enter one source-backed closure selection without selecting it again.
    /// The supplying frame/Native/Host scope stays alive through frame publication.
    pub fn enter_closure_execution_stack(
        &mut self,
        closure: &SelectedClosure<'_>,
    ) -> Result<ExecutionStack<'_>, RuntimeError> {
        let session = self.begin_execution_inner(
            closure.owner(),
            self.execution_options(),
            ExecutionEntry::RetainedClosure,
            closure.origin(),
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
        let root = session.logical_root.get();
        drop(session);
        let root = self
            .resources()
            .sessions
            .get(root)
            .ok_or_else(|| RuntimeError::module_validation("logical invocation root"))?;
        Ok(root.next_random_u64())
    }

    pub fn begin_candidate_initialization<'candidate>(
        &mut self,
        candidate: &'candidate StagedReload,
    ) -> Result<CandidateSession<'_, 'candidate>, RuntimeError> {
        self.resources().ensure_not_publishing()?;
        self.reject_static_initialization("candidate session switch")?;
        if let Some(error) = candidate.initialization_error() {
            return Err(error);
        }
        self.validate_executable_module(candidate.module())?;
        if self.is_candidate_initialization() {
            return Err(RuntimeError::execution_phase_violation(
                "nested candidate initialization",
            ));
        }
        let mut options = self.execution_options();
        options.phase = ExecutionPhase::CandidateInitialization;
        let previous = self.resources().replace_session(None);
        match self.admit_execution(
            candidate.module(),
            options,
            ExecutionEntry::Candidate,
            RevisionSource::Host(candidate.program.revision()),
        ) {
            Ok(execution) => Ok(CandidateSession {
                candidate,
                runtime: self,
                execution: Some(execution),
                previous,
            }),
            Err(error) => {
                self.resources().replace_session(previous);
                candidate.record_initialization_error(error.clone());
                Err(error)
            }
        }
    }

    /// Begin or reenter execution through a matching actual admitted body, or
    /// through current publication when no such source exists. An old descriptor
    /// alone cannot reopen retired state. Ordinary inner calls still select current code.
    pub fn begin_execution(
        &mut self,
        module: &ExecutableModule,
        options: ExecutionOptions,
    ) -> Result<ExecutionSession<'_>, RuntimeError> {
        let origin = self.selection_origin(module)?;
        self.begin_execution_inner(module, options, ExecutionEntry::Program, origin)
    }

    fn begin_execution_inner(
        &mut self,
        module: &ExecutableModule,
        options: ExecutionOptions,
        entry: ExecutionEntry,
        origin: RevisionSource<'_>,
    ) -> Result<ExecutionSession<'_>, RuntimeError> {
        let scope = self
            .admit_execution(module, options, entry, origin)
            .map_err(|error| error.with_trace(self.capture_error_trace()))?;
        Ok(ExecutionSession {
            runtime: self,
            scope,
        })
    }

    fn admit_execution(
        &mut self,
        module: &ExecutableModule,
        options: ExecutionOptions,
        entry: ExecutionEntry,
        origin: RevisionSource<'_>,
    ) -> Result<SessionScope, RuntimeError> {
        let resources = &self.resources;
        let activation = resources.begin_synchronous_activation()?;
        self.ensure_scheduler_open()?;
        if self.modules.is_staged(module)
            && resources.active_session().is_none()
            && !matches!(entry, ExecutionEntry::Candidate)
        {
            return Err(RuntimeError::execution_phase_violation(
                "staged modules require the candidate session entry",
            ));
        }
        self.validate_executable_module(module)?;
        let phase = self
            .resources()
            .active_session()
            .map_or(options.phase, |session| session.options.phase);
        if self.modules.is_staged(module) && phase != ExecutionPhase::CandidateInitialization {
            return Err(RuntimeError::execution_phase_violation(
                "staged modules require candidate initialization execution",
            ));
        }

        let id = if let Some(session) = resources.active_session() {
            if options.phase == ExecutionPhase::CandidateInitialization
                && session.options.phase != ExecutionPhase::CandidateInitialization
            {
                return Err(RuntimeError::execution_phase_violation(
                    "candidate initialization requires an isolated root session",
                ));
            }
            // Ordinary reentry selects current code within the same installation.
            // Already prepared dependency calls retain their selected generation.
            let root_dependency = session.root.members().any(|member| member.id == module.id);
            let caller_dependency = self
                .resources()
                .sessions
                .frames(session.id)
                .ok_or_else(|| {
                    resources.quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
                })?
                .last()
                .is_some_and(|frame| {
                    frame
                        .loaded()
                        .members()
                        .any(|member| member.id == module.id)
                });
            if !root_dependency
                && !caller_dependency
                && !matches!(
                    entry,
                    ExecutionEntry::RetainedClosure | ExecutionEntry::RetainedFunction
                )
            {
                return Err(RuntimeError::module_validation(
                    "nested execution must use the same installation dependency program",
                ));
            }
            self.memory
                .scope(&self.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        origin.resolve(&self.modules, slots, module).map(|_| ())
                    })
                })
                .map_err(|error| match error {
                    GcError::ForeignHeap
                    | GcError::InvalidRoot
                    | GcError::Index
                    | GcError::Representation => RuntimeError::module_validation(
                        "foreign, expired or mismatched execution origin",
                    ),
                    error => self.managed_memory_error(error),
                })?;
            session.id
        } else {
            let registration = self
                .modules
                .register_execution(module)
                .ok_or_else(|| RuntimeError::module_validation("execution program was released"))?;
            // Resolve the original origin before growth, then publish the sole
            // body edge in the same closed source scope. No root pack or second
            // origin lookup is needed while creating the execution.
            let published = self.memory.scope(&self.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let revision = match origin.resolve(&self.modules, slots, module) {
                        Ok(revision) => revision,
                        Err(error) => return Ok(Err(error)),
                    };
                    let id = slots.enter(0, resources.stack_limit())?;
                    match slots.push_dependency(id, revision) {
                        Ok(body) => Ok(Ok((id, body))),
                        Err(error) => {
                            slots.leave(id)?;
                            Err(error)
                        }
                    }
                })
            });
            let (id, body) = published
                .map_err(|error| self.managed_memory_error(error))?
                .map_err(|error| match error {
                    GcError::ForeignHeap
                    | GcError::InvalidRoot
                    | GcError::Index
                    | GcError::Representation => RuntimeError::module_validation(
                        "foreign, expired or mismatched execution origin",
                    ),
                    error => self.managed_memory_error(error),
                })?;
            if let Err(error) = resources.sessions.insert(
                id,
                module.clone(),
                options,
                resources.counters(),
                registration,
                SessionStorage {
                    body,
                    space: self
                        .memory
                        .execution_space(id)
                        .map_err(|error| self.managed_memory_error(error))?,
                },
            ) {
                if let Err(cleanup) = self.memory.retire_execution(&self.resources, id) {
                    return Err(resources
                        .quarantine(EngineFault::ExecutionStorageRetirementFailed(cleanup)));
                }
                return Err(error);
            }
            resources.start_execution(id);
            id
        };
        let state = self
            .resources()
            .sessions
            .get(id)
            .expect("entered execution session");
        let scopes = state
            .scopes
            .get()
            .checked_add(1)
            .ok_or_else(|| resources.quarantine(EngineFault::ExecutionScopeCountOverflow))?;
        state.scopes.set(scopes);
        drop(state);
        let mut scope = SessionScope::new(id, activation.detach());
        if let Err(error) = resources.poll_execution() {
            scope.retire(self);
            return Err(error);
        }
        Ok(scope)
    }

    /// Observe the current managed collector without opening a payload loan.
    pub fn gc_stats(&self) -> HeapStats {
        self.memory.stats()
    }

    pub fn host(&self) -> &HostRegistry {
        &self.host
    }

    pub fn validate_host_borrow(
        &self,
        token: FrameHostBorrowToken,
        required: HostBorrowKind,
    ) -> Result<(), RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        self.host_borrows.validate(token, required)
    }

    pub fn host_scope(
        &mut self,
        values: &[RootedValue],
    ) -> Result<HostResourceScope<'_>, RuntimeError> {
        HostResourceScope::new(self, values)
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
        declaration: &DefinitionMetadata<HostPathDeclaration>,
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

    /// Resolve an explicitly installed manual path in this runtime's identity scope.
    /// Reject unknown or ambiguous contracts without creating a descriptor or binding.
    pub fn resolve_host_path(
        &self,
        declaration: &DefinitionMetadata<HostPathDeclaration>,
    ) -> Result<host::HostPathDescriptorId, RuntimeError> {
        self.host.resolve_path(declaration)
    }

    pub fn host_dirty_paths(&self) -> Vec<host::HostPathMutationRecord<RootedValue>> {
        self.host.dirty_paths()
    }

    pub fn clear_host_dirty_paths(&mut self) {
        self.host.clear_dirty_paths();
    }

    pub fn types(&self) -> &TypeRegistry {
        &self.types
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
        let executable = self.modules.loaded(module)?;
        Some(self.interpreter_caches.register(
            executable.installation_id(),
            module,
            function,
            dependencies,
        ))
    }

    pub fn interpreter_cache(&self, id: InterpreterCacheId) -> Option<InterpreterCacheRecord> {
        let artifact = self.interpreter_caches.get(id)?;
        self.modules.loaded(artifact.module)?;
        Some(artifact)
    }

    /// Finish collection through the sole execution-root driver. Payload/source
    /// loans must end before entry; provider failures quarantine the Runtime.
    pub fn collect_garbage(&mut self) -> Result<CollectionReport, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_not_publishing()?;
        self.resources.ensure_execution_allowed()?;
        self.modules.check_collection_access()?;
        self.flush_static_failures()?;
        let started = Instant::now();
        // Full collection always includes a fresh cycle, including when it first
        // finishes an active one. Concurrent later releases retain their own flag.
        let abandoned = self.modules.take_abandonment();
        let result = self.resources.collect_operation(|| {
            let mut report = self.memory.collect(&self.resources)?;
            self.modules.dispose_retired(abandoned)?;
            self.structure_layouts.prune();
            self.interface_descriptions.prune();
            report.elapsed = started.elapsed();
            Ok::<_, GcError>(report)
        });
        if abandoned && !matches!(&result, Ok(Ok(_))) {
            self.modules.restore_abandonment();
        }
        result?.map_err(|error| self.managed_memory_error(error))
    }

    /// Advance managed collection only at an admitted execution boundary.
    pub fn gc_safepoint(&mut self) -> Result<(), RuntimeError> {
        self.resources.poll_execution()?;
        let abandoned = if self.memory.automatic_collection_enabled() {
            self.modules.check_collection_access()?;
            self.modules.abandonment_pending()
        } else {
            false
        };
        if self.memory.service_requested(abandoned) {
            self.flush_static_failures()?;
            // A release during an already active cycle may have been marked.
            // Keep its fresh-cycle request until the next Idle -> Roots boundary.
            let acknowledge =
                self.memory.stats().phase == Phase::Idle && self.modules.take_abandonment();
            let result = self.resources.collect_operation(|| {
                self.memory.safepoint(&self.resources)?;
                self.modules.dispose_retired(acknowledge)?;
                if self.memory.stats().phase == Phase::Idle {
                    self.structure_layouts.prune();
                    self.interface_descriptions.prune();
                }
                Ok::<_, GcError>(())
            });
            if acknowledge && !matches!(&result, Ok(Ok(()))) {
                self.modules.restore_abandonment();
            }
            result?.map_err(|error| self.managed_memory_error(error))?;
        }
        Ok(())
    }

    /// Invalid public input handles are admission errors. Failures in an already
    /// admitted graph still use managed_memory_error and quarantine the Runtime.
    pub(crate) fn managed_input_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => RuntimeError::module_validation(error.to_string()),
            error => self.managed_memory_error(error),
        }
    }

    fn managed_memory_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::Allocation => self.resources.allocation_failure(),
            GcError::SourceLimit => self.resources.stack_overflow(),
            GcError::Limit(name) => self.resources.limit(name),
            GcError::Closed => RuntimeError::from(CancellationReason::RuntimeIsClosed),
            error => self
                .resources
                .quarantine(EngineFault::ManagedCollectionFailed(error)),
        }
    }

    /// Invoke a checked primitive value operation.
    pub fn invoke_standard_builtin<'result>(
        &mut self,
        owner: &ExecutableModule,
        intrinsic: RuntimePrimitive,
        args: ArgumentValues<'_>,
        destination: ResultPlace<'result>,
    ) -> Result<PublishedResult<'result>, BuiltinError> {
        builtin::invoke_standard(self, owner, intrinsic, args, destination)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}

#[cfg(test)]
mod tests;
