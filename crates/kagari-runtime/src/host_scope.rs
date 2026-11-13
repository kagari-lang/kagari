//! Scoped Host records and temporaries share the complete execution root source.
pub mod scoped;
mod validation;
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::memory::ManagedMemory,
    execution::roots::{ExecutionRoots, LocalMark, SessionId, ValueBank, ValueLocation},
    host::{HostCallGuard, HostFrameMark},
    module::managed::source::RevisionSource,
    native::{binding::NativeResult, conversion::arguments::ArgumentValues},
    publication::ActivationMark,
    resource::ResourceState,
    session::{ExecutionEntry, ExecutionSession, SessionScope},
    value::{
        EphemeralValue,
        carriers::{RootedValue, ScopedValue},
        host_values::EphemeralObject,
        place::{PublishedResult, ResultPlace},
        strings::StringObject,
        view::ValueView,
    },
};
use kagari_gc::{error::GcError, heap::Phase};
use std::slice;

/// One temporary source suffix and Host reservation. Teardown revokes Host views
/// before retiring temporary values and the surrounding execution admission.
#[must_use]
pub struct HostResourceScope<'a> {
    runtime: &'a mut Runtime,
    activation: Option<ActivationMark>,
    borrows: Option<HostFrameMark>,
    execution: Option<SessionId>,
    owns_execution: bool,
    locals: Option<LocalMark>,
    session: Option<SessionScope>,
}

impl<'a> HostResourceScope<'a> {
    pub(crate) fn new(runtime: &'a mut Runtime, values: &[RootedValue]) -> NativeResult<Self> {
        runtime.resources().poll_execution()?;
        let body = runtime.resources().active_session().map(|state| state.body);
        let session = match runtime.execution_root() {
            Some(root) => Some(runtime.admit_execution(
                &root,
                runtime.execution_options(),
                ExecutionEntry::Program,
                RevisionSource::Execution(body.expect("execution root has its active source")),
            )?),
            None => None,
        };
        let activation = if session.is_none() {
            Some(runtime.resources().begin_synchronous_activation()?.detach())
        } else {
            None
        };
        let mut scope = Self {
            runtime,
            activation,
            borrows: None,
            execution: None,
            owns_execution: false,
            locals: None,
            session,
        };
        let execution = match scope.runtime.resources.active_session() {
            Some(session) => session.id,
            None => {
                let execution = scope
                    .runtime
                    .memory
                    .enter_execution(&scope.runtime.resources)
                    .map_err(|error| scope.gc_error(error))?;
                scope.execution = Some(execution);
                scope.owns_execution = true;
                execution
            }
        };
        scope.execution = Some(execution);
        scope.locals = Some(
            scope
                .runtime
                .memory
                .scope(&scope.runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(execution))
                })
                .map_err(|error| scope.gc_error(error))?,
        );
        let mark = scope
            .runtime
            .host_borrows
            .enter_frame_in(Some(scope.runtime.resources()))?
            .into_mark();
        scope.borrows = Some(mark);
        if let Some(session) = &scope.session {
            let state = session.state(scope.runtime.resources());
            let mut scopes = state.host_scopes.borrow_mut();
            scopes
                .try_reserve(1)
                .map_err(|_| scope.runtime.resources().limit("host scope capacity"))?;
            scopes.insert(scope.borrows().frame_id());
        }
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *scope.runtime;
        let checked = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                for (index, value) in values.iter().enumerate() {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let value = value.get(slots.scope())?;
                    if let Err(error) =
                        validation::validate(slots.scope(), value, host, host_borrows, resources)?
                    {
                        return Ok(Err(error));
                    }
                }
                slots.reserve_locals(execution, values.len())?;
                // Genuine roots are immutable and remain owned through this
                // closed handoff; reopen them for publication without repeating
                // semantic validation or allocating a second value pack.
                for (index, value) in values.iter().enumerate() {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let value = value.get(slots.scope())?;
                    slots.push_local(execution, value)?;
                }
                Ok(Ok(()))
            })
        });
        checked
            .map_err(|error| scope.gc_error(error))?
            .map_err(|error| scope.runtime.resources.admit_execution_error(error))?;
        Ok(scope)
    }

    pub fn runtime(&self) -> &Runtime {
        self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.runtime
    }

    pub(crate) fn execution_id(&self) -> SessionId {
        self.execution.expect("live Host source")
    }

    pub(crate) fn argument(
        &mut self,
        arguments: ArgumentValues<'_>,
        index: usize,
    ) -> NativeResult<ValueView<'a>> {
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| arguments.view(slots, index))
            });
        result.map_err(|error| self.gc_error(error))
    }

    pub fn borrows(&self) -> HostCallGuard<'_> {
        HostCallGuard::borrow_mark(
            &self.runtime.host_borrows,
            self.runtime.resources(),
            self.borrows.as_ref().expect("live Host scope"),
        )
    }

    /// Import genuine Host ownership into this scope's temporary source, without
    /// cloning/registering a root. Revocable Host capabilities remain checked.
    pub fn import(&mut self, root: &RootedValue) -> NativeResult<ValueView<'a>> {
        self.runtime.resources.ensure_execution_allowed()?;
        let execution = self.execution.expect("live Host source");
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = root.get(slots.scope())?;
                if let Err(error) =
                    validation::validate(slots.scope(), value, host, host_borrows, resources)?
                {
                    return Ok(Err(error));
                }
                let location = if value.object().is_some() {
                    Some(slots.push_local(execution, value)?)
                } else {
                    None
                };
                Ok(Ok(ValueView::protected(value, location)?))
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    /// Explicitly retain an ordinary result beyond this scope. Scoped Host and
    /// Runtime temporaries reject before registration; scalar escapes stay inline.
    pub fn retain(&mut self, value: &ValueView<'_>) -> NativeResult<RootedValue> {
        self.runtime.resources.ensure_execution_allowed()?;
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = value.get(slots)?;
                if let Err(error) =
                    validation::result(slots.scope(), value, host, host_borrows, resources)?
                {
                    return Ok(Err(error));
                }
                Ok(Ok(value.retain(slots.scope())?))
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    /// Borrow protected UTF-8 for a closed read. The callback cannot retain the
    /// text or execute Runtime services; it may copy a bounded prefix for output.
    /// Publication requires ending this payload read first:
    /// ```compile_fail,E0500
    /// use kagari_runtime::{host_scope::HostResourceScope, module::InstalledProgram,
    ///     value::view::ValueView};
    /// fn publish_during_read(
    ///     scope: &mut HostResourceScope<'_>, value: &ValueView<'_>, target: &InstalledProgram,
    /// ) {
    ///     scope.read_string(value, |_text| {
    ///         let _publication = scope.runtime_mut().begin_publication(target).unwrap();
    ///     }).unwrap();
    /// }
    /// ```
    pub fn read_string<R>(
        &mut self,
        value: &ValueView<'_>,
        read: impl for<'text> FnOnce(&'text str) -> R,
    ) -> NativeResult<R> {
        self.runtime.resources.ensure_execution_allowed()?;
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?;
                    StringObject::read(slots.scope(), value, read)
                })
            });
        result.map_err(|error| self.gc_error(error))
    }

    /// Authenticate an existing caller-owned pack without copying its protected
    /// values into this Host scope. The caller keeps its source through invocation.
    pub(crate) fn validate_values(&mut self, values: &[ValueView<'_>]) -> NativeResult<()> {
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                validation::values(slots, values, host, host_borrows, resources)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    pub(crate) fn validate_result(&mut self, value: &ValueView<'_>) -> NativeResult<()> {
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = value.get(slots)?;
                validation::result(slots.scope(), value, host, host_borrows, resources)
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    /// Copy a complete protected pack into this scope. Validate every original
    /// source before growth can reuse an expired coordinate named by a later one.
    pub fn retain_values(&mut self, values: &[ValueView<'_>]) -> NativeResult<ArgumentValues<'a>> {
        self.runtime.resources.ensure_execution_allowed()?;
        let execution = self.execution.expect("live Host source");
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                if let Err(error) =
                    validation::values(slots, values, host, host_borrows, resources)?
                {
                    return Ok(Err(error));
                }
                let start = slots.len(execution, ValueBank::Local)?;
                slots.reserve_locals(execution, values.len())?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_closed()
                    {
                        return Ok(Err(error));
                    }
                    let value = value.get(slots)?;
                    slots.push_local(execution, value)?;
                }
                Ok(Ok(ArgumentValues::new(execution, start, values.len())))
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    /// Copy an already protected view into this scope's own retention lifetime.
    pub fn protect(&mut self, value: &ValueView<'_>) -> NativeResult<ValueView<'a>> {
        let pack = self.retain_values(slice::from_ref(value))?;
        let location = pack.location(0).expect("one protected value");
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = slots.get(location)?;
                    ValueView::protected(value, Some(location))
                })
            });
        result.map_err(|error| self.gc_error(error))
    }

    fn ephemeral(&mut self, value: EphemeralValue) -> NativeResult<ValueView<'a>> {
        self.runtime.resources.ensure_execution_allowed()?;
        let execution = self.execution.expect("live Host source");
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = EphemeralObject::alloc(slots.scope(), value)?;
                    let location = if value.object().is_some() {
                        Some(slots.push_local(execution, value)?)
                    } else {
                        None
                    };
                    ValueView::protected(value, location)
                })
            });
        result.map_err(|error| self.gc_error(error))
    }

    fn gc_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => RuntimeError::module_validation(error.to_string()),
            error => self.runtime.managed_memory_error(error),
        }
    }

    fn revoke(
        runtime: &Runtime,
        frame: &mut Option<HostFrameMark>,
        session: Option<&SessionScope>,
    ) {
        if let Some(mark) = frame.take() {
            if let Some(session) = session {
                session
                    .state(runtime.resources())
                    .host_scopes
                    .borrow_mut()
                    .remove(
                        &HostCallGuard::borrow_mark(
                            &runtime.host_borrows,
                            runtime.resources(),
                            &mark,
                        )
                        .frame_id(),
                    );
            }
            drop(HostCallGuard::from_mark(
                &runtime.host_borrows,
                runtime.resources(),
                mark,
            ));
        }
    }

    fn retire_values(
        memory: &mut ManagedMemory,
        resources: &ResourceState,
        locals: &mut Option<LocalMark>,
        execution: Option<SessionId>,
        owns: bool,
    ) {
        match memory.stats().phase {
            Phase::Closed => {
                locals.take();
                return;
            }
            Phase::Faulted => {
                locals.take();
                cleanup_fault(resources, GcError::Faulted);
                return;
            }
            _ => {}
        }
        if let Some(mark) = locals.take() {
            let result = memory.scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| slots.retire_locals(mark))
            });
            if let Err(error) = result {
                cleanup_fault(resources, error);
            }
        }
        if owns
            && let Some(execution) = execution
            && let Err(error) = memory.retire_execution(resources, execution)
        {
            cleanup_fault(resources, error);
        }
    }
}

impl Drop for HostResourceScope<'_> {
    fn drop(&mut self) {
        let activation = self.activation.take();
        if let Some(scope) = self.session.take() {
            let session = ExecutionSession {
                runtime: self.runtime,
                scope,
            };
            Self::revoke(session.runtime(), &mut self.borrows, Some(&session.scope));
            Self::retire_values(
                &mut session.runtime.memory,
                &session.runtime.resources,
                &mut self.locals,
                self.execution,
                self.owns_execution,
            );
        } else {
            let _activation = activation.map(|mark| mark.attach(&self.runtime.resources));
            Self::revoke(self.runtime, &mut self.borrows, None);
            Self::retire_values(
                &mut self.runtime.memory,
                &self.runtime.resources,
                &mut self.locals,
                self.execution,
                self.owns_execution,
            );
        }
    }
}

fn cleanup_fault(resources: &ResourceState, error: GcError) {
    if !resources.is_quarantined() {
        resources.quarantine(EngineFault::ExecutionStorageRetirementFailed(error));
    }
}

/// Own only one invocation's local suffix. Success promotes its result to the
/// enclosing Host scope before arguments/preparation disappear; all other exits
/// retire the entire unpublished suffix, including callback unwind.
struct HostInvocation<'scope, 'runtime> {
    scope: &'scope mut HostResourceScope<'runtime>,
    mark: Option<LocalMark>,
}

impl<'runtime> HostInvocation<'_, 'runtime> {
    fn finish(
        &mut self,
        published: PublishedResult<'_>,
        expected: ValueLocation,
    ) -> NativeResult<ValueView<'runtime>> {
        let result = published.admit(expected)?;
        let mark = self.mark.as_mut().expect("live Host invocation");
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.scope.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(result)?;
                if let Err(error) =
                    validation::result(slots.scope(), value, host, host_borrows, resources)?
                {
                    return Ok(Err(error));
                }
                let location = slots.preserve_local(mark, value)?;
                Ok(Ok(ValueView::protected(value, Some(location))?))
            })
        });
        result
            .map_err(|error| self.scope.gc_error(error))?
            .map_err(|error| self.scope.runtime.resources.admit_execution_error(error))
    }
}

impl Drop for HostInvocation<'_, '_> {
    fn drop(&mut self) {
        HostResourceScope::retire_values(
            &mut self.scope.runtime.memory,
            &self.scope.runtime.resources,
            &mut self.mark,
            None,
            false,
        );
    }
}

impl<'runtime> HostResourceScope<'runtime> {
    pub(crate) fn execute<'scope>(
        &mut self,
        arguments: &[ValueView<'_>],
        invoke: impl for<'result> FnOnce(
            &mut Runtime,
            ArgumentValues<'scope>,
            ResultPlace<'result>,
        ) -> NativeResult<PublishedResult<'result>>,
    ) -> NativeResult<ValueView<'scope>>
    where
        'runtime: 'scope,
    {
        let execution = self.execution.expect("live Host source");
        let mark = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| slots.mark_locals(execution))
            })
            .map_err(|error| self.gc_error(error))?;
        let mut invocation = HostInvocation {
            scope: self,
            mark: Some(mark),
        };
        let arguments = invocation.scope.retain_values(arguments)?;
        let expected = invocation
            .scope
            .runtime
            .memory
            .scope(&invocation.scope.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    slots.push_local(execution, ScopedValue::unit())
                })
            })
            .map_err(|error| invocation.scope.gc_error(error))?;
        let published = invoke(
            invocation.scope.runtime,
            arguments.reborrow(),
            ResultPlace::new(expected),
        )?;
        invocation
            .finish(published, expected)
            .map(ValueView::reborrow)
    }
}
