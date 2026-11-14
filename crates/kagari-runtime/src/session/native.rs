//! Native logical control contains code facts, never managed arguments or roots.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::{roots::SessionId, space},
    module::ExecutableModule,
};
use kagari_bytecode::instruction::NativeImportId;

#[derive(Debug)]
pub(crate) struct NativeFrame {
    pub(crate) owner: ExecutableModule,
    pub(crate) import: NativeImportId,
    pub(crate) depth: usize,
}

/// Own the temporary logical entry and depth guard through callback/error/unwind.
/// The NativeCall inside it independently owns the actual traced source suffix.
pub(crate) struct NativeActivation<'runtime> {
    runtime: &'runtime mut Runtime,
    session: SessionId,
    mark: usize,
    frame: Option<NativeFrame>,
    published: bool,
    #[cfg(feature = "execution-diagnostics")]
    completed: bool,
}

impl<'runtime> NativeActivation<'runtime> {
    pub(crate) fn enter(
        runtime: &'runtime mut Runtime,
        owner: ExecutableModule,
        import: NativeImportId,
        depth: usize,
    ) -> Result<Self, RuntimeError> {
        let (session, mark, reserve) = {
            let state = runtime.resources.active_session().ok_or_else(|| {
                RuntimeError::module_validation("logical Native call has no active execution")
            })?;
            let mut calls = state.native_calls.borrow_mut();
            (
                state.id,
                calls.len(),
                space::reserve(&mut calls, 1, &state.space),
            )
        };
        reserve.map_err(|error| runtime.managed_memory_error(error))?;
        runtime.resources.enter_call()?;
        Ok(Self {
            runtime,
            session,
            mark,
            frame: Some(NativeFrame {
                owner,
                import,
                depth,
            }),
            published: false,
            #[cfg(feature = "execution-diagnostics")]
            completed: false,
        })
    }

    /// Original parameter admission precedes the observable Native entry, as for
    /// a physical callee. No source/control loan crosses either operation.
    pub(crate) fn invoke<T>(
        mut self,
        validate: impl FnOnce(&mut Runtime) -> Result<(), RuntimeError>,
        invoke: impl FnOnce(&mut Runtime) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        validate(self.runtime)?;
        {
            let state = self
                .runtime
                .resources
                .sessions
                .get(self.session)
                .expect("active Native execution");
            state
                .native_calls
                .borrow_mut()
                .push(self.frame.take().expect("unpublished Native frame"));
        }
        self.published = true;
        let result = invoke(self.runtime).map_err(|error| {
            if error.trace().is_some() {
                error
            } else {
                error.with_trace(self.runtime.capture_error_trace())
            }
        });
        #[cfg(feature = "execution-diagnostics")]
        {
            self.completed = result.is_ok();
        }
        result
    }
}

impl Drop for NativeActivation<'_> {
    fn drop(&mut self) {
        if self.published {
            if let Some(state) = self.runtime.resources.sessions.get(self.session) {
                let mut calls = state.native_calls.borrow_mut();
                if calls.len() != self.mark + 1 {
                    self.runtime
                        .resources
                        .quarantine(EngineFault::HostInvariant("logical Native cleanup order"));
                }
                calls.truncate(self.mark);
            } else {
                self.runtime
                    .resources
                    .quarantine(EngineFault::HostInvariant(
                        "logical Native execution retired during callback",
                    ));
            }
        }
        self.runtime.resources.leave_call();
        #[cfg(feature = "execution-diagnostics")]
        if self.completed {
            diagnostics::record(Event::CallReturn);
        }
    }
}
