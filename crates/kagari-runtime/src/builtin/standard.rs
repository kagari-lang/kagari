//! Runtime-owned primitives consume source arguments and publish through their caller.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::{ExecutionRoots, Slots},
    module::linked_execution::builtin_results::BuiltinResult,
    native::{
        binding::NativeResult,
        conversion::{arguments::ArgumentValues, context::ConversionContext},
    },
    resource::ResourceState,
    value::{
        arrays::ArrayObject, carriers::ScopedValue, kind::ObjectKind, strings::StringObject,
        view::ValueView,
    },
    value_semantics::{comparison, formatting, hashing, storage_error},
};
use kagari_abi::representation::ValueType;
use kagari_contract::standard::RuntimePrimitive;
use kagari_gc::{error::GcError, object::Gc};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::cmp::Ordering;

#[cfg(test)]
mod tests;

pub(super) fn invoke<'call>(
    cx: &mut ConversionContext<'call>,
    primitive: RuntimePrimitive,
    arguments: ArgumentValues<'call>,
) -> NativeResult<ValueView<'call>> {
    match primitive {
        RuntimePrimitive::ValuePartialCmp | RuntimePrimitive::ValueCmp => {
            arity(arguments, 2, TrapReason::ComparisonRequiresTwoOperands)?;
            let order = inspect(cx, arguments, |slots, resources| {
                let a = arguments.scoped(slots, 0).map_err(input_error)?;
                let b = arguments.scoped(slots, 1).map_err(input_error)?;
                comparison::builtin_order(slots.scope(), resources, a, b)
            })?;
            if primitive == RuntimePrimitive::ValueCmp && order.is_none() {
                return Err(TrapReason::TotalComparisonCannotBeUnordered.into());
            }
            let applied = cx
                .runtime()
                .builtin_result_type(cx.owner(), BuiltinResult::Ordering)?;
            let value = order
                .map(|order| {
                    cx.allocate_enum_member(
                        &applied,
                        match order {
                            Ordering::Less => "Less",
                            Ordering::Equal => "Equal",
                            Ordering::Greater => "Greater",
                        },
                        &[],
                    )
                })
                .transpose()?;
            if primitive == RuntimePrimitive::ValuePartialCmp {
                let option = cx
                    .runtime()
                    .builtin_result_type(cx.owner(), BuiltinResult::OptionalOrdering)?;
                match value {
                    Some(value) => cx.allocate_enum_member(&option, "Some", &[value]),
                    None => cx.allocate_enum_member(&option, "None", &[]),
                }
            } else {
                value.ok_or_else(|| TrapReason::TotalComparisonCannotBeUnordered.into())
            }
        }
        RuntimePrimitive::ValueEq => {
            arity(arguments, 2, TrapReason::EqExpectsTwoOperands)?;
            inspect(cx, arguments, |slots, resources| {
                let a = arguments.scoped(slots, 0).map_err(input_error)?;
                let b = arguments.scoped(slots, 1).map_err(input_error)?;
                comparison::script_equal(slots.scope(), resources, a, b)
            })
            .map(ValueView::from)
        }
        RuntimePrimitive::ValueHash => {
            arity(arguments, 1, TrapReason::HashExpectsOneOperand)?;
            let hash = inspect(cx, arguments, |slots, resources| {
                let value = arguments.scoped(slots, 0).map_err(input_error)?;
                hashing::builtin_hash(slots.scope(), resources, value)
            })?
            .ok_or_else(|| {
                RuntimeError::from(TrapReason::ValueHasNoHashSemanticsOrExceedsKeySizeLimit)
            })?;
            Ok(ValueView::from(hash))
        }
        RuntimePrimitive::ValueDebug | RuntimePrimitive::ValueDisplay => {
            arity(arguments, 1, TrapReason::FormatExpectsOneOperand)?;
            let text = inspect(cx, arguments, |slots, resources| {
                let value = arguments.scoped(slots, 0).map_err(input_error)?;
                formatting::format_value(
                    slots.scope(),
                    &mut || resources.poll_execution(),
                    value,
                    primitive == RuntimePrimitive::ValueDebug,
                )
            })?;
            cx.allocate_string(text)
        }
        RuntimePrimitive::StringPartsJoin => {
            arity(
                arguments,
                2,
                TrapReason::ArrayJoinExpectsAStringArrayAndSeparator,
            )?;
            let text = inspect(cx, arguments, |slots, resources| {
                array_join(slots, resources, arguments)
            })?;
            cx.allocate_string(text)
        }
        RuntimePrimitive::Assert => {
            arity(
                arguments,
                2,
                TrapReason::DebugAssertExpectsBoolAndStringMessage,
            )?;
            inspect(cx, arguments, |slots, resources| {
                debug_assert(slots, resources, arguments)
            })?;
            Ok(ValueView::unit())
        }
    }
}

fn arity(arguments: ArgumentValues<'_>, expected: usize, reason: TrapReason) -> NativeResult<()> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(reason.into())
    }
}

fn inspect<R>(
    cx: &mut ConversionContext<'_>,
    arguments: ArgumentValues<'_>,
    inspect: impl for<'source, 'gc> FnOnce(&mut Slots<'source, 'gc>, &ResourceState) -> NativeResult<R>,
) -> NativeResult<R> {
    let Runtime {
        memory, resources, ..
    } = cx.runtime_mut();
    let result = memory.scope(resources, |scope, roots| {
        ExecutionRoots::with(roots, scope, |slots| {
            arguments.check_bounds(slots)?;
            Ok(inspect(slots, resources))
        })
    });
    result.map_err(input_error)?
}

fn input_error(error: GcError) -> RuntimeError {
    match error {
        GcError::ForeignHeap | GcError::InvalidRoot | GcError::Index | GcError::Representation => {
            RuntimeError::module_validation("invalid primitive argument")
        }
        error => storage_error(error),
    }
}

fn string<'gc>(
    slots: &mut Slots<'_, 'gc>,
    value: ScopedValue<'gc>,
    reason: TrapReason,
) -> NativeResult<Gc<'gc, StringObject>> {
    if value.kind() != Some(ObjectKind::String) {
        return Err(reason.into());
    }
    slots
        .scope()
        .downcast(
            value
                .object()
                .ok_or_else(|| input_error(GcError::Representation))?,
        )
        .map_err(input_error)
}

/// Both passes stay in one collection-free scope. Resource polling cannot execute
/// scripts or mutate the array, so validated fields cannot change before copying.
fn array_join(
    slots: &mut Slots<'_, '_>,
    resources: &ResourceState,
    arguments: ArgumentValues<'_>,
) -> NativeResult<String> {
    let array = arguments.scoped(slots, 0).map_err(input_error)?;
    let separator = arguments.scoped(slots, 1).map_err(input_error)?;
    if array.kind() != Some(ObjectKind::Array) || separator.kind() != Some(ObjectKind::String) {
        return Err(TrapReason::ArrayJoinExpectsAStringArrayAndSeparator.into());
    }
    let array = slots
        .scope()
        .downcast::<ArrayObject>(
            array
                .object()
                .ok_or_else(|| input_error(GcError::Representation))?,
        )
        .map_err(input_error)?;
    if !slots.scope().read(array, |array| {
        array.contract().ty == Ty::Builtin(BuiltinType::String)
    }) {
        return Err(TrapReason::StringInterpolationRequiresStringStorage.into());
    }
    let separator = string(slots, separator, TrapReason::InvalidStringSeparator)?;
    let count = slots.scope().read(array, ArrayObject::len);
    let overflow = || RuntimeError::resource_limit("joined string size");
    let mut length = slots
        .scope()
        .read(separator, |text| text.as_str().len())
        .checked_mul(count.saturating_sub(1))
        .ok_or_else(overflow)?;
    for index in 0..count {
        if index != 0 && index.is_multiple_of(1024) {
            resources.poll_execution()?;
        }
        let value = ArrayObject::get(slots.scope(), array, index).map_err(input_error)?;
        let text = string(slots, value, TrapReason::ArrayJoinExpectsStringElements)?;
        length = length
            .checked_add(slots.scope().read(text, |text| text.as_str().len()))
            .ok_or_else(overflow)?;
    }
    let mut output = String::new();
    output.try_reserve_exact(length).map_err(|_| overflow())?;
    for index in 0..count {
        if index != 0 {
            slots.scope().read(separator, |text| {
                append_text(&mut output, text.as_str(), resources)
            })?;
        }
        let value = ArrayObject::get(slots.scope(), array, index).map_err(input_error)?;
        let text = string(slots, value, TrapReason::InvalidStringElement)?;
        slots.scope().read(text, |text| {
            append_text(&mut output, text.as_str(), resources)
        })?;
    }
    Ok(output)
}

fn append_text(output: &mut String, text: &str, resources: &ResourceState) -> NativeResult<()> {
    let mut start = 0;
    while start < text.len() {
        resources.poll_execution()?;
        let mut end = start.saturating_add(4096).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&text[start..end]);
        start = end;
    }
    Ok(())
}

fn debug_assert(
    slots: &mut Slots<'_, '_>,
    resources: &ResourceState,
    arguments: ArgumentValues<'_>,
) -> NativeResult<()> {
    let condition = arguments.scoped(slots, 0).map_err(input_error)?;
    let message = arguments.scoped(slots, 1).map_err(input_error)?;
    let Some((ValueType::Bool, condition)) = condition.scalar_payload() else {
        return Err(TrapReason::DebugAssertExpectsBoolAndStringMessage.into());
    };
    if message.kind() != Some(ObjectKind::String) {
        return Err(TrapReason::DebugAssertExpectsBoolAndStringMessage.into());
    }
    if condition != 0 {
        return Ok(());
    }
    let message = string(slots, message, TrapReason::InvalidAssertionMessage)?;
    let text = slots.scope().read(message, |message| {
        let mut output = String::new();
        output
            .try_reserve_exact(message.as_str().len())
            .map_err(|_| RuntimeError::resource_limit("assertion message"))?;
        append_text(&mut output, message.as_str(), resources)?;
        Ok::<_, RuntimeError>(output)
    })?;
    Err(TrapReason::DebugAssertion { message: text }.into())
}
