//! Bounded, owned wire records; heap graphs and runtime identities never cross JS.
use crate::diagnostics::Diagnostic;
use kagari::error::EmbeddingError;
use kagari_runtime::{
    gc::{GcHeap, roots::RootedValue},
    value::Value,
};
use serde::Serialize;
use std::fmt::{self, Write};

pub(crate) const TEXT_BYTES: usize = 4096;
pub(crate) const RESULT_BYTES: usize = 16 * 1024;

pub(crate) fn prefix(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

struct BoundedText {
    text: String,
    truncated: bool,
}

impl Write for BoundedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let kept = prefix(text, TEXT_BYTES - self.text.len());
        self.text.push_str(kept);
        if kept.len() != text.len() {
            self.truncated = true;
            return Err(fmt::Error);
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub(crate) struct WebError {
    pub code: String,
    pub severity: &'static str,
    pub message: String,
    pub truncated: bool,
}

impl WebError {
    pub(crate) fn new(code: &str, message: impl fmt::Display) -> Self {
        let mut buffer = BoundedText {
            text: String::new(),
            truncated: false,
        };
        let _ = write!(buffer, "{message}");
        Self {
            code: prefix(code, 128).to_owned(),
            severity: "error",
            message: buffer.text,
            truncated: buffer.truncated || code.len() > 128,
        }
    }

    pub(crate) fn sdk(error: &EmbeddingError) -> Self {
        Self::new(&error.code(), format_args!("{error:?}"))
    }
}

#[derive(Serialize)]
#[serde(tag = "tag", rename_all = "snake_case")]
pub(crate) enum OwnedResult {
    Unit,
    Bool { value: bool },
    I32 { value: String },
    I64 { value: String },
    U64 { value: String },
    F32 { value: String },
    F64 { value: String },
    String { value: String, truncated: bool },
    Opaque { kind: &'static str },
}

impl OwnedResult {
    pub(crate) fn rooted(root: &RootedValue, heap: &GcHeap) -> Result<Self, WebError> {
        let value = root
            .value(heap)
            .ok_or_else(|| WebError::new("WEB_RESULT_ROOT", "result root is stale"))?;
        Self::value(value, heap)
    }

    fn value(value: Value, heap: &GcHeap) -> Result<Self, WebError> {
        Ok(match value {
            Value::Unit => Self::Unit,
            Value::Bool(value) => Self::Bool { value },
            Value::I32(value) => Self::I32 {
                value: value.to_string(),
            },
            Value::I64(value) => Self::I64 {
                value: value.to_string(),
            },
            Value::U64(value) => Self::U64 {
                value: value.to_string(),
            },
            Value::F32(value) => Self::F32 {
                value: value.to_string(),
            },
            Value::F64(value) => Self::F64 {
                value: value.to_string(),
            },
            Value::Str(id) => {
                let text = heap
                    .string(id)
                    .ok_or_else(|| WebError::new("WEB_RESULT_ROOT", "result String is stale"))?;
                Self::String {
                    value: prefix(&text, RESULT_BYTES).to_owned(),
                    truncated: text.len() > RESULT_BYTES,
                }
            }
            value => Self::Opaque {
                kind: match value {
                    Value::Tuple(_) => "tuple",
                    Value::Range(_) => "range",
                    Value::Array(_) => "array",
                    Value::Map(_) => "map",
                    Value::Set(_) => "set",
                    Value::Enum(_) => "enum",
                    Value::Struct(_) => "struct",
                    Value::GcHandle(_) => "gc_handle",
                    Value::Interface(_) => "interface",
                    Value::Closure(_) => "closure",
                    Value::Cell(_) => "cell",
                    Value::HostRoot(_) => "host_root",
                    Value::HostPathView(_) => "host_path_view",
                    Value::Ephemeral(_) | Value::RuntimeEphemeral(_) => "ephemeral",
                    _ => unreachable!("scalars handled above"),
                },
            },
        })
    }
}

#[derive(Serialize)]
pub(crate) struct Response {
    pub version: u8,
    pub status: &'static str,
    pub output: Vec<String>,
    pub output_truncated: bool,
    pub result: Option<OwnedResult>,
    pub error: Option<WebError>,
    pub diagnostics: Vec<Diagnostic>,
    pub diagnostics_truncated: bool,
}

impl Response {
    pub(crate) fn new(status: &'static str) -> Self {
        Self {
            version: 1,
            status,
            output: Vec::new(),
            output_truncated: false,
            result: None,
            error: None,
            diagnostics: Vec::new(),
            diagnostics_truncated: false,
        }
    }

    pub(crate) fn error(error: WebError) -> Self {
        Self {
            error: Some(error),
            ..Self::new("error")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_runtime::Runtime;

    #[test]
    fn integers_are_tagged_lossless_decimal_strings() {
        let runtime = Runtime::default();
        let heap = runtime.gc();
        for (value, tag, expected) in [
            (Value::I64(i64::MIN), "i64", "-9223372036854775808"),
            (Value::I64(i64::MAX), "i64", "9223372036854775807"),
            (Value::U64(u64::MAX), "u64", "18446744073709551615"),
            (Value::I32(-7), "i32", "-7"),
        ] {
            let owned = OwnedResult::value(value, heap).unwrap_or_else(|_| panic!("scalar"));
            let json = serde_json::to_value(owned).unwrap();
            assert_eq!(json["tag"], tag);
            assert_eq!(json["value"], expected);
        }
    }

    #[test]
    fn strings_are_bounded_owned_copies_not_retained_heap_handles() {
        let runtime = Runtime::default();
        let heap = runtime.gc();
        let value = heap.alloc_string("😀".repeat(RESULT_BYTES)).unwrap();
        let root = heap.root_value(value).unwrap();
        runtime.collect_garbage().unwrap();
        let owned = OwnedResult::rooted(&root, heap).unwrap_or_else(|_| panic!("rooted String"));
        drop(root);
        runtime.collect_garbage().unwrap();
        assert!(!heap.validate_value(&value));
        let json = serde_json::to_value(owned).unwrap();
        assert_eq!(json["tag"], "string");
        assert_eq!(json["value"].as_str().unwrap().len(), RESULT_BYTES);
        assert_eq!(json["truncated"], true);
    }

    #[test]
    fn errors_and_unicode_prefixes_are_bounded() {
        assert_eq!(prefix("a😀b", 4), "a");
        let error = WebError::new("WEB_TEST", "😀".repeat(TEXT_BYTES));
        assert_eq!(error.message.len(), TEXT_BYTES);
        assert!(error.truncated);
    }
}
