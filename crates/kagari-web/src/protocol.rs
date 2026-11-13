//! Bounded, owned wire records; heap graphs and runtime identities never cross JS.
use crate::diagnostics::Diagnostic;
use kagari::error::EmbeddingError;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    host_scope::HostResourceScope,
    value::{carriers::RootedValue, kind::ObjectKind, view::ValueView},
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

    pub(crate) fn runtime(error: &RuntimeError) -> Self {
        Self::new(error.code(), error)
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
    pub(crate) fn rooted(root: &RootedValue, runtime: &mut Runtime) -> Result<Self, WebError> {
        let mut scope = runtime
            .host_scope(&[])
            .map_err(|error| WebError::runtime(&error))?;
        let value = scope
            .import(root)
            .map_err(|error| WebError::runtime(&error))?;
        Self::value(value, &mut scope)
    }

    fn value(value: ValueView<'_>, scope: &mut HostResourceScope<'_>) -> Result<Self, WebError> {
        if value.is_unit() {
            return Ok(Self::Unit);
        }
        if let Some(value) = value.as_bool() {
            return Ok(Self::Bool { value });
        }
        macro_rules! scalar {
            ($accessor:ident, $variant:ident) => {
                if let Some(value) = value.$accessor() {
                    return Ok(Self::$variant {
                        value: value.to_string(),
                    });
                }
            };
        }
        scalar!(as_i32, I32);
        scalar!(as_i64, I64);
        scalar!(as_u64, U64);
        scalar!(as_f32, F32);
        scalar!(as_f64, F64);
        if value.as_char().is_some() {
            return Ok(Self::Opaque { kind: "char" });
        }
        let kind = value
            .kind()
            .ok_or_else(|| WebError::new("WEB_RESULT_ROOT", "invalid result representation"))?;
        if kind == ObjectKind::String {
            return scope
                .read_string(&value, |text| Self::String {
                    value: prefix(text, RESULT_BYTES).to_owned(),
                    truncated: text.len() > RESULT_BYTES,
                })
                .map_err(|error| WebError::runtime(&error));
        }
        Ok(Self::Opaque {
            kind: match kind {
                ObjectKind::Tuple => "tuple",
                ObjectKind::Range => "range",
                ObjectKind::Array => "array",
                ObjectKind::Enum => "enum",
                ObjectKind::Struct => "struct",
                ObjectKind::Native => "gc_handle",
                ObjectKind::Interface => "interface",
                ObjectKind::Closure => "closure",
                ObjectKind::Cell => "cell",
                ObjectKind::HostRoot => "host_root",
                ObjectKind::HostPath => "host_path_view",
                ObjectKind::Ephemeral => "ephemeral",
                ObjectKind::String => unreachable!("String handled above"),
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
    #[cfg(feature = "source")]
    use crate::BrowserSession;
    use kagari_runtime::Runtime;
    #[cfg(feature = "source")]
    use kagari_runtime::native::conversion::context::ConversionContext;

    #[test]
    fn integers_are_tagged_lossless_decimal_strings() {
        let mut runtime = Runtime::default();
        for (value, tag, expected) in [
            (RootedValue::from(i64::MIN), "i64", "-9223372036854775808"),
            (RootedValue::from(i64::MAX), "i64", "9223372036854775807"),
            (RootedValue::from(u64::MAX), "u64", "18446744073709551615"),
            (RootedValue::from(-7i32), "i32", "-7"),
        ] {
            let owned =
                OwnedResult::rooted(&value, &mut runtime).unwrap_or_else(|_| panic!("scalar"));
            let json = serde_json::to_value(owned).unwrap();
            assert_eq!(json["tag"], tag);
            assert_eq!(json["value"], expected);
        }
    }

    #[cfg(feature = "source")]
    #[test]
    fn strings_are_bounded_owned_copies_not_retained_heap_handles() {
        let mut session = BrowserSession::new().unwrap();
        let prepared: serde_json::Value = serde_json::from_str(
            &session.compile(include_str!("../tests/fixtures/protocol-owner.kgr")),
        )
        .unwrap();
        assert_eq!(prepared["status"], "ready");
        let owner = session
            .runtime
            .select_program(session.module.as_ref().unwrap())
            .unwrap();
        session.runtime.collect_garbage().unwrap();
        let baseline = session.runtime.gc_stats();
        let root = ConversionContext::new(&mut session.runtime, &owner)
            .unwrap()
            .encode("😀".repeat(RESULT_BYTES))
            .unwrap();
        session.runtime.collect_garbage().unwrap();
        let owned = OwnedResult::rooted(&root, &mut session.runtime)
            .unwrap_or_else(|_| panic!("rooted String"));
        drop(root);
        session.runtime.collect_garbage().unwrap();
        assert_eq!(session.runtime.gc_stats().objects, baseline.objects);
        assert_eq!(
            session.runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
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
