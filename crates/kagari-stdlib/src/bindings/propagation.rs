//! Library-authored Try and FromResidual bodies use ordinary enum operations.
use crate::bindings::enums;
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext},
    value::Value,
};
use std::slice;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("propagation library contract")
}

fn branch(
    cx: &mut CallContext<'_>,
    name: &str,
    success: &str,
    failure: &str,
) -> NativeResult<Value> {
    let original = cx.argument(0)?;
    let (member, fields) = enums::inspect(cx, &original, name)?;
    let result = cx.result_type_argument()?;
    if member == success && fields.len() == 1 {
        return enums::allocate(cx, &result, "ControlFlow", "Continue", fields);
    }
    if member != failure || fields.len() != usize::from(name != "Option") {
        return Err(invalid());
    }
    let residual_type = cx.type_parameter(&result, 0)?;
    let residual = enums::allocate(cx, &residual_type, name, failure, fields)?;
    let _constructed = cx.heap().root_value(residual.clone()).ok_or_else(invalid)?;
    let residual = if name == "Result" {
        cx.forward_enum_origin(&original, &residual)?
    } else {
        residual
    };
    let _residual = cx.heap().root_value(residual.clone()).ok_or_else(invalid)?;
    enums::allocate(cx, &result, "ControlFlow", "Break", vec![residual])
}

fn from_output(cx: &mut CallContext<'_>, name: &str, success: &str) -> NativeResult<Value> {
    enums::allocate(
        cx,
        &cx.result_type_argument()?,
        name,
        success,
        vec![cx.argument(0)?],
    )
}

fn from_residual(cx: &mut CallContext<'_>, name: &str, failure: &str) -> NativeResult<Value> {
    let original = cx.argument(0)?;
    let (member, mut fields) = enums::inspect(cx, &original, name)?;
    if member != failure || fields.len() != usize::from(name != "Option") {
        return Err(invalid());
    }
    if name == "Result" {
        let selected = cx.selected_at(0)?;
        fields[0] = cx.call_values(selected, slice::from_ref(&fields[0]))?;
    }
    let _payload = fields
        .first()
        .map(|value| cx.heap().root_value(value.clone()).ok_or_else(invalid))
        .transpose()?;
    let result = enums::allocate(cx, &cx.result_type_argument()?, name, failure, fields)?;
    if name != "Result" {
        return Ok(result);
    }
    let _result = cx.heap().root_value(result.clone()).ok_or_else(invalid)?;
    cx.forward_enum_origin(&original, &result)
}

pub(super) fn option_branch(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    branch(cx, "Option", "Some", "None")
}

pub(super) fn result_branch(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    branch(cx, "Result", "Ok", "Err")
}

pub(super) fn control_flow_branch(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    branch(cx, "ControlFlow", "Continue", "Break")
}

pub(super) fn option_from_output(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    from_output(cx, "Option", "Some")
}

pub(super) fn result_from_output(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    from_output(cx, "Result", "Ok")
}

pub(super) fn control_flow_from_output(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    from_output(cx, "ControlFlow", "Continue")
}

pub(super) fn option_from_residual(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    from_residual(cx, "Option", "None")
}

pub(super) fn result_from_residual(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    from_residual(cx, "Result", "Err")
}

pub(super) fn control_flow_from_residual(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    from_residual(cx, "ControlFlow", "Break")
}
