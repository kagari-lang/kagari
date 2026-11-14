//! Formal shared access publishes an inert alias, preserving the caller's capability.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, ValueBank},
    native::{binding::NativeResult, context::NativeCall, conversion::arguments::ArgumentValues},
    value::{kind::ObjectKind, view::ValueView},
    value_check,
};
use kagari_gc::error::GcError;
use kagari_types::{declaration::host::HostPassingStyle, reference::ReferenceAccess, ty::Ty};

#[derive(Clone, Copy)]
enum SharedKind {
    ExternalHost,
    ScopedRust,
}

impl<'call> NativeCall<'call> {
    pub(crate) fn narrow_arguments(&mut self) -> NativeResult<()> {
        let original_arguments = self.arguments;
        let mut restricted = None;
        let signature = self.function.type_signature()?;
        for (index, parameter) in signature.params.iter().enumerate() {
            if index != 0 && index.is_multiple_of(1024) {
                self.poll()?;
            }
            let external_shared =
                self.function
                    .binding
                    .host_policy
                    .as_ref()
                    .is_some_and(|policy| {
                        policy.passing.get(index) == Some(&HostPassingStyle::SharedBorrow)
                            && matches!(parameter.ty(), Ty::Host(_))
                    });
            let shared = matches!(
                parameter.ty(),
                Ty::Reference {
                    access: ReferenceAccess::Shared,
                    ..
                }
            ) || (index == 0
                && self.function.binding.receiver_access == Some(ReferenceAccess::Shared))
                || external_shared;
            if !shared {
                continue;
            }
            let original = self.argument(index)?;
            if !matches!(
                original.kind(),
                Some(ObjectKind::Ephemeral | ObjectKind::HostPath)
            ) {
                continue;
            }
            let kind = if external_shared {
                SharedKind::ExternalHost
            } else {
                SharedKind::ScopedRust
            };
            // Live capability admission must precede logical matching, so an
            // expired borrow keeps its precise error even when already shared.
            let narrowed = self.shared_alias(original, kind)?;
            let matches =
                value_check::matches_protected(self.runtime, self.owner, parameter, &original)?;
            if !matches {
                // Nominal Rust receivers can be represented by a scoped Host
                // view. This does not broaden the owned Native value matcher.
                if !matches!(kind, SharedKind::ScopedRust)
                    || !self.runtime.matches_scoped_receiver(
                        &original,
                        parameter.ty(),
                        self.owner,
                        None,
                        ReferenceAccess::Shared,
                    )
                {
                    return Err(invalid());
                }
            }
            if let Some(narrowed) = narrowed {
                let arguments = match restricted {
                    Some(arguments) => arguments,
                    None => {
                        let arguments = self.copy_arguments(original_arguments)?;
                        restricted = Some(arguments);
                        arguments
                    }
                };
                let destination = arguments.location(index).ok_or_else(invalid)?;
                self.runtime
                    .memory
                    .scope(&self.runtime.resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| {
                            let value = narrowed.get(slots)?;
                            slots.set(destination, value)
                        })
                    })
                    .map_err(|error| self.gc_error(error))?;
            }
        }
        self.arguments = restricted.unwrap_or(original_arguments);
        Ok(())
    }

    fn shared_alias(
        &mut self,
        original: ValueView<'call>,
        kind: SharedKind,
    ) -> NativeResult<Option<ValueView<'call>>> {
        let execution = self.pending.execution();
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        memory
            .scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = original.get(slots)?;
                    let narrowed = match kind {
                        SharedKind::ExternalHost => host.narrow_argument_scoped(
                            slots.scope(),
                            value,
                            host_borrows,
                            resources,
                        )?,
                        SharedKind::ScopedRust => host_borrows.narrow_scoped(
                            slots.scope(),
                            value,
                            ReferenceAccess::Shared,
                        )?,
                    };
                    let value = match narrowed {
                        Ok(Some(value)) => value,
                        Ok(None) => return Ok(Ok(None)),
                        Err(error) => return Ok(Err(error)),
                    };
                    let location = slots.push_local(execution, value)?;
                    ValueView::protected(value, Some(location)).map(|view| Ok(Some(view)))
                })
            })
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }

    /// Only a changed capability needs a private parameter pack. Admission of
    /// all original coordinates precedes growth that could reuse a stale slot.
    fn copy_arguments(
        &mut self,
        arguments: ArgumentValues<'call>,
    ) -> NativeResult<ArgumentValues<'call>> {
        let execution = self.pending.execution();
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        memory
            .scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    arguments.check_bounds(slots)?;
                    for index in 0..arguments.len() {
                        if index != 0
                            && index.is_multiple_of(1024)
                            && let Err(error) = resources.poll_closed()
                        {
                            return Ok(Err(error));
                        }
                        arguments.scoped(slots, index)?;
                    }
                    let start = slots.len(execution, ValueBank::Local)?;
                    start
                        .checked_add(arguments.len())
                        .ok_or(GcError::Limit("native shared argument count"))?;
                    slots.reserve_locals(execution, arguments.len())?;
                    for index in 0..arguments.len() {
                        if index != 0
                            && index.is_multiple_of(1024)
                            && let Err(error) = resources.poll_closed()
                        {
                            return Ok(Err(error));
                        }
                        let value = arguments.scoped(slots, index)?;
                        slots.push_local(execution, value)?;
                    }
                    Ok(Ok(ArgumentValues::new(execution, start, arguments.len())))
                })
            })
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| self.runtime.resources.admit_execution_error(error))
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native shared argument capability")
}
