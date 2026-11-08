//! JSON-RPC envelopes and the shared, IO-independent LSP frame codec.
pub mod codec;
mod envelope;
pub(crate) mod router;
pub(crate) mod typed;

#[cfg(any(test, feature = "verification"))]
pub(crate) mod verification;

#[cfg(test)]
mod tests;

use crate::protocol::envelope::{decode_envelope, encode_envelope};
use kagari_source::source_database::normalize_source_name;
use lsp_types::Uri;
use serde::{Serialize, Serializer, ser::Error as SerializeError};
use serde_json::Value;
#[cfg(any(test, feature = "verification"))]
use serde_json::to_value;
use std::fmt;

/// Maximum UTF-8 bytes in a wire request identifier.
pub const MAX_REQUEST_ID_BYTES: usize = 256;

/// A client/server request identity; null is reserved for error responses.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RequestId {
    Integer(i32),
    String(String),
}

impl From<i32> for RequestId {
    fn from(value: i32) -> Self {
        Self::Integer(value)
    }
}

impl From<String> for RequestId {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => value.fmt(formatter),
            Self::String(value) => value.fmt(formatter),
        }
    }
}

impl Serialize for RequestId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Integer(value) => serializer.serialize_i32(*value),
            Self::String(value) if value.len() <= MAX_REQUEST_ID_BYTES => {
                serializer.serialize_str(value)
            }
            Self::String(_) => Err(SerializeError::custom("request ID exceeds its byte limit")),
        }
    }
}

/// Standard JSON-RPC and LSP error codes used by the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrorCode {
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    #[cfg(any(test, feature = "verification"))]
    InternalError = -32603,
    ServerNotInitialized = -32002,
    RequestFailed = -32803,
    RequestCancelled = -32800,
}

/// A request awaiting a terminal response. Missing params normalize to null.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: RequestId,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn new(id: RequestId, method: String, params: Value) -> Self {
        Self { id, method, params }
    }
}

/// A notification never receives a response, including parameter rejection.
#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

impl Notification {
    pub fn new(method: String, params: Value) -> Self {
        Self { method, params }
    }
}

/// A protocol error with optional structured details.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Exactly one success result or failure; only failures may have a null ID.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub id: Option<RequestId>,
    pub response_result: Result<Value, ResponseError>,
}

impl Response {
    #[cfg(any(test, feature = "verification"))]
    pub fn new_ok(id: RequestId, result: impl Serialize) -> Self {
        match to_value(result) {
            Ok(result) => Self {
                id: Some(id),
                response_result: Ok(result),
            },
            Err(_) => Self::new_err(
                id,
                ErrorCode::InternalError as i32,
                "response conversion failed".into(),
            ),
        }
    }

    pub fn new_err(id: impl Into<Option<RequestId>>, code: i32, message: String) -> Self {
        Self {
            id: id.into(),
            response_result: Err(ResponseError {
                code,
                message,
                data: None,
            }),
        }
    }
}

/// Wire classification is independent of the set of registered LSP methods.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Request(Request),
    Notification(Notification),
    Response(Response),
}

impl From<Request> for Message {
    fn from(request: Request) -> Self {
        Self::Request(request)
    }
}

impl From<Notification> for Message {
    fn from(notification: Notification) -> Self {
        Self::Notification(notification)
    }
}

impl From<Response> for Message {
    fn from(response: Response) -> Self {
        Self::Response(response)
    }
}

/// A framed-body error. Malformed responses and fatal limits must not be answered.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct DecodeError {
    pub response: Option<Response>,
    pub message: String,
    /// Resource-limit violations terminate the session before request admission.
    pub fatal: bool,
}

/// Encoding failed before any bytes reached the transport.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct EncodeError {
    pub message: String,
}

/// Validate one UTF-8 JSON-RPC body, preserving present-null success results.
pub fn decode(body: &[u8]) -> Result<Message, DecodeError> {
    decode_envelope(body)
}

/// Encode a complete frame with a capped body before emitting any header bytes.
pub fn encode(message: &Message, max_body_bytes: usize) -> Result<Vec<u8>, EncodeError> {
    encode_envelope(message, max_body_bytes)
}

/// Validate a document URI and return its source-owned normalized key.
///
/// File URIs require an absolute local path without credentials, ports, queries or
/// fragments. Source normalization owns decoding, path folding and decoded-path
/// validation. Non-file identities retain all scheme-specific bytes unchanged.
pub(crate) fn source_name(uri: &Uri) -> Result<String, String> {
    // `Uri::is_absolute()` excludes fragments, which virtual document keys retain.
    let scheme = uri
        .scheme()
        .ok_or("document URI requires an absolute scheme")?
        .as_str();
    if scheme.len() == 1 {
        return Err("single-letter document URI schemes are reserved for Windows drives".into());
    }
    if scheme.eq_ignore_ascii_case("file") {
        if uri.query().is_some() || uri.fragment().is_some() {
            return Err("file document URIs cannot contain a query or fragment".into());
        }
        if let Some(authority) = uri.authority() {
            if authority.userinfo().is_some() || authority.port().is_some() {
                return Err("file document URIs cannot contain credentials or ports".into());
            }
            let host = authority.host().as_str();
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return Err("nonlocal file document URI authorities are unsupported".into());
            }
        }
        let path = uri.path();
        if !path.is_absolute() {
            return Err("file document URI requires an absolute filesystem path".into());
        }
        if path.as_str().starts_with("//") {
            return Err("UNC document paths are unsupported".into());
        }
    }
    normalize_source_name(uri.as_str())
}
