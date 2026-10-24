use crate::state::{
    data::{SnapshotHeader, StateSchema, StateSnapshot, StateValue},
    error::{SnapshotError, SnapshotErrorKind, SnapshotResult, SnapshotStage},
    limits::{Budget, SnapshotOptions, validate_snapshot},
};
use bincode::{DefaultOptions, Options, Serializer};
use serde::Serialize;
use std::io::{self, Error as IoError, Write};

mod decode;
mod encode;

const MAGIC: &[u8; 8] = b"KGRSTATE";
const FRAME_SIZE: usize = 20;

fn options(limit: u64) -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(limit)
}

#[derive(Serialize)]
struct Body<'a> {
    header: &'a SnapshotHeader,
    schema: &'a StateSchema,
    payload: &'a StateValue,
}

struct Sink<'a> {
    budget: &'a mut Budget,
    bytes: Option<Vec<u8>>,
    length: usize,
    failure: Option<SnapshotError>,
}

impl Sink<'_> {
    fn append(&mut self, bytes: &[u8]) -> SnapshotResult<()> {
        let length = self
            .length
            .checked_add(bytes.len())
            .ok_or_else(|| SnapshotError::limit(self.budget.stage, "bytes"))?;
        self.budget.count(
            length,
            self.budget.limits.max_bytes.saturating_sub(FRAME_SIZE),
            "bytes",
        )?;
        for chunk in bytes.chunks(4096) {
            self.budget.bytes(chunk.len())?;
            if let Some(output) = &mut self.bytes {
                self.budget.reserve(output, chunk.len())?;
                output.extend_from_slice(chunk);
            }
        }
        self.length = length;
        Ok(())
    }
}

impl Write for Sink<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.append(bytes) {
            Ok(()) => Ok(bytes.len()),
            Err(error) => {
                self.failure = Some(error);
                Err(IoError::other("snapshot sink rejected write"))
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_body(
    header: &SnapshotHeader,
    schema: &StateSchema,
    payload: &StateValue,
    budget: &mut Budget,
    collect: bool,
) -> SnapshotResult<Vec<u8>> {
    budget.count(FRAME_SIZE, budget.limits.max_bytes, "bytes")?;
    budget.cancel()?;
    let body_limit = (budget.limits.max_bytes - FRAME_SIZE) as u64;
    let mut sink = Sink {
        budget,
        bytes: collect.then(Vec::new),
        length: 0,
        failure: None,
    };
    // Use the serializer directly: Options::serialize_into first runs a size pass
    // without our cancellation-aware sink. The sink independently bounds writes.
    let result = Body {
        header,
        schema,
        payload,
    }
    .serialize(&mut Serializer::new(&mut sink, options(body_limit)));
    if let Some(error) = sink.failure {
        return Err(error);
    }
    result.map_err(|error| {
        sink.budget
            .error(SnapshotErrorKind::InvalidWire, error.to_string())
    })?;
    Ok(sink.bytes.unwrap_or_default())
}

pub(crate) fn check_size(
    header: &SnapshotHeader,
    schema: &StateSchema,
    payload: &StateValue,
    budget: &mut Budget,
) -> SnapshotResult<()> {
    write_body(header, schema, payload, budget, false).map(|_| ())
}

pub(super) fn encode(
    snapshot: &StateSnapshot,
    options: &SnapshotOptions,
) -> SnapshotResult<Vec<u8>> {
    let mut budget = Budget::new(options, SnapshotStage::Encode);
    validate_snapshot(
        snapshot.header(),
        snapshot.schema(),
        snapshot.payload(),
        &mut budget,
    )?;
    let body = write_body(
        snapshot.header(),
        snapshot.schema(),
        snapshot.payload(),
        &mut budget,
        true,
    )?;
    let mut bytes = Vec::new();
    budget.reserve(&mut bytes, FRAME_SIZE + body.len())?;
    budget.work(FRAME_SIZE as u64)?;
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u64).to_le_bytes());
    for chunk in body.chunks(4096) {
        budget.bytes(chunk.len())?;
        bytes.extend_from_slice(chunk);
    }
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8], options: &SnapshotOptions) -> SnapshotResult<StateSnapshot> {
    let mut budget = Budget::new(options, SnapshotStage::Decode);
    budget.count(bytes.len(), budget.limits.max_bytes, "bytes")?;
    budget.work(FRAME_SIZE as u64)?;
    let invalid = || {
        SnapshotError::new(
            SnapshotStage::Decode,
            SnapshotErrorKind::InvalidHeader,
            "invalid snapshot framing",
        )
    };
    if bytes.len() < FRAME_SIZE
        || &bytes[..8] != MAGIC
        || bytes[8..10] != 1_u16.to_le_bytes()
        || bytes[10..12] != 0_u16.to_le_bytes()
    {
        return Err(invalid());
    }
    let length = u64::from_le_bytes(bytes[12..20].try_into().map_err(|_| invalid())?);
    if length != (bytes.len() - FRAME_SIZE) as u64 {
        return Err(invalid());
    }
    decode::body(&bytes[FRAME_SIZE..], &mut budget)
}

#[cfg(test)]
mod tests;
