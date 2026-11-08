//! Method-bound parameters; decoding failures remain owned by session admission.
use crate::protocol::{Notification, Request, RequestId};
use lsp_types::{notification::Notification as LspNotification, request::Request as LspRequest};
use serde::Serialize;
use serde_json::Error as JsonError;
use serde_json::{Value, from_value, to_value, to_writer};
use std::io::{self, Error, Write};

/// Keeps malformed parameters distinguishable without retaining raw JSON or error strings.
#[derive(Debug)]
pub(crate) struct InvalidParams;

pub(crate) struct TypedRequest<R: LspRequest> {
    pub id: RequestId,
    pub params: Result<R::Params, InvalidParams>,
}

impl<R: LspRequest> TypedRequest<R> {
    pub(super) fn decode(request: Request) -> Self {
        Self {
            id: request.id,
            params: from_value(request.params).map_err(|_| InvalidParams),
        }
    }
}

pub(crate) struct TypedNotification<N: LspNotification> {
    pub params: Result<N::Params, InvalidParams>,
}

impl<N: LspNotification> TypedNotification<N> {
    pub(super) fn decode(notification: Notification) -> Self {
        Self {
            params: from_value(notification.params).map_err(|_| InvalidParams),
        }
    }
}

/// Serialize only the result type paired with this method descriptor.
pub(crate) fn encode_result<R: LspRequest>(result: R::Result) -> Result<Value, JsonError> {
    to_value(result)
}

/// Count serialized bytes without allocating a second encoded payload.
pub(crate) fn check_encoded_size<T: Serialize>(value: &T, limit: usize) -> Result<(), JsonError> {
    to_writer(&mut PayloadCounter { bytes: 0, limit }, value)
}

struct PayloadCounter {
    bytes: usize,
    limit: usize,
}

impl Write for PayloadCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            return Err(Error::other("encoded protocol byte limit"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
