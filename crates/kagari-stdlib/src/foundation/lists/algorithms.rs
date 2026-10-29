//! Native list defaults share algorithms; Vec operates on its actual buffer.
mod comparison;
mod receivers;
#[cfg(panic = "unwind")]
use crate::collections::vector::sort_scalar::try_sort_scalar;
use crate::{
    collections::vector::ScriptVec,
    foundation::{
        enumerations::value::EnumSymbols,
        iteration::traversal,
        lists::algorithms::{comparison::Comparison, receivers::ReceiverCalls},
    },
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult, context::CallContext, typed::NativeContext,
        value_handle::ScriptValue,
    },
    value::Value,
};

#[derive(Clone, Copy)]
enum Algorithm {
    Sort,
    SortBy,
    SortByKey,
    Reverse,
    Retain,
    Dedup,
    Distinct,
}

impl Algorithm {
    fn comparison(self) -> bool {
        matches!(
            self,
            Self::Sort | Self::SortByKey | Self::Dedup | Self::Distinct
        )
    }
}

macro_rules! entries {
    ($($name:ident: $algorithm:ident, $copy:literal),+ $(,)?) => {
        $(pub(crate) fn $name(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> { run(symbols, cx, Algorithm::$algorithm, $copy) })+
    };
}

entries!(
    sorted: Sort, true,
    sorted_by: SortBy, true,
    sorted_by_key: SortByKey, true,
    reversed: Reverse, true,
    distinct: Distinct, true,
    sort: Sort, false,
    sort_by: SortBy, false,
    sort_by_key: SortByKey, false,
    reverse: Reverse, false,
    retain: Retain, false,
    dedup: Dedup, false,
);

fn run(
    symbols: &EnumSymbols,
    call: &mut CallContext<'_>,
    algorithm: Algorithm,
    copy: bool,
) -> NativeResult<Value> {
    call.poll()?;
    let concrete = traversal::vec_argument(call)?.is_some();
    // The scalar leaf uses unwind recovery for unordered floats. Abort targets
    // must use the same ordinary fallible algorithm as collecting comparisons.
    #[cfg(panic = "unwind")]
    if concrete && !copy && matches!(algorithm, Algorithm::Sort) && try_sort_scalar(call)? {
        return Ok(Value::Unit);
    }
    let mut cx = call.native_context()?;
    let comparison = Comparison::prepare(call, &mut cx, algorithm, symbols)?;
    let operations = if concrete {
        None
    } else {
        Some(ReceiverCalls::prepare(
            call,
            &mut cx,
            usize::from(algorithm.comparison()),
            !copy,
        )?)
    };
    let target: ScriptVec<ScriptValue> = if concrete {
        // SAFETY: CallContext owns the complete argument snapshot.
        let source: ScriptVec<ScriptValue> = unsafe {
            cx.conversion().decode_transport(
                &call.argument_type_argument(0)?,
                &call.argument_transport(0)?,
            )
        }?;
        if copy {
            let item = cx
                .conversion()
                .parameter(&call.argument_type_argument(0)?, 0)?;
            let target = ScriptVec::create_with_type(&mut cx, item, vec![])?;
            source.for_each(&mut cx, |cx, value| target.push(cx, value))?;
            target
        } else {
            source
        }
    } else {
        operations.as_ref().ok_or_else(invalid)?.snapshot(&mut cx)?
    };
    if !copy && let Some(operations) = &operations {
        edit_custom(&mut cx, &target, operations, algorithm, &comparison)?;
    } else {
        edit(&mut cx, &target, algorithm, &comparison)?;
    }
    if copy {
        // SAFETY: Conversion roots protect the result until this terminal Native
        // return publishes it; no collecting operation intervenes.
        unsafe {
            cx.conversion()
                .encode_value(&call.result_type_argument()?, target)?
                .value_transport(cx.runtime())
        }
    } else {
        Ok(Value::Unit)
    }
}

fn edit(
    cx: &mut NativeContext<'_>,
    target: &ScriptVec<ScriptValue>,
    algorithm: Algorithm,
    comparison: &Comparison<'_, '_>,
) -> NativeResult<()> {
    match algorithm {
        Algorithm::Reverse => target.reverse(cx),
        Algorithm::Retain => target.retain(cx, |cx, value| comparison.keep(cx, value)),
        Algorithm::Dedup => target.dedup_by(cx, |cx, a, b| comparison.equal(cx, a, b)),
        Algorithm::Distinct => {
            let mut seen: Vec<ScriptValue> = Vec::new();
            target.retain(cx, |cx, value| {
                for previous in &seen {
                    if comparison.equal(cx, value.clone(), previous.clone())? {
                        return Ok(false);
                    }
                }
                seen.try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("distinct values"))?;
                seen.push(value);
                Ok(true)
            })
        }
        Algorithm::Sort | Algorithm::SortBy | Algorithm::SortByKey => {
            target.sort_retained(cx, |cx, a, b| comparison.order(cx, a, b))
        }
    }
}

fn edit_custom(
    cx: &mut NativeContext<'_>,
    target: &ScriptVec<ScriptValue>,
    operations: &ReceiverCalls,
    algorithm: Algorithm,
    comparison: &Comparison<'_, '_>,
) -> NativeResult<()> {
    let length = target.len(cx)?;
    if matches!(algorithm, Algorithm::Retain | Algorithm::Dedup) {
        let mut previous: Option<ScriptValue> = None;
        let mut kept = 0;
        for index in 0..length {
            cx.poll()?;
            let value = target.get(cx, index)?.ok_or_else(invalid)?;
            let keep = if matches!(algorithm, Algorithm::Retain) {
                comparison.keep(cx, value.clone())?
            } else if let Some(previous) = &previous {
                !comparison.equal(cx, previous.clone(), value.clone())?
            } else {
                true
            };
            if keep {
                previous = Some(value);
                kept += 1;
            } else {
                operations.remove(cx, kept)?;
            }
        }
    } else {
        edit(cx, target, algorithm, comparison)?;
        for index in 0..length {
            cx.poll()?;
            let value = target.get(cx, index)?.ok_or_else(invalid)?;
            operations.set(cx, index, value)?;
        }
    }
    Ok(())
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("list algorithm contract")
}
