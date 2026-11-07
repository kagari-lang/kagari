//! Implementation requests yield after one complete context signature shard.

use crate::{
    analysis::{
        AnalysisFailure, AnalysisState,
        features::{
            FeatureInput, FeatureOutput, QueryStep,
            projection::{location, selected_context},
        },
        indexes::{ImplementationIndex, IndexFailure},
    },
    workspace::{ContextKey, budget::InputLease},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use kagari_hir::{
    analysis::signatures::{
        SignatureSnapshot,
        relations::{ImplementationTarget, RelationLimits},
    },
    declarations::DeclarationId,
};
use serde_json::{Value, to_value};
use std::collections::{BTreeMap, BTreeSet};

const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
const MAX_INDEX_SCOPES: usize = 64;

type ScopeStamp = (u64, u64);

struct SelectedTarget {
    scope: ScopeStamp,
    context: ContextKey,
    signatures: SignatureSnapshot,
    target: ImplementationTarget,
    _lease: InputLease,
}

/// One warm scope plus older scopes still owned by admitted requests.
#[derive(Default)]
pub(in crate::analysis) struct IndexCache {
    scopes: BTreeMap<ScopeStamp, ImplementationIndex>,
    requests: BTreeMap<u64, SelectedTarget>,
    live_contexts: BTreeSet<ContextKey>,
    live_generation: u64,
}

impl IndexCache {
    pub(in crate::analysis) fn retained_bytes(&self) -> usize {
        self.scopes
            .values()
            .map(ImplementationIndex::retained_bytes)
            .sum()
    }

    fn retire(&mut self) {
        let newest = self.scopes.keys().next_back().copied().filter(|stamp| {
            stamp.0 == self.live_generation
                && self.scopes[stamp]
                    .capture
                    .contexts
                    .iter()
                    .all(|context| self.live_contexts.contains(&context.key))
        });
        self.scopes.retain(|stamp, _| {
            Some(*stamp) == newest
                || self
                    .requests
                    .values()
                    .any(|request| request.scope == *stamp)
        });
    }

    pub(in crate::analysis) fn retain_contexts(&mut self, live: &BTreeSet<ContextKey>) {
        self.live_contexts = live.clone();
        self.retire();
    }

    fn retain_generation(&mut self, generation: u64) {
        self.live_generation = self.live_generation.max(generation);
        self.retire();
    }
}

impl From<IndexFailure> for AnalysisFailure {
    fn from(error: IndexFailure) -> Self {
        match error {
            IndexFailure::Cancelled => Self::Cancelled,
            IndexFailure::Capacity(message) => Self::Capacity(message),
            IndexFailure::Incomplete(message) => Self::Unavailable(message),
        }
    }
}

impl AnalysisState {
    /// Retires completed older index scopes while preserving explicitly admitted cursors.
    /// Context databases remain owned separately while replacement inputs are acquired.
    pub fn retain_input_generation(&mut self, generation: u64) {
        self.indexes.retain_generation(generation);
    }

    #[cfg(test)]
    pub(crate) fn index_cursor_count(&self) -> usize {
        self.indexes.requests.len()
    }

    /// Drops cancelled/completed request cursors while preserving the newest warm view.
    pub fn retain_requests(&mut self, live: &BTreeSet<u64>) {
        self.indexes
            .requests
            .retain(|serial, _| live.contains(serial));
        self.indexes.retire();
    }

    pub fn implementation(
        &mut self,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<QueryStep, AnalysisFailure> {
        let result = self.implementation_step(input, cancel);
        if !matches!(result, Ok(QueryStep::Continue)) {
            self.indexes.requests.remove(&input.serial);
            self.indexes.retire();
        }
        result
    }

    fn implementation_step(
        &mut self,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<QueryStep, AnalysisFailure> {
        cancel.check()?;
        self.indexes.retain_generation(input.capture.generation);
        self.check_generated_views(&input.capture, &input.documents)?;
        let stamp = (input.capture.generation, 1);
        if !self.indexes.requests.contains_key(&input.serial) {
            let context = selected_context(input)?.key.clone();
            let retained = self.editor_for_input(input, cancel)?;
            let editor = &retained.editor;
            let signatures = editor.signature_snapshot().clone();
            let declaration = editor.definition_at(input.offset);
            let selected = declaration.and_then(|declaration| match declaration.id {
                DeclarationId::Definition(id) => Some(id),
                _ => None,
            });
            let kind = selected.and_then(|id| {
                signatures
                    .definitions()
                    .resolve(id)
                    .ok()?
                    .last()
                    .map(|segment| segment.kind)
            });
            let own_site = declaration.is_some_and(|declaration| {
                declaration.location.file == editor.source().origin_id()
                    && declaration.location.range.start <= input.offset
                    && input.offset < declaration.location.range.end
            });
            let receiver = if own_site
                && matches!(
                    kind,
                    Some(DefinitionKind::Trait | DefinitionKind::Struct | DefinitionKind::Enum)
                ) {
                None
            } else {
                editor
                    .member_receiver_type(input.offset)
                    .or_else(|| editor.type_at(input.offset))
                    .filter(|ty| !ty.is_unresolved())
            };
            if selected.is_none() && receiver.is_none() {
                return FeatureOutput::new(Value::Array(Vec::new()), input.limit)
                    .map(QueryStep::Complete);
            }
            let target = ImplementationTarget {
                declaration: selected
                    .filter(|_| kind != Some(DefinitionKind::TypeAlias) || receiver.is_none()),
                receiver,
                assumptions: editor.generic_bounds(input.offset),
            };
            if !self.indexes.scopes.contains_key(&stamp) {
                self.indexes.retire();
                if self.indexes.scopes.len() >= MAX_INDEX_SCOPES {
                    return Err(AnalysisFailure::Capacity(
                        "retained implementation scope limit exceeded".into(),
                    ));
                }
                let index = ImplementationIndex::new(
                    input.capture.clone(),
                    stamp.1,
                    RelationLimits::default(),
                )?;
                self.indexes.scopes.insert(stamp, index);
            }
            self.indexes.requests.insert(
                input.serial,
                SelectedTarget {
                    scope: stamp,
                    context,
                    signatures,
                    target,
                    _lease: retained._lease,
                },
            );
        }
        let next = self.indexes.scopes[&stamp].next_context().cloned();
        if let Some(context) = next {
            let signatures = self.signatures_for_context(&context, cancel)?;
            let available = MAX_INDEX_BYTES.saturating_sub(self.indexes.retained_bytes());
            self.indexes
                .scopes
                .get_mut(&stamp)
                .expect("admitted implementation scope")
                .insert(
                    &context.key,
                    signatures.snapshot,
                    &self.budget,
                    available,
                    cancel,
                )?;
            drop(signatures._lease);
            return Ok(QueryStep::Continue);
        }
        let request = &self.indexes.requests[&input.serial];
        let index = &self.indexes.scopes[&stamp];
        debug_assert_eq!(index.generation(), input.capture.generation);
        debug_assert_eq!(index.environment_epoch(), stamp.1);
        let destinations = index.query(
            &request.context,
            &request.signatures,
            &request.target,
            cancel,
        )?;
        let mut locations = Vec::new();
        let mut seen = BTreeSet::new();
        let mut projected_bytes = 4096usize;
        for destination in destinations {
            cancel.check()?;
            let context = input
                .capture
                .contexts
                .iter()
                .find(|context| context.key == destination.context)
                .ok_or_else(|| {
                    AnalysisFailure::Unavailable(
                        "implementation destination context was not retained".into(),
                    )
                })?;
            let signatures = index
                .signatures(&destination.context)
                .expect("complete implementation shard");
            let target = location(input, context, signatures, destination.location.selection)?;
            if seen.insert((
                target.uri.as_str().to_owned(),
                target.range.start.line,
                target.range.start.character,
                target.range.end.line,
                target.range.end.character,
            )) {
                let encoded_bound = target
                    .uri
                    .as_str()
                    .len()
                    .saturating_mul(6)
                    .saturating_add(256);
                let value = to_value(target)
                    .map_err(|error| AnalysisFailure::Unavailable(error.to_string()))?;
                let output = FeatureOutput::new(value, input.limit)?;
                // Each element is measured before retaining the next one. Include
                // vector growth and a worst-case JSON URI escape bound, rather
                // than constructing an unbounded typed array before admission.
                projected_bytes = projected_bytes
                    .saturating_add(output.charge.saturating_sub(4096).max(encoded_bound))
                    .saturating_add(size_of::<Value>() * 2);
                if projected_bytes > input.limit {
                    return Err(AnalysisFailure::Capacity(
                        "implementation locations exceed their projection byte limit".into(),
                    ));
                }
                locations.push(output.value);
            }
        }
        FeatureOutput::new(Value::Array(locations), input.limit).map(QueryStep::Complete)
    }
}
