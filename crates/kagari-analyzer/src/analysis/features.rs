//! Bounded protocol projections from retained source and HIR query owners.
pub(crate) mod completion;
pub(crate) mod navigation;
pub(crate) mod projection;

use crate::{
    analysis::{AnalysisFailure, AnalysisState},
    documents::DocumentInput,
    workspace::WorkspaceCapture,
};
use kagari_common::cancellation::CancellationToken;
use lsp_types::Uri;
use serde_json::{Value, to_writer};
use std::{
    io::{self, Error, Write},
    sync::Arc,
};

#[derive(Clone)]
pub(crate) struct FeatureInput {
    pub serial: u64,
    pub capture: Arc<WorkspaceCapture>,
    pub documents: Arc<[DocumentInput]>,
    pub source_name: String,
    pub uri: Uri,
    pub offset: usize,
    pub limit: usize,
}

pub(crate) struct FeatureOutput {
    pub value: Value,
    pub charge: usize,
}

impl FeatureOutput {
    /// Bound both encoded payload and retained JSON storage before actor delivery.
    pub fn new(value: Value, limit: usize) -> Result<Self, AnalysisFailure> {
        let mut counter = PayloadCounter {
            bytes: 0,
            limit: limit.saturating_sub(4096),
        };
        to_writer(&mut counter, &value).map_err(|_| {
            AnalysisFailure::Capacity("semantic result exceeds its encoded byte limit".into())
        })?;
        let charge = retained_value_bytes(&value).saturating_add(4096);
        if charge > limit {
            return Err(AnalysisFailure::Capacity(
                "semantic result exceeds its retained byte limit".into(),
            ));
        }
        Ok(Self { value, charge })
    }
}

struct PayloadCounter {
    bytes: usize,
    limit: usize,
}

impl Write for PayloadCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            return Err(Error::other("semantic result byte limit"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn retained_value_bytes(value: &Value) -> usize {
    match value {
        Value::String(text) => text.capacity(),
        Value::Array(values) => {
            values.capacity() * size_of::<Value>()
                + values.iter().map(retained_value_bytes).sum::<usize>()
        }
        Value::Object(values) => {
            // Rust 1.99.1 alloc::collections::btree::node has B=6: eleven
            // key/value slots, twelve child edges and a parent/header. Every
            // non-root node owns at least five entries; the root can own one.
            // Charge every possible node as an internal node, including padding.
            // Recheck this conservative allocation bound when changing toolchains.
            let nodes = if values.is_empty() {
                0
            } else {
                1 + values.len() / 5
            };
            let node_bytes =
                11 * (size_of::<String>() + size_of::<Value>()) + 16 * size_of::<usize>();
            nodes * node_bytes
                + values
                    .iter()
                    .map(|(key, value)| key.capacity() + retained_value_bytes(value))
                    .sum::<usize>()
        }
        _ => 0,
    }
}

pub(crate) enum QueryStep {
    Complete(FeatureOutput),
    Continue,
}

pub(crate) trait FeatureQuery: Send + 'static {
    const METHOD: &'static str;
    const WORK_DONE: bool = false;

    fn run(
        state: &mut AnalysisState,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<QueryStep, AnalysisFailure>;
}
