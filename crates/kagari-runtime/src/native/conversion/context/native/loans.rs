//! Short, checked loans of source-protected Native payloads.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, ValueLocation},
    frame::types::arguments::TypeArgument,
    native::{binding::NativeResult, conversion::context::ConversionContext},
    resource::ResourceState,
    value::{kind::ObjectKind, native::NativeObject},
    value_check,
};
use kagari_gc::error::GcError;
use kagari_types::{reference::ReferenceAccess, ty::Ty};
use std::panic::resume_unwind;

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
    /// Callbacks receive polling state, not executing or GC services. Shared
    /// access uses no root, descriptor clone, NativeType or Host frame.
    pub(crate) fn borrow_native_argument<T: Send + 'static, R>(
        &mut self,
        receiver: NativeArgument<'_>,
        borrow: impl for<'loan> FnOnce(&ResourceState, NativeLoan<'loan, T>) -> NativeResult<R>,
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
        // scratch execution. Source admission checks heap/owner/generation;
        // this context allocates and retires only its own local suffix.
        self.execution_id()?;
        let Ty::NativeObject(nominal) = receiver.applied.ty() else {
            return Err(RuntimeError::module_validation("Native argument type"));
        };
        let owner = self.owner;
        let Runtime {
            memory,
            resources,
            native_entries,
            ..
        } = &mut *self.runtime;
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
                                    borrow(resources, NativeLoan::Shared(value))
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
                                    borrow(resources, NativeLoan::Mutable(value))
                                })
                            }
                        }
                    })
                })
            });
        // No payload or source loan remains while Runtime maps
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
