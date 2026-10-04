//! Native list defaults share algorithms; Vec operates on its actual buffer.
mod comparison;
mod receivers;
use {
    crate::bindings::{
        Entry,
        lists::{comparison::Comparison, receivers::ReceiverCalls},
    },
    kagari_runtime::{
        error::RuntimeError,
        gc::HeapObjectId,
        native::{binding::NativeResult, context::CallContext},
        value::Value,
    },
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
    ($($name:ident: $algorithm:ident, $copy:literal, $default:literal),+ $(,)?) => {
        pub(super) fn entry(name: &str) -> Option<Entry> {
            match name {
                $(concat!("$foundation_list_", stringify!($name)) | $default => Some($name),)+
                _ => None,
            }
        }
        $(fn $name(cx: &mut CallContext<'_>) -> NativeResult<Value> { run(cx, Algorithm::$algorithm, $copy) })+
    };
}

entries!(
    sorted: Sort, true, "__default_List_sorted",
    sorted_by: SortBy, true, "__default_List_sorted_by",
    sorted_by_key: SortByKey, true, "__default_List_sorted_by_key",
    reversed: Reverse, true, "__default_List_reversed",
    distinct: Distinct, true, "__default_List_distinct",
    sort: Sort, false, "__default_MutableList_sort",
    sort_by: SortBy, false, "__default_MutableList_sort_by",
    sort_by_key: SortByKey, false, "__default_MutableList_sort_by_key",
    reverse: Reverse, false, "__default_MutableList_reverse",
    retain: Retain, false, "__default_MutableList_retain",
    dedup: Dedup, false, "__default_MutableList_dedup",
);

fn run(cx: &mut CallContext<'_>, algorithm: Algorithm, copy: bool) -> NativeResult<Value> {
    cx.poll()?;
    let source = cx.argument(0)?;
    let mut comparison = Comparison::prepare(cx, algorithm)?;
    let concrete = matches!(source, Value::Array(_));
    let operations = if concrete {
        None
    } else {
        Some(ReceiverCalls::prepare(
            cx,
            usize::from(algorithm.comparison()),
            !copy,
        )?)
    };
    let target = match source {
        Value::Array(id) if copy => cx.clone_sequence(id)?,
        Value::Array(id) => id,
        _ => operations.as_ref().ok_or_else(invalid)?.snapshot(cx)?,
    };
    let _root = cx
        .heap()
        .root_value(Value::Array(target))
        .ok_or_else(invalid)?;
    if !copy && let Some(operations) = &operations {
        return edit_custom(cx, target, operations, algorithm, &mut comparison);
    }
    edit(cx, target, algorithm, &mut comparison)?;
    Ok(if copy {
        Value::Array(target)
    } else {
        Value::Unit
    })
}

fn edit(
    cx: &mut CallContext<'_>,
    target: HeapObjectId,
    algorithm: Algorithm,
    comparison: &mut Comparison<'_>,
) -> NativeResult<()> {
    cx.edit_sequence(target, |cx, mut values| match algorithm {
        Algorithm::Reverse => {
            values.reverse();
            Ok(())
        }
        Algorithm::Retain => values.retain(|value| comparison.keep(cx, value)),
        Algorithm::Dedup => values.dedup_by(|a, b| comparison.equal(cx, a, b)),
        Algorithm::Distinct => {
            let mut seen: Vec<Value> = Vec::new();
            values.retain(|value| {
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
        Algorithm::Sort if comparison.sort_scalars(&mut values)? => Ok(()),
        Algorithm::Sort | Algorithm::SortBy | Algorithm::SortByKey => {
            values.sort_by(|a, b| comparison.order(cx, a, b))
        }
    })
}

fn edit_custom(
    cx: &mut CallContext<'_>,
    target: HeapObjectId,
    operations: &ReceiverCalls<'_>,
    algorithm: Algorithm,
    comparison: &mut Comparison<'_>,
) -> NativeResult<Value> {
    let length = cx.heap().array_len(target).ok_or_else(invalid)?;
    if matches!(algorithm, Algorithm::Retain | Algorithm::Dedup) {
        let mut previous: Option<Value> = None;
        let mut kept = 0;
        for index in 0..length {
            cx.poll()?;
            let value = cx.heap().array_get(target, index).ok_or_else(invalid)?;
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
            operations.set(
                cx,
                index,
                cx.heap().array_get(target, index).ok_or_else(invalid)?,
            )?;
        }
    }
    Ok(Value::Unit)
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("list algorithm contract")
}
