use crate::builtin::BuiltinError;
use crate::builtin::standard::index_value;
use crate::builtin::standard::one_string;
use crate::builtin::standard::option_none;
use crate::builtin::standard::option_some;
use crate::builtin::standard::usize_value;
use crate::error::RuntimeError;
use crate::gc::GcHeap;
use crate::value::Value;
use kagari_ir::builtin::surface::StandardIntrinsic;
pub(super) fn string_transform(
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    let Some(Value::Str(text)) = args.first() else {
        return Err(BuiltinError::new(
            "string operation requires a string receiver",
        ));
    };
    let allocation = || BuiltinError::from(RuntimeError::resource_limit("string result size"));
    match (intrinsic, args) {
        (StandardIntrinsic::StringIsAscii, [_]) => Ok(Value::Bool(text.is_ascii())),
        (StandardIntrinsic::StringEqIgnoreAsciiCase, [_, Value::Str(other)]) => {
            Ok(Value::Bool(text.eq_ignore_ascii_case(other)))
        }
        (StandardIntrinsic::StringIsCharBoundary, [_, Value::U64(index)]) => Ok(Value::Bool(
            usize::try_from(*index).is_ok_and(|index| text.is_char_boundary(index)),
        )),
        (StandardIntrinsic::StringToLowercase, [_]) => Ok(Value::Str(text.to_lowercase())),
        (StandardIntrinsic::StringToUppercase, [_]) => Ok(Value::Str(text.to_uppercase())),
        (
            StandardIntrinsic::StringToAsciiLowercase | StandardIntrinsic::StringToAsciiUppercase,
            [_],
        ) => {
            let Value::Str(mut result) = copy_string(text)? else {
                unreachable!()
            };
            if intrinsic == StandardIntrinsic::StringToAsciiLowercase {
                result.make_ascii_lowercase();
            } else {
                result.make_ascii_uppercase();
            }
            Ok(Value::Str(result))
        }
        (StandardIntrinsic::StringRepeat, [_, Value::U64(count)]) => {
            if text.is_empty() {
                return Ok(Value::Str(String::new()));
            }
            let count = usize::try_from(*count).map_err(|_| allocation())?;
            let size = text.len().checked_mul(count).ok_or_else(allocation)?;
            let mut result = String::new();
            result.try_reserve_exact(size).map_err(|_| allocation())?;
            for _ in 0..count {
                result.push_str(text);
            }
            Ok(Value::Str(result))
        }
        (StandardIntrinsic::StringReplace, [_, Value::Str(from), Value::Str(to)]) => {
            replace_string(text, from, to, usize::MAX)
        }
        (
            StandardIntrinsic::StringReplaceN,
            [_, Value::Str(from), Value::Str(to), Value::U64(count)],
        ) => replace_string(
            text,
            from,
            to,
            usize::try_from(*count).unwrap_or(usize::MAX),
        ),
        _ => Err(BuiltinError::new("invalid string operation arguments")),
    }
}

pub(super) fn replace_string(
    text: &str,
    from: &str,
    to: &str,
    count: usize,
) -> Result<Value, BuiltinError> {
    let allocation = || BuiltinError::from(RuntimeError::resource_limit("string replacement size"));
    let mut length = text.len();
    for (_, matched) in text.match_indices(from).take(count) {
        length = length
            .checked_sub(matched.len())
            .and_then(|n| n.checked_add(to.len()))
            .ok_or_else(allocation)?;
    }
    let mut output = String::new();
    output.try_reserve_exact(length).map_err(|_| allocation())?;
    let mut previous = 0;
    for (index, matched) in text.match_indices(from).take(count) {
        output.push_str(&text[previous..index]);
        output.push_str(to);
        previous = index + matched.len();
    }
    output.push_str(&text[previous..]);
    Ok(Value::Str(output))
}

pub(super) fn string_split_once(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    let [Value::Str(text), Value::Str(separator)] = args else {
        return Err(BuiltinError::new("split_once requires two strings"));
    };
    let split = if intrinsic == StandardIntrinsic::StringSplitOnce {
        text.split_once(separator.as_str())
    } else {
        text.rsplit_once(separator.as_str())
    };
    match split {
        Some((left, right)) => option_some(
            gc,
            Value::Tuple(vec![copy_string(left)?, copy_string(right)?]),
        ),
        None => option_none(gc),
    }
}

pub(super) fn string_query(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    let Some(Value::Str(text)) = args.first() else {
        return Err(BuiltinError::new("string query requires a string receiver"));
    };
    if matches!(
        intrinsic,
        StandardIntrinsic::StringTrim
            | StandardIntrinsic::StringTrimStart
            | StandardIntrinsic::StringTrimEnd
    ) {
        if args.len() != 1 {
            return Err(BuiltinError::new("trim expects one argument"));
        }
        return copy_string(match intrinsic {
            StandardIntrinsic::StringTrim => text.trim(),
            StandardIntrinsic::StringTrimStart => text.trim_start(),
            _ => text.trim_end(),
        });
    }
    let [_, Value::Str(needle)] = args else {
        return Err(BuiltinError::new("string query requires a string pattern"));
    };
    if matches!(
        intrinsic,
        StandardIntrinsic::StringFind | StandardIntrinsic::StringRfind
    ) {
        let found = if intrinsic == StandardIntrinsic::StringFind {
            text.find(needle)
        } else {
            text.rfind(needle)
        };
        return match found {
            Some(index) => option_some(gc, Value::U64(index as u64)),
            None => option_none(gc),
        };
    }
    let stripped = if intrinsic == StandardIntrinsic::StringStripPrefix {
        text.strip_prefix(needle.as_str())
    } else {
        text.strip_suffix(needle.as_str())
    };
    match stripped {
        Some(value) => option_some(gc, copy_string(value)?),
        None => option_none(gc),
    }
}

pub(super) fn copy_string(text: &str) -> Result<Value, BuiltinError> {
    let mut output = String::new();
    output
        .try_reserve_exact(text.len())
        .map_err(|_| BuiltinError::from(RuntimeError::resource_limit("string allocation")))?;
    output.push_str(text);
    Ok(Value::Str(output))
}

pub(super) fn string_len_bytes(args: &[Value]) -> Result<Value, BuiltinError> {
    let value = one_string(args, "string.len_bytes")?;
    Ok(usize_value(value.len()))
}

pub(super) fn string_len_chars(args: &[Value]) -> Result<Value, BuiltinError> {
    let value = one_string(args, "string.len_chars")?;
    Ok(usize_value(value.chars().count()))
}

pub(super) fn string_is_empty(args: &[Value]) -> Result<Value, BuiltinError> {
    let value = one_string(args, "string.is_empty")?;
    Ok(Value::Bool(value.is_empty()))
}

pub(super) fn string_concat(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(lhs), Value::Str(rhs)] = args else {
        return Err(BuiltinError::new("string.concat expects two strings"));
    };
    Ok(Value::Str(format!("{lhs}{rhs}")))
}

pub(super) fn string_contains(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), Value::Str(needle)] = args else {
        return Err(BuiltinError::new("string.contains expects two strings"));
    };
    Ok(Value::Bool(value.contains(needle)))
}

pub(super) fn string_starts_with(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), Value::Str(prefix)] = args else {
        return Err(BuiltinError::new("string.starts_with expects two strings"));
    };
    Ok(Value::Bool(value.starts_with(prefix)))
}

pub(super) fn string_ends_with(args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), Value::Str(suffix)] = args else {
        return Err(BuiltinError::new("string.ends_with expects two strings"));
    };
    Ok(Value::Bool(value.ends_with(suffix)))
}

pub(super) fn string_slice(gc: &GcHeap, args: &[Value]) -> Result<Value, BuiltinError> {
    let [Value::Str(value), start, end] = args else {
        return Err(BuiltinError::new(
            "string.slice expects string, start, and end",
        ));
    };
    let start = index_value(start, "string.slice")?;
    let end = index_value(end, "string.slice")?;
    if start > end {
        return option_none(gc);
    }
    match value.get(start..end) {
        Some(slice) => option_some(gc, Value::Str(slice.to_owned())),
        None => option_none(gc),
    }
}
