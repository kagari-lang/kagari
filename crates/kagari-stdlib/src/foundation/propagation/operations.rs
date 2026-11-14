//! Library-authored Try and FromResidual bodies use ordinary enum operations.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols};
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::NativeCall},
    value::view::ValueView,
};
use std::{ops::ControlFlow, slice};

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("propagation library contract")
}

fn branch<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    name: &str,
    success: &str,
    failure: &str,
) -> NativeResult<ValueView<'call>> {
    let original = cx.argument(0)?;
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
            return enums::allocate(symbols, cx, &result, "ControlFlow", "Continue", &[value]);
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
        payload.as_slice(),
    )?;
    let residual = if name == "Result" {
        cx.forward_enum_origin(&original, &residual)?
    } else {
        residual
    };
    enums::allocate(symbols, cx, &result, "ControlFlow", "Break", &[residual])
}

fn from_output<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    name: &str,
    success: &str,
) -> NativeResult<ValueView<'call>> {
    let value = cx.argument(0)?;
    enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        name,
        success,
        &[value],
    )
}

fn from_residual<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    name: &str,
    failure: &str,
) -> NativeResult<ValueView<'call>> {
    let original = cx.argument(0)?;
    let mut payload = enums::read(symbols, cx, &original, name, |member, fields| {
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
    let result = enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        name,
        failure,
        payload.as_slice(),
    )?;
    if name != "Result" {
        return Ok(result);
    }
    cx.forward_enum_origin(&original, &result)
}

pub(crate) fn option_branch<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    branch(symbols, cx, "Option", "Some", "None")
}

pub(crate) fn result_branch<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    branch(symbols, cx, "Result", "Ok", "Err")
}

pub(crate) fn control_flow_branch<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    branch(symbols, cx, "ControlFlow", "Continue", "Break")
}

pub(crate) fn option_from_output<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    from_output(symbols, cx, "Option", "Some")
}

pub(crate) fn result_from_output<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    from_output(symbols, cx, "Result", "Ok")
}

pub(crate) fn control_flow_from_output<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    from_output(symbols, cx, "ControlFlow", "Continue")
}

pub(crate) fn option_from_residual<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    from_residual(symbols, cx, "Option", "None")
}

pub(crate) fn result_from_residual<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    from_residual(symbols, cx, "Result", "Err")
}

pub(crate) fn control_flow_from_residual<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    from_residual(symbols, cx, "ControlFlow", "Break")
}
