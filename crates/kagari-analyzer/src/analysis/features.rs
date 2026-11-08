//! Bounded protocol projections from retained source and HIR query owners.
pub(crate) mod completion;
pub(crate) mod navigation;
pub(crate) mod projection;

use crate::{
    analysis::{AnalysisFailure, AnalysisState},
    documents::DocumentInput,
    protocol::typed::check_encoded_size,
    workspace::WorkspaceCapture,
};
use kagari_common::cancellation::CancellationToken;
use lsp_types::{ProgressToken, TextDocumentPositionParams, Uri, request::Request as LspRequest};
use serde::Serialize;
use serde_json::{Value, to_value};
use std::sync::Arc;

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
    /// Erase a method-bound result only at the bounded protocol output boundary.
    pub fn encode<T: Serialize>(value: T, limit: usize) -> Result<Self, AnalysisFailure> {
        let value = to_value(value).map_err(projection::conversion)?;
        Self::new(value, limit)
    }

    /// Bound both encoded payload and retained JSON storage before actor delivery.
    pub fn new(value: Value, limit: usize) -> Result<Self, AnalysisFailure> {
        check_encoded_size(&value, limit.saturating_sub(4096)).map_err(|_| {
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

pub(crate) enum QueryStep<T> {
    Complete(T),
    Continue,
}

pub(crate) type QueryParams<Q> = <<Q as FeatureQuery>::Request as LspRequest>::Params;
pub(crate) type QueryResult<Q> = <<Q as FeatureQuery>::Request as LspRequest>::Result;

pub(crate) trait FeatureQuery: Send + Sized + 'static {
    type Request: LspRequest;
    const WORK_DONE: bool = false;

    fn position(params: &QueryParams<Self>) -> &TextDocumentPositionParams;

    fn work_done(params: &QueryParams<Self>) -> Option<&ProgressToken>;

    fn run(
        state: &mut AnalysisState,
        input: &FeatureInput,
        params: &QueryParams<Self>,
        cancel: &CancellationToken,
    ) -> Result<QueryStep<QueryResult<Self>>, AnalysisFailure>;
}
