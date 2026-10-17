//! Library-authored Try and FromResidual bodies use ordinary enum operations.
use crate::bindings::enums;
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext},
    value::Value,
};
use std::{ops::ControlFlow, slice};

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
    let branch = enums::read(cx, &original, name, |member, fields| {
        match (member, fields) {
            (member, [value]) if member == success => Ok(ControlFlow::Continue(*value)),
            (member, []) if member == failure && name == "Option" => Ok(ControlFlow::Break(None)),
            (member, [value]) if member == failure && name != "Option" => {
                Ok(ControlFlow::Break(Some(*value)))
            }
            _ => Err(invalid()),
        }
    })?;
    // Preserve result-type failure precedence over payload-shape rejection,
    // without keeping a heap borrow across type preparation or allocation.
    let result = cx.result_type_argument()?;
    let payload = match branch? {
        ControlFlow::Continue(value) => {
            return enums::allocate(cx, &result, "ControlFlow", "Continue", vec![value]);
        }
        ControlFlow::Break(payload) => payload,
    };
    let residual_type = cx.type_parameter(&result, 0)?;
    let residual = enums::allocate(
        cx,
        &residual_type,
        name,
        failure,
        payload.into_iter().collect(),
    )?;
    let _constructed = cx.heap().root_value(residual).ok_or_else(invalid)?;
    let residual = if name == "Result" {
        cx.forward_enum_origin(&original, &residual)?
    } else {
        residual
    };
    let _residual = cx.heap().root_value(residual).ok_or_else(invalid)?;
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
    let mut payload = enums::read(cx, &original, name, |member, fields| {
        match (member, fields) {
            (member, []) if member == failure && name == "Option" => Ok(None),
            (member, [value]) if member == failure && name != "Option" => Ok(Some(*value)),
            _ => Err(invalid()),
        }
    })??;
    if name == "Result" {
        let selected = cx.selected_at(0)?;
        payload = Some(cx.call_values(
            selected,
            slice::from_ref(payload.as_ref().ok_or_else(invalid)?),
        )?);
    }
    let _payload = payload
        .as_ref()
        .map(|value| cx.heap().root_value(*value).ok_or_else(invalid))
        .transpose()?;
    let result = enums::allocate(
        cx,
        &cx.result_type_argument()?,
        name,
        failure,
        payload.into_iter().collect(),
    )?;
    if name != "Result" {
        return Ok(result);
    }
    let _result = cx.heap().root_value(result).ok_or_else(invalid)?;
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
