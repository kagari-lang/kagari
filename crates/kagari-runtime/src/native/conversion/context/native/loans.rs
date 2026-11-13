//! Source-owned Native arguments share a closed loan interval with Host places.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, ValueLocation},
    frame::types::arguments::TypeArgument,
    host::{HostCallGuard, scoped::ScopedHostView},
    native::{binding::NativeResult, conversion::context::ConversionContext},
    resource::ResourceState,
    value::{kind::ObjectKind, native::NativeObject},
    value_check,
};
use kagari_gc::error::GcError;
use kagari_types::{reference::ReferenceAccess, ty::Ty};
use std::{any::TypeId, panic::resume_unwind, ptr};

/// The invocation owns this immutable source prefix and the prepared type fact.
/// Neither the coordinate nor its copied metadata grants a Rust payload loan.
pub(crate) struct NativeArgument<'a> {
    pub(crate) value: ValueLocation,
    pub(crate) applied: &'a TypeArgument,
    pub(crate) access: ReferenceAccess,
}

pub(crate) enum NativeLoan<'loan, T> {
    Shared(&'loan T),
    Mutable(&'loan mut T),
}

impl<'loan, T> NativeLoan<'loan, T> {
    pub(crate) fn shared(self) -> NativeResult<&'loan T> {
        match self {
            Self::Shared(value) => Ok(value),
            Self::Mutable(_) => Err(invalid_access()),
        }
    }

    pub(crate) fn mutable(self) -> NativeResult<&'loan mut T> {
        match self {
            Self::Mutable(value) => Ok(value),
            Self::Shared(_) => Err(invalid_access()),
        }
    }
}

impl ConversionContext<'_> {
    /// Admit all external loans before constructing either reference. Runtime is
    /// split by owner: callbacks receive polling state, not executing/GC services.
    /// Shared access uses no root, descriptor clone, NativeType or Host frame.
    pub(crate) fn borrow_native_argument<T: Send + 'static, R, const N: usize>(
        &mut self,
        receiver: NativeArgument<'_>,
        requests: [(ScopedHostView, TypeId, ReferenceAccess); N],
        borrow: impl for<'loan> FnOnce(
            &ResourceState,
            NativeLoan<'loan, T>,
            [*mut (); N],
        ) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.poll()?;
        self.runtime.resources.ensure_execution_allowed()?;
        receiver.applied.validate(self.runtime)?;
        let value = self
            .runtime
            .memory
            .inspect_value(receiver.value)
            .map_err(|error| self.gc_error(error))?;
        value_check::candidate_protected(self.runtime, &[value])?;
        // Inputs may belong to a live caller source rather than this callee's
        // scratch execution. Paired source admission checks heap/owner/generation;
        // this context allocates and retires only its own local suffix.
        self.execution_id()?;
        let Ty::NativeObject(nominal) = receiver.applied.ty() else {
            return Err(RuntimeError::module_validation("Native argument type"));
        };
        let (mark, pointers) = if N == 0 {
            (None, [ptr::null_mut(); N])
        } else {
            let (mark, pointers) = self
                .runtime
                .host_borrows
                .prepare_scoped_loans(self.runtime, requests)?;
            (Some(mark), pointers)
        };
        let owner = self.owner;
        let Runtime {
            memory,
            resources,
            host_borrows,
            native_entries,
            ..
        } = &mut *self.runtime;
        // Install cleanup immediately after taking the linear mark; every error
        // below, including malformed storage, releases this admitted Host pack.
        let loans = mark.map(|mark| HostCallGuard::from_mark(host_borrows, resources, mark));
        let result = native_entries
            .storage
            .get_id(nominal.declaration)
            .ok_or(GcError::Representation)
            .and_then(|storage| {
                memory.scope(resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let value = slots.get(receiver.value)?;
                        if value.kind() != Some(ObjectKind::Native) {
                            return Err(GcError::Representation);
                        }
                        let object = slots.scope().downcast::<NativeObject>(
                            value.object().ok_or(GcError::Representation)?,
                        )?;
                        if !slots.scope().read(object, |object| {
                            storage.matches_descriptor(object.storage())
                                && object.matches(receiver.applied.view(owner))
                        }) {
                            return Err(GcError::Representation);
                        }
                        let payload = NativeObject::payload(slots.scope(), object);
                        match receiver.access {
                            ReferenceAccess::Shared => storage
                                .read(slots.scope(), payload, |value| {
                                    borrow(resources, NativeLoan::Shared(value), pointers)
                                })
                                .map(Ok),
                            ReferenceAccess::Mutable => {
                                if !storage.is_editable_data() || !storage.accepts_payload::<T>() {
                                    return Err(GcError::Representation);
                                }
                                if slots
                                    .scope()
                                    .read(object, |object| object.revision() == u64::MAX)
                                {
                                    return Ok(Ok(Err(RuntimeError::module_validation(
                                        "native revision exhausted",
                                    ))));
                                }
                                // Completed writes and their revision survive user Err
                                // or panic. DataObject catches inside the GC mutation.
                                NativeObject::advance_revision(slots.scope(), object)?;
                                storage.edit_data(slots.scope(), payload, |value| {
                                    borrow(resources, NativeLoan::Mutable(value), pointers)
                                })
                            }
                        }
                    })
                })
            });
        drop(loans);
        // No payload/source or Host reservation loan remains while Runtime maps
        // core failures or resumes the original user panic.
        match result.map_err(|error| self.gc_error(error))? {
            Ok(result) => result,
            Err(panic) => resume_unwind(panic),
        }
    }
}

fn invalid_access() -> RuntimeError {
    RuntimeError::module_validation("Native loan access mismatch")
}
