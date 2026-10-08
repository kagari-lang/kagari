//! Transactional scope/task admission; no factory or continuation runs here.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::asynchronous::SpawnOrigin,
    gc::roots::RootedValue,
    module::LoadedModule,
    native::binding::NativeResult,
    session::{ExecutionOptions, ExecutionPhase},
    task::{
        CancellationCause, ScopeId, SpawnError, TaskId,
        control::{ScopeControl, TaskDispatcher, TaskScopeOwner, TaskSignal},
        payload::{ScopePayload, TaskPayload},
        store::{ScopeRecord, TaskRecord, TaskState},
    },
    value::Value,
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
    pub fn create_task_scope(
        &self,
        owner: &LoadedModule,
        options: ExecutionOptions,
        dispatcher: Arc<dyn TaskDispatcher>,
    ) -> NativeResult<TaskScopeOwner> {
        self.require_idle_driver()?;
        self.validate_loaded_module(owner)?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "TaskScope creation",
            ));
        }
        let id = ScopeId(
            self.tasks
                .borrow_mut()
                .scopes
                .reserve()
                .map_err(|_| RuntimeError::resource_limit("task scopes"))?,
        );
        let result = (|| {
            let control = ScopeControl::new(id, dispatcher, &options.cancellation);
            let ty = Ty::NativeObject(NominalTy {
                declaration: self.task_role(NativeStorageLayout::TaskScope)?,
                arguments: vec![],
                associated_types: Default::default(),
            });
            let ty = self
                .resolve_type_arguments(owner, slice::from_ref(&ty))?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("TaskScope type"))?;
            let capability = self.allocate_task_payload(
                owner,
                ty,
                ScopePayload {
                    id,
                    control: control.clone(),
                },
            )?;
            self.tasks.borrow_mut().scopes.insert(
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
            self.tasks.borrow_mut().scopes.remove(id.0);
        }
        result
    }

    /// Admission errors are values; invalid capabilities/captures and terminated
    /// callers remain execution errors. Publication commits before returning Ok.
    pub fn spawn_task(
        &self,
        capability: &Value,
        factory: &Value,
    ) -> NativeResult<Result<RootedValue, SpawnError>> {
        self.resources().poll_execution()?;
        if self.execution_options().phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation("task spawn"));
        }
        let control = self.gc().scope_control(capability)?;
        if control.id.0.owner != self.tasks.borrow().owner {
            return Err(RuntimeError::module_validation("foreign TaskScope"));
        }
        if control.closing.load(Ordering::Acquire) {
            return Ok(Err(SpawnError::ScopeClosed));
        }
        if control.failed.load(Ordering::Acquire) {
            return Ok(Err(SpawnError::DispatchUnavailable));
        }
        let options = self
            .tasks
            .borrow()
            .scopes
            .get(control.id.0)
            .filter(|scope| Arc::ptr_eq(&scope.control, &control))
            .ok_or_else(|| RuntimeError::module_validation("retired TaskScope"))?
            .options
            .clone();
        let prepared = self.prepare_future_factory(factory)?;
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
        let id = match self.tasks.borrow_mut().tasks.reserve() {
            Ok(id) => TaskId(id),
            Err(error) => return Ok(Err(error)),
        };
        let value = match self.allocate_task_payload(
            &prepared.owner,
            task_type,
            TaskPayload { id, outcome: None },
        ) {
            Ok(value) => value,
            Err(error) => {
                self.tasks.borrow_mut().tasks.remove(id.0);
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
        let retained = self
            .root_value(value.value(self.gc()).expect("new Task root"))
            .expect("new Task retention root");
        let origin = SpawnOrigin {
            factory: prepared.origin,
            site: self.capture_async_site(),
        };
        self.tasks.borrow_mut().tasks.insert(
            id.0,
            TaskRecord {
                origin,
                scope: control.id,
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
            let discarded = self.tasks.borrow_mut().tasks.remove(id.0);
            drop(discarded);
            return Err(error);
        }
        let failure = if control.failed.load(Ordering::Acquire)
            || signal.cause() == Some(CancellationCause::DispatchFailure)
        {
            Some(SpawnError::DispatchUnavailable)
        } else if control.closing.load(Ordering::Acquire) {
            Some(SpawnError::ScopeClosed)
        } else {
            None
        };
        if let Some(error) = failure {
            signal.finished.store(true, Ordering::Release);
            self.tasks.borrow_mut().tasks.remove(id.0);
            return Ok(Err(error));
        }
        self.tasks
            .borrow_mut()
            .tasks
            .get_mut(id.0)
            .expect("admitting task")
            .state = TaskState::Queued(prepared);
        Ok(Ok(value))
    }

    pub(crate) fn validate_scope_owner(&self, scope: &TaskScopeOwner) -> NativeResult<()> {
        if self
            .tasks
            .borrow()
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
