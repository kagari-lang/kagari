//! Physical-path presentation; package semantic keys never become editor URIs.
use crate::{protocol::source_name, workspace::CaptureIssue};
use kagari_package::error::PackageError;
use kagari_source::source_database::normalize_source_name;
use lsp_types::Uri;
use std::{fmt::Write, path::Path};

pub(super) fn native_key(path: &Path) -> Result<String, String> {
    let text = path.to_str().ok_or("source path is not UTF-8")?;
    // Windows canonicalization uses a verbatim prefix. The package provider keeps
    // that exact key; only URI/normalized-document presentation strips the prefix.
    let text = text.strip_prefix("\\\\?\\").unwrap_or(text);
    normalize_source_name(text)
}

pub(super) fn physical_uri(key: &str) -> Result<Uri, String> {
    if key
        .split_once(':')
        .is_some_and(|(scheme, _)| scheme.len() > 1)
        && !key.starts_with("\\\\")
    {
        let uri: Uri = key
            .parse()
            .map_err(|error| format!("invalid source URI: {error}"))?;
        source_name(&uri)?;
        return Ok(uri);
    }
    file_uri(&native_key(Path::new(key))?)
}

/// Convert a local absolute path, including Windows canonical paths, to a file URI.
pub(crate) fn file_uri(path: &str) -> Result<Uri, String> {
    let key = native_key(Path::new(path))?;
    let drive = key.as_bytes().get(1) == Some(&b':') && key.as_bytes().get(2) == Some(&b'/');
    if !key.starts_with('/') && !drive {
        return Err("source origin is not an absolute physical path".into());
    }
    let mut encoded = if key.starts_with('/') {
        "file://"
    } else {
        "file:///"
    }
    .to_owned();
    for byte in key.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing a String cannot fail");
        }
    }
    encoded
        .parse()
        .map_err(|error| format!("invalid physical source URI: {error}"))
}

pub(super) fn issue(error: &PackageError) -> CaptureIssue {
    match error {
        PackageError::Input {
            origin,
            range,
            message,
        } => {
            let uri = physical_uri(origin).ok();
            let message = if uri.is_some() {
                message.clone()
            } else {
                format!("{origin}: {message}")
            };
            CaptureIssue {
                uri,
                range: range.clone(),
                message,
            }
        }
        PackageError::Cancelled => CaptureIssue {
            uri: None,
            range: None,
            message: error.to_string(),
        },
    }
}

pub(super) fn input_error(origin: &str, message: impl Into<String>) -> PackageError {
    PackageError::Input {
        origin: origin.into(),
        range: None,
        message: message.into(),
    }
}
