//! Immutable UTF-8 operations use the public typed native boundary.
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        typed::NativeContext,
    },
};

pub(super) fn binding(
    name: &str,
    catalog: &DeclarationCatalog,
) -> NativeResult<Option<NativeBinding>> {
    let binding = match name {
        "$foundation_string_len" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (): ()| Ok(text.len()),
        ),
        "$foundation_string_is_empty" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (): ()| Ok(text.is_empty()),
        ),
        "$foundation_string_contains" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (pattern,): (String,)| {
                Ok(text.contains(&pattern))
            },
        ),
        "$foundation_string_starts_with" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (pattern,): (String,)| {
                Ok(text.starts_with(&pattern))
            },
        ),
        "$foundation_string_ends_with" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (pattern,): (String,)| {
                Ok(text.ends_with(&pattern))
            },
        ),
        "$foundation_string_find" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (pattern,): (String,)| Ok(text.find(&pattern)),
        ),
        "$foundation_string_slice" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (start, end): (usize, usize)| {
                text.get(start..end).map(str::to_owned).ok_or_else(|| RuntimeError::new(
                    RuntimeErrorKind::IndexOutOfBounds,
                    "String slice requires ordered byte offsets within bounds and on UTF-8 boundaries",
                ))
            },
        ),
        "$foundation_string_trim" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (): ()| Ok(text.trim().to_owned()),
        ),
        "$foundation_string_trim_start" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (): ()| Ok(text.trim_start().to_owned()),
        ),
        "$foundation_string_trim_end" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (): ()| Ok(text.trim_end().to_owned()),
        ),
        "$foundation_string_replace" => NativeBinding::typed_method(
            catalog,
            |_: &mut NativeContext<'_>, text: String, (from, to): (String, String)| {
                Ok(text.replace(&from, &to))
            },
        ),
        "$foundation_string_split" => NativeBinding::typed_method(catalog, split),
        _ => return Ok(None),
    }?;
    Ok(Some(binding))
}

fn split(
    cx: &mut NativeContext<'_>,
    text: String,
    (separator,): (String,),
) -> NativeResult<Vec<String>> {
    cx.collect(text.split(&separator).map(str::to_owned))
}
