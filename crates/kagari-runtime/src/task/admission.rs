//! Transactional scope/task admission; no factory or continuation runs here.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::asynchronous::SpawnOrigin,
    frame::factory::QueuedFactory,
    module::ExecutableModule,
    native::{binding::NativeResult, conversion::context::inputs::ValueInput},
    scheduler::{WorkKey, wake::WakeHub},
    session::{ExecutionOptions, ExecutionPhase},
    task::{
        CancellationCause, ScopeId, SpawnError, TaskId,
        control::{ScopeControl, TaskDispatcher, TaskScopeOwner, TaskSignal},
        payload::{ScopePayload, TaskPayload},
        store::{ScopeRecord, TaskRecord, TaskState},
    },
    value::carriers::RootedValue,
};
use kagari_types::{
    declaration::native::NativeStorageLayout,
    ty::{NominalTy, Ty},
};
use std::{
    slice,
    sync::{Arc, atomic::Ordering},
};

impl Runtime {
    pub fn create_dispatched_task_scope(
        &mut self,
        owner: &ExecutableModule,
        options: ExecutionOptions,
        dispatcher: Arc<dyn TaskDispatcher>,
    ) -> NativeResult<TaskScopeOwner> {
        self.create_task_scope_inner(owner, options, dispatcher, None)
    }

    pub(crate) fn create_task_scope_inner(
        &mut self,
        owner: &ExecutableModule,
        options: ExecutionOptions,
        dispatcher: Arc<dyn TaskDispatcher>,
        scheduler: Option<Arc<WakeHub>>,
    ) -> NativeResult<TaskScopeOwner> {
        self.ensure_scheduler_open()?;
        self.reject_static_initialization("TaskScope creation")?;
        self.require_idle_driver()?;
        self.validate_executable_module(owner)?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "TaskScope creation",
            ));
        }
        let id = ScopeId(
            self.tasks
                .scopes
                .reserve()
                .map_err(|_| RuntimeError::resource_limit("task scopes"))?,
        );
        let result = (|| {
            let control = ScopeControl::new(id, dispatcher, &options.cancellation, scheduler);
            let ty = Ty::NativeObject(NominalTy {
                declaration: self.task_role(NativeStorageLayout::TaskScope)?,
                arguments: vec![],
                associated_types: Default::default(),
            });
            let ty = self
                .type_arguments(owner, None, slice::from_ref(&ty))?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("TaskScope type"))?;
            let capability = self.allocate_task_payload(owner, ty, |cx| {
                ScopePayload::alloc(cx, control.status.clone())
            })?;
            self.tasks.scopes.insert(
                id.0,
                ScopeRecord {
                    control: control.clone(),
                    options,
                },
            );
            Ok(TaskScopeOwner {
                control,
                capability,
            })
        })();
        if result.is_err() {
            self.tasks.scopes.remove(id.0);
        }
        result
    }

    /// Admission errors are values; invalid capabilities/captures and terminated
    /// callers remain execution errors. Publication commits before returning Ok.
    pub fn spawn_task(
        &mut self,
        capability: &RootedValue,
        factory: &RootedValue,
    ) -> NativeResult<Result<RootedValue, SpawnError>> {
        self.spawn_task_inner(ValueInput::Root(capability), |runtime| {
            runtime.prepare_future_factory(factory)
        })
    }

    pub(crate) fn spawn_task_inner(
        &mut self,
        capability: ValueInput<'_, '_>,
        prepare: impl FnOnce(&mut Self) -> NativeResult<QueuedFactory>,
    ) -> NativeResult<Result<RootedValue, SpawnError>> {
        self.ensure_scheduler_open()?;
        self.resources().poll_execution()?;
        self.reject_static_initialization("task spawn")?;
        if self.execution_options().phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation("task spawn"));
        }
        let status = self.scope_status(capability)?;
        if status.id.0.owner != self.tasks.owner {
            return Err(RuntimeError::module_validation("foreign TaskScope"));
        }
        if status.closing.load(Ordering::Acquire) {
            return Ok(Err(SpawnError::ScopeClosed));
        }
        if status.failed.load(Ordering::Acquire) {
            return Ok(Err(SpawnError::DispatchUnavailable));
        }
        let scope = self
            .tasks
            .scopes
            .get(status.id.0)
            .filter(|scope| Arc::ptr_eq(&scope.control.status, &status))
            .ok_or_else(|| RuntimeError::module_validation("retired TaskScope"))?;
        let options = scope.options.clone();
        let control = scope.control.clone();
        let prepared = prepare(self)?;
        let output = prepared.future.parameter(self, &prepared.owner, 0)?;
        let declaration = self.task_role(NativeStorageLayout::Task)?;
        let task_type = prepared.future.derive(self, &prepared.owner, |ty| {
            let Ty::NativeObject(future) = ty else {
                return None;
            };
            let mut task = future.clone();
            task.declaration = declaration;
            Some(Ty::NativeObject(task))
        })?;
        if control.scheduler.is_some() && self.scheduler.reserve_work().is_err() {
            return Ok(Err(SpawnError::CapacityExceeded));
        }
        let id = match self.tasks.tasks.reserve() {
            Ok(id) => TaskId(id),
            Err(error) => return Ok(Err(error)),
        };
        let value = match self
            .allocate_task_payload(&prepared.owner, task_type, |cx| TaskPayload::alloc(cx, id))
        {
            Ok(value) => value,
            Err(error) => {
                self.tasks.tasks.remove(id.0);
                // A heap limit may have terminated the calling execution. An
                // admission Result cannot turn that sticky failure into success.
                self.resources().poll_execution()?;
                return if error.kind() == RuntimeErrorKind::ResourceLimitExceeded {
                    Ok(Err(SpawnError::CapacityExceeded))
                } else {
                    Err(error)
                };
            }
        };
        let signal = TaskSignal::new(id, &control);
        control.attach(&signal);
        let retained = value.clone();
        let origin = SpawnOrigin {
            factory: prepared.origin(),
            site: self.capture_async_site(),
        };
        self.tasks.tasks.insert(
            id.0,
            TaskRecord {
                origin,
                scope: control.status.id,
                signal: signal.clone(),
                value: retained,
                owner: prepared.owner.clone(),
                output,
                options: ExecutionOptions {
                    cancellation: signal.cancellation.clone(),
                    ..options
                },
                state: TaskState::Admitting,
            },
        );
        signal.mark_ready();
        if let Err(error) = self.resources().poll_execution() {
            signal.finished.store(true, Ordering::Release);
            let discarded = self.tasks.tasks.remove(id.0);
            drop(discarded);
            return Err(error);
        }
        let failure = if control.status.failed.load(Ordering::Acquire)
            || signal.cause() == Some(CancellationCause::DispatchFailure)
        {
            Some(SpawnError::DispatchUnavailable)
        } else if control.status.closing.load(Ordering::Acquire) {
            Some(SpawnError::ScopeClosed)
        } else {
            None
        };
        if let Some(error) = failure {
            signal.finished.store(true, Ordering::Release);
            self.tasks.tasks.remove(id.0);
            return Ok(Err(error));
        }
        self.tasks
            .tasks
            .get_mut(id.0)
            .expect("admitting task")
            .state = TaskState::Queued(prepared);
        if control.scheduler.is_some() {
            self.scheduler.enqueue(WorkKey::Task(id));
        }
        Ok(Ok(value))
    }

    pub(crate) fn validate_scope_owner(&self, scope: &TaskScopeOwner) -> NativeResult<()> {
        if self
            .tasks
            .scopes
            .get(scope.id().0)
            .is_none_or(|record| !Arc::ptr_eq(&record.control, &scope.control))
        {
            return Err(RuntimeError::module_validation(
                "foreign or retired scope owner",
            ));
        }
        Ok(())
    }
}
