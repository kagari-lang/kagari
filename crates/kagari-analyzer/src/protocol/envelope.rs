//! Presence-aware JSON-RPC decoding and capped serialization.
use crate::protocol::{
    DecodeError, EncodeError, ErrorCode, MAX_REQUEST_ID_BYTES, Message, Notification, Request,
    RequestId, Response, ResponseError,
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::{Error as SerializeError, SerializeMap},
};
use serde_json::{Map, Value, from_slice, to_writer};
use std::{
    fmt,
    io::{Error, Write},
};

struct EnvelopeObject {
    fields: Map<String, Value>,
    duplicate_fields: bool,
    oversized_id: bool,
}

impl<'de> Deserialize<'de> for EnvelopeObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;

        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = EnvelopeObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("one JSON-RPC object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut entries: M) -> Result<Self::Value, M::Error> {
                let mut object = EnvelopeObject {
                    fields: Map::new(),
                    duplicate_fields: false,
                    oversized_id: false,
                };
                while let Some((key, value)) = entries.next_entry::<String, Value>()? {
                    object.oversized_id |= key == "id"
                        && value
                            .as_str()
                            .is_some_and(|id| id.len() > MAX_REQUEST_ID_BYTES);
                    object.duplicate_fields |= object.fields.insert(key, value).is_some();
                }
                Ok(object)
            }
        }

        deserializer.deserialize_map(ObjectVisitor)
    }
}

fn request_id(value: &Value) -> Option<RequestId> {
    match value {
        Value::Number(value) => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .map(RequestId::Integer),
        Value::String(value) if value.len() <= MAX_REQUEST_ID_BYTES => {
            Some(RequestId::String(value.clone()))
        }
        _ => None,
    }
}

fn invalid(object: &Map<String, Value>, message: &str) -> DecodeError {
    let response_like = !object.contains_key("method")
        && (object.contains_key("id")
            || object.contains_key("result")
            || object.contains_key("error"));
    DecodeError {
        response: (!response_like).then(|| {
            Response::new_err(
                object.get("id").and_then(request_id),
                ErrorCode::InvalidRequest as i32,
                message.into(),
            )
        }),
        message: message.into(),
        fatal: false,
    }
}

pub(super) fn decode_envelope(body: &[u8]) -> Result<Message, DecodeError> {
    let EnvelopeObject {
        fields: mut object,
        duplicate_fields,
        oversized_id,
    } = from_slice(body).map_err(|error| {
        let (code, message) = if error.is_data() {
            (ErrorCode::InvalidRequest, "invalid JSON-RPC object")
        } else {
            (ErrorCode::ParseError, "invalid UTF-8 JSON body")
        };
        DecodeError {
            response: Some(Response::new_err(None, code as i32, message.into())),
            message: message.into(),
            fatal: false,
        }
    })?;
    if oversized_id {
        return Err(DecodeError {
            response: None,
            message: "request ID exceeds its byte limit".into(),
            fatal: true,
        });
    }
    if duplicate_fields {
        let mut error = invalid(&object, "duplicate JSON-RPC envelope field");
        // Duplicate fields never select a request identity from an ambiguous
        // object. Keep response classification to prevent error-response loops.
        if let Some(response) = &mut error.response {
            response.id = None;
        }
        return Err(error);
    }
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid(&object, "jsonrpc must be the string 2.0"));
    }
    if let Some(method) = object.get("method") {
        let method = method
            .as_str()
            .ok_or_else(|| invalid(&object, "method must be a string"))?;
        if object.contains_key("result") || object.contains_key("error") {
            return Err(invalid(&object, "request cannot contain result or error"));
        }
        let method = method.to_owned();
        let id = match object.get("id") {
            Some(value) => Some(request_id(value).ok_or_else(|| {
                invalid(
                    &object,
                    "request ID must be a bounded string or 32-bit integer",
                )
            })?),
            None => None,
        };
        let params = object.remove("params").unwrap_or(Value::Null);
        return Ok(match id {
            Some(id) => Request::new(id, method, params).into(),
            None => Notification::new(method, params).into(),
        });
    }
    let id = match object.get("id") {
        Some(Value::Null) => None,
        Some(value) => {
            Some(request_id(value).ok_or_else(|| invalid(&object, "invalid response ID"))?)
        }
        None => return Err(invalid(&object, "response is missing its ID")),
    };
    if object.contains_key("params")
        || object.contains_key("result") == object.contains_key("error")
    {
        return Err(invalid(
            &object,
            "response requires exactly one result or error",
        ));
    }
    let response_result = if let Some(result) = object.remove("result") {
        if id.is_none() {
            return Err(invalid(
                &object,
                "successful response cannot have a null ID",
            ));
        }
        Ok(result)
    } else {
        let error = object
            .get("error")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid(&object, "error must be an object"))?;
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| invalid(&object, "error code must be a 32-bit integer"))?;
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(&object, "error message must be a string"))?;
        Err(ResponseError {
            code,
            message: message.to_owned(),
            data: error.get("data").cloned(),
        })
    };
    Ok(Response {
        id,
        response_result,
    }
    .into())
}

impl Serialize for Message {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_map(None)?;
        fields.serialize_entry("jsonrpc", "2.0")?;
        match self {
            Self::Request(request) => {
                fields.serialize_entry("id", &request.id)?;
                fields.serialize_entry("method", &request.method)?;
                if !request.params.is_null() {
                    fields.serialize_entry("params", &request.params)?;
                }
            }
            Self::Notification(notification) => {
                fields.serialize_entry("method", &notification.method)?;
                if !notification.params.is_null() {
                    fields.serialize_entry("params", &notification.params)?;
                }
            }
            Self::Response(response) => {
                fields.serialize_entry("id", &response.id)?;
                match &response.response_result {
                    Ok(result) if response.id.is_some() => {
                        fields.serialize_entry("result", result)?;
                    }
                    Ok(_) => {
                        return Err(SerializeError::custom("successful response requires an ID"));
                    }
                    Err(error) => fields.serialize_entry("error", error)?,
                }
            }
        }
        fields.end()
    }
}

// Reserve a header prefix once. In-place compaction avoids holding both an
// encoded body and a second complete frame at the writer boundary.
const HEADER_RESERVE: usize = 64;

struct CappedBody {
    bytes: Vec<u8>,
    max_bytes: usize,
}

impl Write for CappedBody {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        let body_len = self.bytes.len() - HEADER_RESERVE;
        if bytes.len() > self.max_bytes.saturating_sub(body_len) {
            return Err(Error::other("encoded JSON exceeds its body limit"));
        }
        let required = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| Error::other("encoded frame length overflow"))?;
        if required > self.bytes.capacity() {
            let maximum = self
                .max_bytes
                .checked_add(HEADER_RESERVE)
                .ok_or_else(|| Error::other("encoded frame length overflow"))?;
            let desired = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(required)
                .min(maximum);
            self.bytes
                .try_reserve_exact(desired - self.bytes.len())
                .map_err(|_| Error::other("cannot allocate encoded frame"))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

pub(super) fn encode_envelope(
    message: &Message,
    max_body_bytes: usize,
) -> Result<Vec<u8>, EncodeError> {
    let mut output = CappedBody {
        bytes: vec![0; HEADER_RESERVE],
        max_bytes: max_body_bytes,
    };
    to_writer(&mut output, message).map_err(|_| EncodeError {
        message: "response is invalid or exceeds the encoded body limit".into(),
    })?;
    let body_len = output.bytes.len() - HEADER_RESERVE;
    let header = format!("Content-Length: {body_len}\r\n\r\n");
    debug_assert!(header.len() <= HEADER_RESERVE);
    output.bytes.copy_within(HEADER_RESERVE.., header.len());
    output.bytes[..header.len()].copy_from_slice(header.as_bytes());
    output.bytes.truncate(header.len() + body_len);
    Ok(output.bytes)
}
