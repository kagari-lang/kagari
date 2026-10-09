//! Immutable UTF-8 input uses scoped views; owned output keeps typed conversion.
use crate::bindings::Entry;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::CallContext,
        conversion::KagariType,
        scalar::NativeScalar,
    },
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::cell::Ref;

pub(super) fn binding(
    name: &str,
    catalog: &DeclarationCatalog,
) -> NativeResult<Option<NativeBinding>> {
    let string = BuiltinType::String;
    let binding = match name {
        "$foundation_string_len" => method::<usize>(catalog, &[string], length),
        "$foundation_string_is_empty" => method::<bool>(catalog, &[string], is_empty),
        "$foundation_string_contains" => method::<bool>(catalog, &[string, string], contains),
        "$foundation_string_starts_with" => method::<bool>(catalog, &[string, string], starts_with),
        "$foundation_string_ends_with" => method::<bool>(catalog, &[string, string], ends_with),
        "$foundation_string_find" => method::<Option<usize>>(catalog, &[string, string], find),
        "$foundation_string_slice" => method::<String>(
            catalog,
            &[string, BuiltinType::USize, BuiltinType::USize],
            slice,
        ),
        "$foundation_string_trim" => method::<String>(catalog, &[string], trim),
        "$foundation_string_trim_start" => method::<String>(catalog, &[string], trim_start),
        "$foundation_string_trim_end" => method::<String>(catalog, &[string], trim_end),
        "$foundation_string_replace" => {
            method::<String>(catalog, &[string, string, string], replace)
        }
        "$foundation_string_split" => method::<Vec<String>>(catalog, &[string, string], split),
        _ => return Ok(None),
    }?;
    Ok(Some(binding))
}

fn method<R: KagariType>(
    catalog: &DeclarationCatalog,
    arguments: &[BuiltinType],
    entry: Entry,
) -> NativeResult<NativeBinding> {
    Ok(NativeBinding::new(
        arguments
            .iter()
            .map(|ty| Codec::Scalar(Ty::Builtin(*ty)))
            .collect::<Vec<_>>(),
        Codec::Scalar(R::kagari_type(catalog)?.abi().clone()),
        entry,
    ))
}

fn text<'a>(cx: &'a CallContext<'_>, index: usize) -> NativeResult<Ref<'a, str>> {
    let Value::Str(id) = cx.argument(index)? else {
        return Err(RuntimeError::module_validation("string argument"));
    };
    cx.heap()
        .string(id)
        .ok_or_else(|| RuntimeError::module_validation("invalid string argument"))
}

fn length(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(text(cx, 0)?.len().encode())
}

fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.is_empty()))
}

fn contains(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.contains(&*text(cx, 1)?)))
}

fn starts_with(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.starts_with(&*text(cx, 1)?)))
}

fn ends_with(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(Value::Bool(text(cx, 0)?.ends_with(&*text(cx, 1)?)))
}

fn find(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.find(&*text(cx, 1)?);
    cx.encode_result(result)
}

fn slice(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let start = usize::decode(cx.argument(1)?)?;
    let end = usize::decode(cx.argument(2)?)?;
    let result = text(cx, 0)?
        .get(start..end)
        .map(str::to_owned)
        .ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                "String slice requires ordered byte offsets within bounds and on UTF-8 boundaries",
            )
        })?;
    cx.encode_result(result)
}

fn trim(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.trim().to_owned();
    cx.encode_result(result)
}

fn trim_start(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.trim_start().to_owned();
    cx.encode_result(result)
}

fn trim_end(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.trim_end().to_owned();
    cx.encode_result(result)
}

fn replace(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let result = text(cx, 0)?.replace(&*text(cx, 1)?, &text(cx, 2)?);
    cx.encode_result(result)
}

fn split(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let parts = {
        let source = text(cx, 0)?;
        let separator = text(cx, 1)?;
        let mut parts = Vec::new();
        for part in source.split(&*separator) {
            cx.poll()?;
            parts
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("native collected values"))?;
            parts.push(part.to_owned());
        }
        cx.poll()?;
        parts
    };
    cx.encode_result(parts)
}
