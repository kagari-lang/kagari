//! Implementation-specific target selection, search lifetime and LSP presentation.
use crate::{
    analysis::{
        AnalysisFailure, AnalysisState,
        features::{
            DocumentQueryInput, ProtocolOutput,
            projection::{location, selected_context},
        },
        indexes::cache::ScopeStamp,
    },
    workspace::{ContextKey, budget::InputLease},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use kagari_hir::{
    analysis::signatures::{SignatureSnapshot, relations::ImplementationTarget},
    declarations::DeclarationId,
};
use lsp_types::{GotoDefinitionResponse, Location};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) enum ImplementationStep {
    Complete(Option<GotoDefinitionResponse>),
    Continue,
}

struct SelectedTarget {
    scope: ScopeStamp,
    context: ContextKey,
    signatures: SignatureSnapshot,
    target: ImplementationTarget,
    _lease: InputLease,
}

#[derive(Default)]
pub(in crate::analysis) struct ImplementationSearches {
    requests: BTreeMap<u64, SelectedTarget>,
}

impl ImplementationSearches {
    pub(in crate::analysis) fn pins(&self) -> BTreeSet<ScopeStamp> {
        self.requests
            .values()
            .map(|request| request.scope)
            .collect()
    }

    pub(in crate::analysis) fn retain_requests(&mut self, live: &BTreeSet<u64>) {
        self.requests.retain(|serial, _| live.contains(serial));
    }

    #[cfg(test)]
    pub(in crate::analysis) fn len(&self) -> usize {
        self.requests.len()
    }
}

impl AnalysisState {
    pub fn implementation(
        &mut self,
        input: &DocumentQueryInput,
        cancel: &CancellationToken,
    ) -> Result<ImplementationStep, AnalysisFailure> {
        let result = self.implementation_step(input, cancel);
        if !matches!(result, Ok(ImplementationStep::Continue)) {
            self.implementation.requests.remove(&input.serial);
            self.indexes.retain_pins(self.implementation.pins());
        }
        result
    }

    fn implementation_step(
        &mut self,
        input: &DocumentQueryInput,
        cancel: &CancellationToken,
    ) -> Result<ImplementationStep, AnalysisFailure> {
        cancel.check()?;
        self.indexes.retain_generation(input.capture.generation);
        self.check_generated_views(&input.capture, &input.documents)?;
        let stamp = (input.capture.generation, 1);
        if !self.implementation.requests.contains_key(&input.serial) {
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
                return Ok(ImplementationStep::Complete(Some(
                    GotoDefinitionResponse::Array(Vec::new()),
                )));
            }
            let target = ImplementationTarget {
                declaration: selected
                    .filter(|_| kind != Some(DefinitionKind::TypeAlias) || receiver.is_none()),
                receiver,
                assumptions: editor.generic_bounds(input.offset),
            };
            self.indexes.ensure_scope(stamp, input.capture.clone())?;
            self.implementation.requests.insert(
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
        self.indexes.retain_pins(self.implementation.pins());
        let next = self.indexes.scope(stamp).next_context().cloned();
        if let Some(context) = next {
            let signatures = self.signatures_for_context(&context, cancel)?;
            self.indexes.insert_context(
                stamp,
                &context,
                signatures.snapshot,
                &self.budget,
                cancel,
            )?;
            drop(signatures._lease);
            return Ok(ImplementationStep::Continue);
        }
        let request = &self.implementation.requests[&input.serial];
        let index = self.indexes.scope(stamp);
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
                let output = ProtocolOutput::encode(&target, input.limit)?;
                // Each element is measured before retaining the next one. Include
                // vector growth and a worst-case JSON URI escape bound, rather
                // than constructing an unbounded typed array before admission.
                projected_bytes = projected_bytes
                    .saturating_add(output.charge.saturating_sub(4096).max(encoded_bound))
                    .saturating_add(size_of::<Value>().max(size_of::<Location>()) * 2);
                if projected_bytes > input.limit {
                    return Err(AnalysisFailure::Capacity(
                        "implementation locations exceed their projection byte limit".into(),
                    ));
                }
                locations.push(target);
            }
        }
        Ok(ImplementationStep::Complete(Some(
            GotoDefinitionResponse::Array(locations),
        )))
    }
}
