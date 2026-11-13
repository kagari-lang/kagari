//! External-ID services borrow existing Native inputs and publish before scope exit.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, SessionId},
    host::{FrameHostBorrowToken, HostBorrowKind, HostCallContext, HostError, HostRootHandle},
    module::ExecutableModule,
    native::{
        adapter::{Host, NativeAdapter},
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::NativeCall,
    },
    value::{
        EphemeralValue,
        host_values::{EphemeralObject, HostRootObject, paths::HostPathObject},
        kind::ObjectKind,
        properties::ValueProperties,
        view::ValueView,
    },
    value_check,
};
use kagari_gc::error::GcError;
use kagari_types::declaration::{
    FnDecl,
    host::{HostCallPolicy, HostPassingStyle, PathAccess},
};
use std::{fmt::Write, slice};

/// Bind an external-ID callback with one explicit call-lifetime signature. The
/// constructor supplies Rust's higher-ranked expectation before adapter inference;
/// the returned authoring value prepares the ordinary single Native entry.
pub fn callback(
    entry: impl for<'call> Fn(
        &mut HostCallContext<'call>,
        &[ValueView<'call>],
    ) -> Result<ValueView<'call>, HostError>
    + Send
    + Sync
    + 'static,
) -> impl NativeAdapter<Host, ()> {
    HostCallback(entry)
}

struct HostCallback<F>(F);

impl<F> NativeAdapter<Host, ()> for HostCallback<F>
where
    F: for<'call> Fn(
            &mut HostCallContext<'call>,
            &[ValueView<'call>],
        ) -> Result<ValueView<'call>, HostError>
        + Send
        + Sync
        + 'static,
{
    fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        let callback = self.0;
        let policy = declaration.host_policy.clone().ok_or_else(|| {
            RuntimeError::metadata_conflict(
                "host callback requires explicit external access policy",
            )
        })?;
        if policy.passing.len() != declaration.params.len() {
            return Err(RuntimeError::metadata_conflict(
                "host passing policy arity differs from declaration",
            ));
        }
        let proof = policy.clone();
        let mut binding = NativeBinding::scoped(
            vec![Codec::Value; declaration.params.len()],
            Codec::Value,
            move |call| {
                let mut values = Vec::new();
                values
                    .try_reserve_exact(call.arguments.len())
                    .map_err(|_| RuntimeError::resource_limit("Host argument views"))?;
                for index in 0..call.arguments.len() {
                    values.push(call.argument(index)?.reborrow());
                }
                value_check::candidate_protected(call.runtime, &values)?;
                call.runtime.validate_native_host_policy(&policy)?;
                let trace = begin_trace(call, &values)?;
                let owner = call.owner;
                let function = call.function;
                // The Native/caller prefix already protects these inputs. This
                // context owns Host loans and callback locals, not a copied pack.
                let published = (|| {
                    // Own the view list inside this invocation interval, so its
                    // invariant call lifetime cannot escape through a capture.
                    let values = values;
                    let place = call.pending_place()?;
                    let mut context = HostCallContext::new(call.runtime, &[])?;
                    context.validate_arguments(&values)?;
                    check_arguments(&mut context, owner, function, &values, &policy)?;
                    acquire_arguments(&mut context, &values, &policy)?;
                    let result = callback(&mut context, &values)
                        .map_err(RuntimeError::from_host)
                        .and_then(|value| {
                            check_result(context.runtime_mut(), owner, function, value)
                        });
                    // Preserve post-call termination before the callback's
                    // business error/result, including swallowed reentry traps.
                    context.runtime().resources.poll_execution()?;
                    let value = result?;
                    context.validate_result(&value)?;
                    let published = place.publish(context.runtime_mut(), &value)?;
                    // Successful snapshots need the still-live callback source.
                    context
                        .runtime_mut()
                        .finish_host_call_trace(trace, Ok(&value))?;
                    Ok::<_, RuntimeError>(published)
                })();
                match published {
                    // The pending cell already owns the result when Host locals
                    // and reservations end. Ordinary invocation stages once.
                    Ok(published) => call.accept_staged(published),
                    Err(error) => {
                        // Also finish failures before scope creation/publication.
                        // Error capture needs no retired source or heap access.
                        call.runtime.finish_host_call_trace(trace, Err(&error))?;
                        Err(error)
                    }
                }
            },
        );
        binding.host_policy = Some(proof);
        binding.converted_result = true;
        Ok(binding)
    }
}

fn begin_trace(
    call: &mut NativeCall<'_>,
    values: &[ValueView<'_>],
) -> NativeResult<Option<(SessionId, usize)>> {
    let owner = call.owner;
    let function = call.function;
    call.runtime
        .begin_host_call_trace(|| trace_symbol(owner, function), values)
}

fn trace_symbol(owner: &ExecutableModule, function: &LinkedNativeFunction) -> NativeResult<String> {
    let definition = owner
        .definitions()
        .resolve(function.declaration)
        .map_err(|_| RuntimeError::module_validation("host trace declaration identity"))?;
    let module = definition.module();
    let mut symbol = module.package.0.clone();
    for part in &module.path {
        write!(symbol, "::{part}").expect("String formatting is infallible");
    }
    for segment in definition.segments() {
        symbol.push_str("::");
        symbol.push_str(segment.name);
        if segment.occurrence != 0 {
            write!(symbol, "#{}", segment.occurrence).expect("String formatting is infallible");
        }
    }
    Ok(symbol)
}

fn check_arguments(
    context: &mut HostCallContext<'_>,
    owner: &ExecutableModule,
    function: &LinkedNativeFunction,
    values: &[ValueView<'_>],
    policy: &HostCallPolicy,
) -> NativeResult<()> {
    let signature = function.type_signature()?;
    for ((value, expected), passing) in values.iter().zip(&signature.params).zip(&policy.passing) {
        if *passing == HostPassingStyle::Owned {
            let properties = {
                let Runtime {
                    memory, resources, ..
                } = context.runtime_mut();
                memory.scope(resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let value = value.get(slots)?;
                        ValueProperties::of(slots.scope(), value)
                    })
                })
            }
            .map_err(|error| context.runtime().managed_memory_error(error))?;
            if properties.host_borrow {
                return Err(RuntimeError::host_borrow_escape(
                    "frame-scoped host borrow cannot outlive its call frame",
                ));
            }
        }
        if !value_check::matches_protected(context.runtime_mut(), owner, expected, value)? {
            return Err(RuntimeError::host_call_failure(
                "host arguments do not match the declared signature",
            ));
        }
    }
    Ok(())
}

fn check_result<'call>(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    function: &LinkedNativeFunction,
    value: ValueView<'call>,
) -> NativeResult<ValueView<'call>> {
    value_check::candidate_protected(runtime, slice::from_ref(&value))?;
    if !value_check::matches_protected(runtime, owner, &function.type_signature()?.result, &value)?
    {
        return Err(RuntimeError::host_call_failure(
            "host result does not match the declared signature",
        ));
    }
    Ok(value)
}

enum BorrowInput {
    Root(HostRootHandle),
    Token(FrameHostBorrowToken),
    None,
}

fn acquire_arguments(
    context: &mut HostCallContext<'_>,
    values: &[ValueView<'_>],
    policy: &HostCallPolicy,
) -> NativeResult<()> {
    for (value, passing) in values.iter().zip(&policy.passing) {
        let input = {
            let Runtime {
                memory, resources, ..
            } = context.runtime_mut();
            memory.scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?;
                    let Some(object) = value.object() else {
                        return Ok(Ok(BorrowInput::None));
                    };
                    Ok(match value.kind() {
                        Some(ObjectKind::HostRoot) if *passing != HostPassingStyle::Owned => {
                            let object = slots.scope().downcast::<HostRootObject>(object)?;
                            Ok(BorrowInput::Root(
                                slots.scope().read(object, HostRootObject::handle),
                            ))
                        }
                        Some(ObjectKind::HostPath) if *passing != HostPassingStyle::Owned => {
                            let object = slots.scope().downcast::<HostPathObject>(object)?;
                            let facts = slots.scope().read(object, HostPathObject::facts);
                            if *passing == HostPassingStyle::UniqueBorrow
                                && facts.access != PathAccess::ReadWrite
                            {
                                Err(RuntimeError::host_call_failure(
                                    "host path view is readonly",
                                ))
                            } else {
                                Ok(BorrowInput::Root(facts.root))
                            }
                        }
                        Some(ObjectKind::Ephemeral) if value.as_runtime_ephemeral().is_none() => {
                            let object = slots.scope().downcast::<EphemeralObject>(object)?;
                            match slots.scope().read(object, EphemeralObject::value) {
                                EphemeralValue::HostRef(token) | EphemeralValue::HostMut(token) => {
                                    Ok(BorrowInput::Token(token))
                                }
                                _ => Err(RuntimeError::host_call_failure("host borrow argument")),
                            }
                        }
                        _ => Ok(BorrowInput::None),
                    })
                })
            })
        }
        .map_err(|error: GcError| context.runtime().managed_memory_error(error))??;
        // Acquiring/validating a loan can report a traced error. The physical
        // source loan above has ended before entering any borrow-table service.
        match input {
            BorrowInput::Root(root) => match passing {
                HostPassingStyle::SharedBorrow => {
                    context
                        .borrows()
                        .borrow_shared(root.object_id(), root.type_id())?;
                }
                HostPassingStyle::UniqueBorrow => {
                    context
                        .borrows()
                        .borrow_unique(root.object_id(), root.type_id())?;
                }
                HostPassingStyle::Owned => unreachable!("owned root needs no automatic loan"),
            },
            BorrowInput::Token(token) => {
                let access = match passing {
                    HostPassingStyle::SharedBorrow => HostBorrowKind::Shared,
                    HostPassingStyle::UniqueBorrow => HostBorrowKind::Unique,
                    HostPassingStyle::Owned => {
                        return Err(RuntimeError::host_borrow_escape(
                            "borrowed host argument cannot satisfy owned passing",
                        ));
                    }
                };
                context.runtime().validate_host_borrow(token, access)?;
            }
            BorrowInput::None => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
