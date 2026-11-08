//! Implementation-specific target selection, search lifetime and LSP presentation.
mod scope;

use crate::{
    analysis::{
        AnalysisFailure, AnalysisState,
        features::{
            DocumentQueryInput, ProtocolOutput,
            projection::{location, selected_context},
        },
        indexes::scope::ScopeStamp,
    },
    protocol::output::MESSAGE_BASE_CHARGE_BYTES,
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
#[cfg(any(test, feature = "verification"))]
use std::time::Instant;

#[cfg(any(test, feature = "verification"))]
#[derive(Clone, Copy, Default)]
pub(in crate::analysis) struct SearchWork {
    pub candidates: usize,
    pub selected: usize,
    pub selection_ns: u128,
    pub indexed: usize,
}

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
    #[cfg(any(test, feature = "verification"))]
    pub(in crate::analysis) work: SearchWork,
}

impl ImplementationSearches {
    pub(in crate::analysis) fn pins(&self) -> BTreeSet<ScopeStamp> {
        self.requests
            .values()
            .map(|request| request.scope.clone())
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
        self.indexes.retain_capture(&input.capture);
        self.check_generated_views(&input.capture, &input.documents)?;
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
            #[cfg(any(test, feature = "verification"))]
            let selection_started = Instant::now();
            let search = scope::select(
                &input.capture,
                selected_context(input)?,
                &signatures,
                &target,
                cancel,
            )?;
            #[cfg(any(test, feature = "verification"))]
            {
                self.implementation.work.candidates += input.capture.contexts.len();
                self.implementation.work.selected += search.contexts.len();
                self.implementation.work.selection_ns += selection_started.elapsed().as_nanos();
            }
            let stamp = self.indexes.ensure_scope(search)?;
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
        let stamp = self.implementation.requests[&input.serial].scope.clone();
        let next = self.indexes.scope(&stamp).next_context().cloned();
        if let Some(context) = next {
            let signatures = self.signatures_for_context(&context, cancel)?;
            self.indexes.insert_context(
                &stamp,
                &context,
                signatures.snapshot,
                &self.budget,
                cancel,
            )?;
            #[cfg(any(test, feature = "verification"))]
            {
                self.implementation.work.indexed += 1;
            }
            drop(signatures._lease);
            return Ok(ImplementationStep::Continue);
        }
        let request = &self.implementation.requests[&input.serial];
        let index = self.indexes.scope(&stamp);
        let destinations = index.query(
            &request.context,
            &request.signatures,
            &request.target,
            cancel,
        )?;
        let mut locations = Vec::new();
        let mut seen = BTreeSet::new();
        let mut projected_bytes = MESSAGE_BASE_CHARGE_BYTES;
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
                    .saturating_add(
                        output
                            .charge
                            .saturating_sub(MESSAGE_BASE_CHARGE_BYTES)
                            .max(encoded_bound),
                    )
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
