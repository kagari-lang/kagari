//! Bounded protocol projections from retained source and HIR query owners.
pub(crate) mod completion;
pub(crate) mod navigation;
pub(crate) mod projection;

use crate::{
    analysis::AnalysisFailure,
    documents::DocumentInput,
    protocol::{output::MESSAGE_BASE_CHARGE_BYTES, typed::check_encoded_size},
    workspace::WorkspaceCapture,
};
use lsp_types::{Uri, request::Request as LspRequest};
use serde::Serialize;
use serde_json::{Value, to_value};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct DocumentQueryInput {
    pub serial: u64,
    pub capture: Arc<WorkspaceCapture>,
    pub documents: Arc<[DocumentInput]>,
    pub source_name: String,
    pub uri: Uri,
    pub offset: usize,
    pub limit: usize,
}

pub(crate) struct ProtocolOutput {
    pub value: Value,
    pub charge: usize,
}

impl ProtocolOutput {
    pub fn for_request<R: LspRequest>(
        value: R::Result,
        limit: usize,
    ) -> Result<Self, AnalysisFailure> {
        Self::encode(value, limit)
    }

    /// Erase a method-bound result only at the bounded protocol output boundary.
    pub fn encode<T: Serialize>(value: T, limit: usize) -> Result<Self, AnalysisFailure> {
        let value = to_value(value).map_err(projection::conversion)?;
        Self::new(value, limit)
    }

    /// Bound both encoded payload and retained JSON storage before actor delivery.
    pub fn new(value: Value, limit: usize) -> Result<Self, AnalysisFailure> {
        check_encoded_size(&value, limit.saturating_sub(MESSAGE_BASE_CHARGE_BYTES)).map_err(
            |_| AnalysisFailure::Capacity("semantic result exceeds its encoded byte limit".into()),
        )?;
        let charge = retained_value_bytes(&value).saturating_add(MESSAGE_BASE_CHARGE_BYTES);
        if charge > limit {
            return Err(AnalysisFailure::Capacity(
                "semantic result exceeds its retained byte limit".into(),
            ));
        }
        Ok(Self { value, charge })
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
