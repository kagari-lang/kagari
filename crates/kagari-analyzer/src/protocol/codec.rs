//! Bounded incremental framing with an explicit allocation-admission boundary.
use std::{mem::take, str::from_utf8};

/// Limits count wire bytes, including header terminators and encoded JSON bodies.
#[derive(Clone, Copy, Debug)]
pub struct CodecLimits {
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
}

impl Default for CodecLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 8 * 1024,
            max_body_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Framing failure is terminal: subsequent input cannot restore synchronization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("LSP header exceeds its byte limit")]
    HeaderTooLarge,
    #[error("invalid LSP header")]
    InvalidHeader,
    #[error("LSP header is missing Content-Length")]
    MissingContentLength,
    #[error("duplicate LSP Content-Length")]
    DuplicateContentLength,
    #[error("Content-Length must be an unsigned decimal length")]
    InvalidContentLength,
    #[error("LSP body exceeds its byte limit")]
    BodyTooLarge,
    #[error("unsupported LSP content encoding")]
    UnsupportedEncoding,
    #[error("EOF within an LSP frame")]
    Truncated,
    #[error("cannot allocate LSP body")]
    AllocationFailed,
    #[error("LSP body allocation was not awaiting admission")]
    BudgetNotRequested,
    #[error("LSP framing has already failed")]
    Failed,
}

/// A decoder boundary; the caller retains any bytes after `consumed`.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameEvent {
    NeedMore,
    /// Acquire the encoded-body permit before calling `grant_body`.
    BodyLength(usize),
    /// The body and its application permit travel together through dispatch.
    Frame(Vec<u8>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct FrameProgress {
    pub consumed: usize,
    pub event: FrameEvent,
}

#[derive(Clone, Copy)]
enum State {
    Header,
    AwaitBodyBudget(usize),
    Body(usize),
    Failed,
}

/// Shared codec for blocking stdio, asynchronous TCP and memory test ports.
///
/// `consume` stops after each header and complete body. No body storage is
/// allocated until the adapter admits the announced length with `grant_body`.
/// State survives cancelled reads; a failed stream must be closed, not retried.
pub struct FrameDecoder {
    limits: CodecLimits,
    state: State,
    header: Vec<u8>,
    body: Vec<u8>,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new(CodecLimits::default())
    }
}

impl FrameDecoder {
    pub fn new(limits: CodecLimits) -> Self {
        Self {
            limits,
            state: State::Header,
            header: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Consume only the returned prefix; the same input suffix may hold more frames.
    pub fn consume(&mut self, input: &[u8]) -> Result<FrameProgress, FrameError> {
        let result = self.consume_inner(input);
        if result.is_err() {
            self.state = State::Failed;
            self.body = Vec::new();
        }
        result
    }

    fn consume_inner(&mut self, input: &[u8]) -> Result<FrameProgress, FrameError> {
        match self.state {
            State::Header => {
                for (index, byte) in input.iter().copied().enumerate() {
                    if self.header.len() >= self.limits.max_header_bytes {
                        return Err(FrameError::HeaderTooLarge);
                    }
                    self.header.push(byte);
                    if self.header.ends_with(b"\r\n\r\n") {
                        let length = parse_header(&self.header, self.limits.max_body_bytes)?;
                        self.header.clear();
                        self.state = State::AwaitBodyBudget(length);
                        return Ok(FrameProgress {
                            consumed: index + 1,
                            event: FrameEvent::BodyLength(length),
                        });
                    }
                }
                Ok(FrameProgress {
                    consumed: input.len(),
                    event: FrameEvent::NeedMore,
                })
            }
            State::AwaitBodyBudget(length) => Ok(FrameProgress {
                consumed: 0,
                event: FrameEvent::BodyLength(length),
            }),
            State::Body(length) => {
                let consumed = (length - self.body.len()).min(input.len());
                self.body.extend_from_slice(&input[..consumed]);
                let event = if self.body.len() == length {
                    self.state = State::Header;
                    FrameEvent::Frame(take(&mut self.body))
                } else {
                    FrameEvent::NeedMore
                };
                Ok(FrameProgress { consumed, event })
            }
            State::Failed => Err(FrameError::Failed),
        }
    }

    /// Allocate the admitted body. The adapter owns and retains its byte permit.
    pub fn grant_body(&mut self) -> Result<(), FrameError> {
        let State::AwaitBodyBudget(length) = self.state else {
            return Err(FrameError::BudgetNotRequested);
        };
        if self.body.try_reserve_exact(length).is_err() {
            self.state = State::Failed;
            return Err(FrameError::AllocationFailed);
        }
        self.state = State::Body(length);
        Ok(())
    }

    /// EOF is clean only between frames; any truncated frame poisons the decoder.
    pub fn finish(&mut self) -> Result<(), FrameError> {
        match self.state {
            State::Header if self.header.is_empty() => Ok(()),
            State::Failed => Err(FrameError::Failed),
            _ => {
                self.state = State::Failed;
                self.body = Vec::new();
                Err(FrameError::Truncated)
            }
        }
    }
}

fn parse_header(bytes: &[u8], max_body_bytes: usize) -> Result<usize, FrameError> {
    if !bytes.is_ascii() {
        return Err(FrameError::InvalidHeader);
    }
    let header = from_utf8(&bytes[..bytes.len() - 4]).map_err(|_| FrameError::InvalidHeader)?;
    let mut content_length = None;
    let mut content_type_seen = false;
    for line in header.split("\r\n") {
        let (name, value) = line.split_once(':').ok_or(FrameError::InvalidHeader)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(FrameError::InvalidHeader);
        }
        if value
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err(FrameError::InvalidHeader);
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(FrameError::DuplicateContentLength);
            }
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(FrameError::InvalidContentLength);
            }
            let length = value
                .parse::<usize>()
                .map_err(|_| FrameError::InvalidContentLength)?;
            if length > max_body_bytes {
                return Err(FrameError::BodyTooLarge);
            }
            content_length = Some(length);
        } else if name.eq_ignore_ascii_case("Content-Type") {
            if content_type_seen {
                return Err(FrameError::InvalidHeader);
            }
            content_type_seen = true;
            validate_content_type(value)?;
        } else if name.eq_ignore_ascii_case("Content-Encoding")
            && !value.eq_ignore_ascii_case("identity")
        {
            return Err(FrameError::UnsupportedEncoding);
        }
    }
    content_length.ok_or(FrameError::MissingContentLength)
}

fn validate_content_type(value: &str) -> Result<(), FrameError> {
    let mut parts = value.split(';');
    if !parts.next().is_some_and(|mime| {
        mime.trim()
            .eq_ignore_ascii_case("application/vscode-jsonrpc")
    }) {
        return Err(FrameError::UnsupportedEncoding);
    }
    let mut charset_seen = false;
    for part in parts {
        let (name, value) = part
            .trim()
            .split_once('=')
            .ok_or(FrameError::InvalidHeader)?;
        if !name.trim().eq_ignore_ascii_case("charset") || charset_seen {
            return Err(FrameError::UnsupportedEncoding);
        }
        charset_seen = true;
        let value = value.trim();
        if !value.eq_ignore_ascii_case("utf-8") && !value.eq_ignore_ascii_case("utf8") {
            return Err(FrameError::UnsupportedEncoding);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
