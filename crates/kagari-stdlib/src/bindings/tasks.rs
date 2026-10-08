//! Script Task methods use the same transactional admission/control path as hosts.
use crate::bindings::enums;
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext},
    task::SpawnError,
    value::Value,
};

pub(super) fn spawn(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let scope = cx.argument(0)?;
    let factory = cx.argument(1)?;
    let result = cx.result_type_argument()?;
    match cx.spawn_task(&scope, &factory)? {
        Ok(task) => {
            let value = task
                .value(cx.heap())
                .ok_or_else(|| RuntimeError::module_validation("spawned Task root"))?;
            enums::allocate(cx, &result, "Result", "Ok", vec![value])
        }
        Err(error) => {
            let ty = cx.type_parameter(&result, 1)?;
            let member = match error {
                SpawnError::ScopeClosed => "ScopeClosed",
                SpawnError::CapacityExceeded => "CapacityExceeded",
                SpawnError::DispatchUnavailable => "DispatchUnavailable",
            };
            let value = enums::allocate(cx, &ty, "SpawnError", member, vec![])?;
            let _error = cx
                .heap()
                .root_value(value.clone())
                .ok_or_else(|| RuntimeError::module_validation("SpawnError root"))?;
            enums::allocate(cx, &result, "Result", "Err", vec![value])
        }
    }
}

pub(super) fn cancel(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.cancel_task(&cx.argument(0)?)?;
    Ok(Value::Unit)
}
