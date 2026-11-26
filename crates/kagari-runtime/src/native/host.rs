//! External-ID services borrow existing Native inputs and publish before scope exit.
pub(crate) mod path;

use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, SessionId},
    host::{HostBorrowKind, HostCallContext, HostError},
    module::ExecutableModule,
    native::{
        binding::{Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        context::NativeCall,
    },
    value::{
        host_values::{HostRootObject, paths::HostPathObject},
        kind::ObjectKind,
        view::ValueView,
    },
    value_check,
};
use kagari_gc::error::GcError;
use kagari_types::declaration::host::{HostCallPolicy, HostPassingStyle, PathAccess};
use std::{fmt::Write, slice};

/// Bind an external-ID callback to its Rust-authored passing and effect policy.
/// Registration checks this binding against the declaration; executable metadata
/// cannot select or weaken the policy applied before callback entry.
pub fn callback(
    policy: HostCallPolicy,
    entry: impl for<'call> Fn(
        &mut HostCallContext<'call>,
        &[ValueView<'call>],
    ) -> Result<ValueView<'call>, HostError>
    + Send
    + Sync
    + 'static,
) -> NativeBinding {
    let mut binding = NativeBinding::scoped(
        vec![Codec::Value; policy.passing.len()],
        Codec::Value,
        move |call| {
            invoke_host(call, |cx, args| {
                entry(cx, args).map_err(RuntimeError::from_host)
            })
        },
    );
    binding.host_policy = Some(policy);
    binding.converted_result = true;
    binding
}

pub(super) fn invoke_host(
    call: &mut NativeCall<'_>,
    body: impl for<'call> FnOnce(
        &mut HostCallContext<'call>,
        &[ValueView<'call>],
    ) -> NativeResult<ValueView<'call>>,
) -> NativeResult<()> {
    let policy = call
        .function
        .binding
        .host_policy
        .as_ref()
        .ok_or_else(|| RuntimeError::module_validation("Host entry lacks installed policy"))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(call.arguments.len())
        .map_err(|_| RuntimeError::resource_limit("Host argument views"))?;
    for index in 0..call.arguments.len() {
        values.push(call.argument(index)?.reborrow());
    }
    value_check::candidate_protected(call.runtime, &values)?;
    call.runtime.validate_native_host_policy(policy)?;
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
        check_arguments(&mut context, owner, function, &values)?;
        acquire_arguments(&mut context, &values, policy)?;
        let result = body(&mut context, &values)
            .and_then(|value| check_result(context.runtime_mut(), owner, function, value));
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
) -> NativeResult<()> {
    let signature = function.type_signature()?;
    for (value, expected) in values.iter().zip(&signature.params) {
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

fn acquire_arguments(
    context: &mut HostCallContext<'_>,
    values: &[ValueView<'_>],
    policy: &HostCallPolicy,
) -> NativeResult<()> {
    for (value, passing) in values.iter().zip(&policy.passing) {
        let kind = match passing {
            HostPassingStyle::Owned => continue,
            HostPassingStyle::SharedBorrow => HostBorrowKind::Shared,
            HostPassingStyle::UniqueBorrow => HostBorrowKind::Unique,
        };
        let root = {
            let Runtime {
                memory, resources, ..
            } = context.runtime_mut();
            memory.scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?;
                    let Some(object) = value.object() else {
                        return Ok(Ok(None));
                    };
                    Ok(match value.kind() {
                        Some(ObjectKind::HostRoot) => {
                            let object = slots.scope().downcast::<HostRootObject>(object)?;
                            Ok(Some(slots.scope().read(object, HostRootObject::handle)))
                        }
                        Some(ObjectKind::HostPath) => {
                            let object = slots.scope().downcast::<HostPathObject>(object)?;
                            let facts = slots.scope().read(object, HostPathObject::facts);
                            if kind == HostBorrowKind::Unique
                                && facts.access != PathAccess::ReadWrite
                            {
                                Err(RuntimeError::host_call_failure(
                                    "host path view is readonly",
                                ))
                            } else {
                                Ok(Some(facts.root))
                            }
                        }
                        _ => Ok(None),
                    })
                })
            })
        }
        .map_err(|error: GcError| context.runtime().managed_memory_error(error))??;
        // Acquiring/validating a loan can report a traced error. The physical
        // source loan above has ended before entering any borrow-table service.
        if let Some(root) = root {
            context.borrow_root(root, kind)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
