//! Library-authored Try and FromResidual bodies use ordinary enum operations.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols};
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
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    name: &str,
    success: &str,
    failure: &str,
) -> NativeResult<Value> {
    // SAFETY: The call frame retains the source and its immutable payload through result publication.
    let original = unsafe { cx.argument_transport(0) }?;
    let branch = enums::read(symbols, cx, &original, name, |member, fields| {
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
            return enums::allocate(symbols, cx, &result, "ControlFlow", "Continue", vec![value]);
        }
        ControlFlow::Break(payload) => payload,
    };
    let residual_type = cx.type_parameter(&result, 0)?;
    let residual = enums::allocate(
        symbols,
        cx,
        &residual_type,
        name,
        failure,
        payload.into_iter().collect(),
    )?;
    let _constructed = unsafe {
        // SAFETY: The input frame protects source payloads; allocated/forwarded results are rooted before further allocation or reentry.
        cx.heap().root_value_transport(residual)
    }
    .ok_or_else(invalid)?;
    let residual = if name == "Result" {
        // SAFETY: The original argument and constructed-value roots protect inputs; the result is
        // rooted or returned before reentry.
        unsafe { cx.forward_enum_origin_transport(&original, &residual) }?
    } else {
        residual
    };
    let _residual = unsafe {
        // SAFETY: The input frame protects source payloads; allocated/forwarded results are rooted before further allocation or reentry.
        cx.heap().root_value_transport(residual)
    }
    .ok_or_else(invalid)?;
    enums::allocate(symbols, cx, &result, "ControlFlow", "Break", vec![residual])
}

fn from_output(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    name: &str,
    success: &str,
) -> NativeResult<Value> {
    enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        name,
        success,
        // SAFETY: The call frame retains the source and its immutable payload through result publication.
        vec![unsafe { cx.argument_transport(0) }?],
    )
}

fn from_residual(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    name: &str,
    failure: &str,
) -> NativeResult<Value> {
    // SAFETY: The call frame retains the source and its immutable payload through result publication.
    let original = unsafe { cx.argument_transport(0) }?;
    let mut payload = enums::read(symbols, cx, &original, name, |member, fields| {
        match (member, fields) {
            (member, []) if member == failure && name == "Option" => Ok(None),
            (member, [value]) if member == failure && name != "Option" => Ok(Some(*value)),
            _ => Err(invalid()),
        }
    })??;
    if name == "Result" {
        let selected = cx.selected_at(0)?;
        // SAFETY: The original argument and constructed-value roots protect inputs; the result is
        // rooted or returned before reentry.
        payload = Some(unsafe {
            cx.call_values_transport(
                selected,
                slice::from_ref(payload.as_ref().ok_or_else(invalid)?),
            )
        }?);
    }
    let _payload = payload
        .as_ref()
        .map(|value| {
            unsafe {
                // SAFETY: The input frame protects source payloads; allocated/forwarded results are rooted before further allocation or reentry.
                cx.heap().root_value_transport(*value)
            }
            .ok_or_else(invalid)
        })
        .transpose()?;
    let result = enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        name,
        failure,
        payload.into_iter().collect(),
    )?;
    if name != "Result" {
        return Ok(result);
    }
    let _result = unsafe {
        // SAFETY: The input frame protects source payloads; allocated/forwarded results are rooted before further allocation or reentry.
        cx.heap().root_value_transport(result)
    }
    .ok_or_else(invalid)?;
    // SAFETY: The original argument and constructed-value roots protect inputs; the result is
    // rooted or returned before reentry.
    unsafe { cx.forward_enum_origin_transport(&original, &result) }
}

pub(crate) fn option_branch(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    branch(symbols, cx, "Option", "Some", "None")
}

pub(crate) fn result_branch(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    branch(symbols, cx, "Result", "Ok", "Err")
}

pub(crate) fn control_flow_branch(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    branch(symbols, cx, "ControlFlow", "Continue", "Break")
}

pub(crate) fn option_from_output(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    from_output(symbols, cx, "Option", "Some")
}

pub(crate) fn result_from_output(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    from_output(symbols, cx, "Result", "Ok")
}

pub(crate) fn control_flow_from_output(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    from_output(symbols, cx, "ControlFlow", "Continue")
}

pub(crate) fn option_from_residual(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    from_residual(symbols, cx, "Option", "None")
}

pub(crate) fn result_from_residual(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    from_residual(symbols, cx, "Result", "Err")
}

pub(crate) fn control_flow_from_residual(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
) -> NativeResult<Value> {
    from_residual(symbols, cx, "ControlFlow", "Break")
}
