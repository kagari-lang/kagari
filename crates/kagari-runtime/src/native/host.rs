//! External-ID service adaptation uses ordinary checked native entry and host scopes.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    host::{HostBorrowKind, HostBorrowTable, HostCallContext, HostError},
    native::{
        adapter::{Host, NativeAdapter},
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::CallContext,
    },
    session::store::SessionId,
    value::{EphemeralValue, Value},
};
use kagari_types::declaration::{
    FnDecl,
    host::{HostCallPolicy, HostPassingStyle, PathAccess},
};
use std::fmt::Write;

impl<F> NativeAdapter<Host, Value> for F
where
    F: for<'call> Fn(&HostCallContext<'call>, &[Value]) -> Result<Value, HostError>
        + Send
        + Sync
        + 'static,
{
    fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        let policy = declaration.host_policy.clone().ok_or_else(|| {
            RuntimeError::metadata_conflict(
                "host callback requires explicit external access policy",
            )
        })?;
        let proof = policy.clone();
        let mut binding = NativeBinding::new(
            vec![Codec::Value; declaration.params.len()],
            Codec::Value,
            move |cx| {
                let values = (0..cx.arguments.len())
                    .map(|index| cx.argument(index))
                    .collect::<NativeResult<Vec<_>>>()?;
                if !values
                    .iter()
                    .all(|value| cx.heap().validate_candidate_value(value))
                {
                    return Err(RuntimeError::execution_phase_violation(
                        "external object in candidate host arguments",
                    ));
                }
                cx.runtime.validate_native_host_policy(&policy)?;
                let trace = begin_trace(cx, &values)?;
                let result = (|| {
                    let context = HostCallContext::new(cx.runtime, &values)?;
                    check_arguments(cx, &values, &policy)?;
                    acquire_arguments(&context, &values, &policy)?;
                    let result = self(&context, &values)
                        .map_err(|error| {
                            let failure = RuntimeError::host_call_failure(error.message());
                            match error.trace() {
                                Some(trace) => failure.with_trace(trace.clone()),
                                None => failure,
                            }
                        })
                        .and_then(|value| check_result(cx, value));
                    context.runtime().resources().poll_execution()?;
                    let value = result?;
                    HostBorrowTable::validate_no_escape(cx.heap(), &value)?;
                    if !cx.heap().validate_value(&value) {
                        return Err(RuntimeError::host_call_failure(
                            "invalid heap reference in host result",
                        ));
                    }
                    Ok(value)
                })();
                if let Some((id, index)) = trace
                    && let Some(session) = cx.runtime.resources().sessions.get(id)
                {
                    session.finish_host_call(cx.heap(), index, &result);
                }
                result
            },
        );
        binding.host_policy = Some(proof);
        // The shared prepared result was checked above before scope release;
        // selected result adaptation still runs at ordinary native completion.
        binding.converted_result = true;
        Ok(binding)
    }
}

fn begin_trace(cx: &CallContext<'_>, values: &[Value]) -> NativeResult<Option<(SessionId, usize)>> {
    let Some(session) = cx.runtime.resources().active_session() else {
        return Ok(None);
    };
    if !session.options.record_host_calls {
        return Ok(None);
    }
    let definition = cx
        .owner
        .definitions()
        .resolve(cx.function.declaration)
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
    Ok(session
        .begin_host_call(cx.heap(), &symbol, values)
        .map(|index| (session.id, index)))
}

fn check_arguments(
    cx: &CallContext<'_>,
    values: &[Value],
    policy: &HostCallPolicy,
) -> NativeResult<()> {
    let signature = cx.function.type_signature()?;
    for ((value, expected), passing) in values.iter().zip(&signature.params).zip(&policy.passing) {
        if *passing == HostPassingStyle::Owned {
            HostBorrowTable::validate_no_escape(cx.heap(), value)?;
        }
        if !matches_external_value(cx, expected, value) {
            return Err(RuntimeError::host_call_failure(
                "host arguments do not match the declared signature",
            ));
        }
    }
    Ok(())
}

fn check_result(cx: &CallContext<'_>, value: Value) -> NativeResult<Value> {
    if !cx.heap().validate_candidate_value(&value) {
        return Err(RuntimeError::execution_phase_violation(
            "external object in candidate host result",
        ));
    }
    if !matches_external_value(cx, &cx.function.type_signature()?.result, &value) {
        return Err(RuntimeError::host_call_failure(
            "host result does not match the declared signature",
        ));
    }
    Ok(value)
}

// External borrow tokens are accepted only for declaration matching at this
// boundary. General value admission still forbids storing or returning them.
fn matches_external_value(cx: &CallContext<'_>, expected: &TypeArgument, value: &Value) -> bool {
    if !cx.heap().validate_value(value) {
        return false;
    }
    expected.matches(cx.runtime, value, cx.owner)
}

fn acquire_arguments(
    context: &HostCallContext<'_>,
    values: &[Value],
    policy: &HostCallPolicy,
) -> NativeResult<()> {
    let runtime = context.runtime();
    for (value, passing) in values.iter().zip(&policy.passing) {
        match (value, passing) {
            (
                Value::HostRoot(id),
                HostPassingStyle::SharedBorrow | HostPassingStyle::UniqueBorrow,
            ) => {
                let root = runtime
                    .gc()
                    .host_root(*id)
                    .ok_or_else(|| RuntimeError::host_call_failure("host borrow root"))?;
                match passing {
                    HostPassingStyle::SharedBorrow => {
                        context
                            .borrows()
                            .borrow_shared(root.object_id(), root.type_id())?;
                    }
                    _ => {
                        context
                            .borrows()
                            .borrow_unique(root.object_id(), root.type_id())?;
                    }
                }
            }
            (Value::Ephemeral(id), passing) => {
                let Some(EphemeralValue::HostRef(token) | EphemeralValue::HostMut(token)) =
                    runtime.gc().ephemeral(*id)
                else {
                    return Err(RuntimeError::host_call_failure("host borrow argument"));
                };
                let access = match passing {
                    HostPassingStyle::SharedBorrow => HostBorrowKind::Shared,
                    HostPassingStyle::UniqueBorrow => HostBorrowKind::Unique,
                    HostPassingStyle::Owned => {
                        return Err(RuntimeError::host_borrow_escape(
                            "borrowed host argument cannot satisfy owned passing",
                        ));
                    }
                };
                runtime.validate_host_borrow(token, access)?;
            }
            (Value::HostPathView(id), passing) => {
                let view = runtime
                    .gc()
                    .host_path(*id)
                    .ok_or_else(|| RuntimeError::host_call_failure("host borrow path view"))?;
                runtime.host().validate_view(runtime.gc(), &view)?;
                let root = view.root();
                match passing {
                    HostPassingStyle::SharedBorrow => {
                        context
                            .borrows()
                            .borrow_shared(root.object_id(), root.type_id())?;
                    }
                    HostPassingStyle::UniqueBorrow => {
                        if view.access() != PathAccess::ReadWrite {
                            return Err(RuntimeError::host_call_failure(
                                "host path view is readonly",
                            ));
                        }
                        context
                            .borrows()
                            .borrow_unique(root.object_id(), root.type_id())?;
                    }
                    HostPassingStyle::Owned => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}
