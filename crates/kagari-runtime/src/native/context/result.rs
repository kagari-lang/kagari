//! Native completion stays in the authoritative source until checked caller publication.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::{DependencyId, ExecutionRoots, NativeSourceMark, SessionId},
    execution_metadata::{
        applications::AdmittedApplication,
        managed::{ProtectedMetadata, applications::ApplicationObject, sources::MetadataSource},
    },
    module::{
        ExecutableModule,
        managed::{revision::RevisionState, source::RevisionSource},
    },
    native::{binding::NativeResult, context::NativeCall, conversion::arguments::ArgumentValues},
    value::{
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
};
use kagari_gc::{error::GcError, heap::Phase};
use std::ops::{Deref, DerefMut};

/// Short scratch lends share the actual invocation's pending result and source.
/// Only the admitting owner retires that Native suffix.
pub(crate) enum ReturnSource<'call> {
    Owned(PendingReturn),
    Borrowed(&'call mut PendingReturn),
}

impl Deref for ReturnSource<'_> {
    type Target = PendingReturn;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(value) => value,
            Self::Borrowed(value) => value,
        }
    }
}

impl DerefMut for ReturnSource<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Owned(value) => value,
            Self::Borrowed(value) => value,
        }
    }
}

impl ReturnSource<'_> {
    pub(super) fn retire_owned(&mut self, runtime: &mut Runtime) {
        if let Self::Owned(value) = self {
            value.retire(runtime);
        }
    }
}

/// The pending cell precedes this call's conversion suffix. Neither scalars nor
/// objects register a Host root, and cleanup consumes one complete Native mark.
pub(crate) struct PendingReturn {
    native: Option<NativeSourceMark>,
    body: DependencyId<RevisionState>,
    application: Option<DependencyId<ApplicationObject>>,
    staged: bool,
    invoked: bool,
}

impl PendingReturn {
    pub(super) fn new(
        runtime: &mut Runtime,
        owner: &ExecutableModule,
        body: RevisionSource<'_>,
        arguments: ArgumentValues<'_>,
        application: Option<&AdmittedApplication>,
    ) -> NativeResult<Self> {
        let execution = runtime
            .resources
            .active_session()
            .map(|session| session.id)
            .ok_or_else(|| {
                RuntimeError::module_validation("Native call has no active execution")
            })?;
        let Runtime {
            memory,
            resources,
            modules,
            ..
        } = runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                // Complete original admission precedes growth. Scalar inputs may
                // name no source; all prepared/managed inputs are caller-owned.
                arguments.check_bounds(slots)?;
                for index in 0..arguments.len() {
                    if index != 0 && index.is_multiple_of(1024) {
                        // Trace/termination admission must wait until this
                        // paired source loan and argument inspection have ended.
                        if let Err(error) = resources.poll_closed() {
                            return Ok(Err(error));
                        }
                    }
                    arguments.scoped(slots, index)?;
                }
                // Admit the original body coordinate before growth could reuse
                // a stale dependency slot. Metadata alone grants no GC access.
                let revision = body.resolve(modules, slots, owner)?;
                let application = application
                    .map(|application| {
                        let ProtectedMetadata::Application(value) =
                            MetadataSource::AdmittedApplication(application)
                                .resolve(modules, slots)?
                        else {
                            return Err(GcError::Representation);
                        };
                        Ok(value)
                    })
                    .transpose()?;
                // A synchronous same-execution caller keeps its admitted prefix
                // through this nested call. Reuse that protection, as for its
                // argument views; independent executions need their own edge.
                let inherited = match body {
                    RevisionSource::Execution(body) if body.execution() == execution => Some(body),
                    _ => None,
                };
                let native = slots.enter_native(execution)?;
                let body = match inherited {
                    Some(body) => body,
                    None => match slots.push_dependency(execution, revision) {
                        Ok(body) => body,
                        Err(error) => {
                            // No NativeCall exists yet to own cleanup. Undo the
                            // mark before propagating failed publication.
                            slots.retire_native(native)?;
                            return Err(error);
                        }
                    },
                };
                if let Err(error) = slots.set_native_origin(&native, body) {
                    slots.retire_native(native)?;
                    return Err(error);
                }
                let application = match application {
                    Some(value) => match slots.push_dependency(execution, value) {
                        Ok(source) => Some(source),
                        Err(error) => {
                            slots.retire_native(native)?;
                            return Err(error);
                        }
                    },
                    None => None,
                };
                Ok(Ok(Self {
                    native: Some(native),
                    body,
                    application,
                    staged: false,
                    invoked: false,
                }))
            })
        });
        result
            .map_err(|error| match error {
                GcError::ForeignHeap
                | GcError::InvalidRoot
                | GcError::Index
                | GcError::Representation => RuntimeError::module_validation(error.to_string()),
                error => runtime.managed_memory_error(error),
            })?
            .map_err(|error| runtime.resources.admit_execution_error(error))
    }

    pub(super) fn begin_invocation(&mut self) -> NativeResult<()> {
        if self.invoked {
            return Err(RuntimeError::module_validation(
                "Native invocation already started",
            ));
        }
        self.invoked = true;
        Ok(())
    }

    pub(crate) fn execution(&self) -> SessionId {
        self.body.execution()
    }

    pub(in crate::native) fn body(&self) -> DependencyId<RevisionState> {
        self.body
    }

    pub(in crate::native) fn application(&self) -> Option<DependencyId<ApplicationObject>> {
        self.application
    }

    pub(super) fn retire(&mut self, runtime: &mut Runtime) {
        let Some(native) = self.native.take() else {
            return;
        };
        match runtime.memory.stats().phase {
            Phase::Closed => return,
            Phase::Faulted => {
                if !runtime.resources.is_quarantined() {
                    runtime
                        .resources
                        .quarantine(EngineFault::ExecutionStorageRetirementFailed(
                            GcError::Faulted,
                        ));
                }
                return;
            }
            _ => {}
        }
        let result = runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.retire_native(native))
        });
        if let Err(error) = result
            && !runtime.resources.is_quarantined()
        {
            runtime
                .resources
                .quarantine(EngineFault::ExecutionStorageRetirementFailed(error));
        }
    }
}

impl<'call> NativeCall<'call> {
    /// Stage once without changing the caller's destination. The protected cell
    /// survives later conversion, collection and reentry until post-call checks.
    pub fn set_result(&mut self, value: ValueView<'_>) -> NativeResult<()> {
        let place = self.pending_place()?;
        let published = place.publish(self.runtime, &value)?;
        self.accept_staged(published)
    }

    /// Internal encoders publish before their child conversion owner retires.
    /// Issuing a place alone stages nothing; an encoding error permits recovery.
    pub(crate) fn pending_place(&self) -> NativeResult<ResultPlace<'call>> {
        if self.pending.staged {
            return Err(RuntimeError::module_validation(
                "native result already staged",
            ));
        }
        Ok(ResultPlace::new(
            self.pending
                .native
                .as_ref()
                .expect("live Native suffix")
                .result_location(),
        ))
    }

    pub(crate) fn accept_staged(&mut self, published: PublishedResult<'_>) -> NativeResult<()> {
        if self.pending.staged {
            return Err(RuntimeError::module_validation(
                "native result already staged",
            ));
        }
        published.admit(
            self.pending
                .native
                .as_ref()
                .expect("live Native suffix")
                .result_location(),
        )?;
        self.pending.staged = true;
        Ok(())
    }

    pub(crate) fn result(&mut self) -> NativeResult<ValueView<'call>> {
        if !self.pending.staged {
            return Err(RuntimeError::module_validation("native result is unset"));
        }
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let location = slots
                        .native_result(self.pending.native.as_ref().expect("live Native suffix"))?;
                    let value = slots.get(location)?;
                    ValueView::protected(value, Some(location))
                })
            })
            .map_err(|error| self.gc_error(error))
    }
}
