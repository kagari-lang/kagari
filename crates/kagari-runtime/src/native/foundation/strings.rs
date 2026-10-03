//! Immutable UTF-8 operations implemented through ordinary foundation bindings.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        context::CallContext,
        foundation::{Entry, index, option},
        scalar::NativeScalar,
    },
    value::Value,
};

pub(super) fn entry(name: &str) -> Option<Entry> {
    Some(match name {
        "$foundation_string_len" => len,
        "$foundation_string_is_empty" => is_empty,
        "$foundation_string_contains" => contains,
        "$foundation_string_starts_with" => starts_with,
        "$foundation_string_ends_with" => ends_with,
        "$foundation_string_find" => find,
        "$foundation_string_slice" => slice,
        "$foundation_string_trim" => trim,
        "$foundation_string_trim_start" => trim_start,
        "$foundation_string_trim_end" => trim_end,
        "$foundation_string_replace" => replace,
        "$foundation_string_split" => split,
        _ => return None,
    })
}

fn string<R>(
    cx: &CallContext<'_>,
    slot: usize,
    read: impl FnOnce(&str) -> NativeResult<R>,
) -> NativeResult<R> {
    cx.arguments()
        .with_value(slot, |value| {
            let Value::Str(text) = value else {
                return Err(invalid());
            };
            read(text)
        })
        .ok_or_else(invalid)?
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("String argument")
}

fn len(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| Ok(text.len().encode()))
}

fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| Ok(Value::Bool(text.is_empty())))
}

fn contains(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| {
        string(cx, 1, |pattern| Ok(Value::Bool(text.contains(pattern))))
    })
}

fn starts_with(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| {
        string(cx, 1, |pattern| Ok(Value::Bool(text.starts_with(pattern))))
    })
}

fn ends_with(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| {
        string(cx, 1, |pattern| Ok(Value::Bool(text.ends_with(pattern))))
    })
}

fn find(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let found = string(cx, 0, |text| {
        string(cx, 1, |pattern| Ok(text.find(pattern)))
    })?;
    option(cx, found.map(NativeScalar::encode))
}

fn slice(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let start = index(cx, 1)?;
    let end = index(cx, 2)?;
    string(cx, 0, |text| {
        text.get(start..end).map(|text| Value::Str(text.to_owned())).ok_or_else(|| RuntimeError::new(
            RuntimeErrorKind::IndexOutOfBounds,
            "String slice requires ordered byte offsets within bounds and on UTF-8 boundaries",
        ))
    })
}

fn trim(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| Ok(Value::Str(text.trim().to_owned())))
}

fn trim_start(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| Ok(Value::Str(text.trim_start().to_owned())))
}

fn trim_end(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| Ok(Value::Str(text.trim_end().to_owned())))
}

fn replace(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    string(cx, 0, |text| {
        string(cx, 1, |from| {
            string(cx, 2, |to| Ok(Value::Str(text.replace(from, to))))
        })
    })
}

fn split(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let fields = string(cx, 0, |text| {
        string(cx, 1, |separator| {
            let mut fields = Vec::new();
            for field in text.split(separator) {
                cx.poll()?;
                fields
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("String split fields"))?;
                fields.push(Value::Str(field.to_owned()));
            }
            Ok(fields)
        })
    })?;
    // Strings own their bytes; allocation begins after releasing the borrowed
    // argument slots. The checked result adapter roots and boxes this array.
    cx.allocate_sequence(cx.result_type_parameter(0)?, fields)
}
