//! Bounded relation cache: scope coverage, retained rows and explicit resource pins.
use crate::{
    analysis::{
        AnalysisFailure,
        indexes::{
            ImplementationIndex, IndexFailure,
            scope::{IndexScope, ScopeStamp},
        },
    },
    workspace::{ContextCapture, ContextKey, WorkspaceCapture, budget::InputBudget},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::analysis::signatures::{SignatureSnapshot, relations::RelationLimits};
use std::collections::{BTreeMap, BTreeSet};

const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
const MAX_INDEX_SCOPES: usize = 64;

/// Bounded warm scopes plus older scopes still pinned by admitted requests.
/// The cache is destroyed whenever the installed native environment is replaced.
#[derive(Default)]
pub(in crate::analysis) struct IndexCache {
    scopes: BTreeMap<ScopeStamp, ImplementationIndex>,
    pins: BTreeSet<ScopeStamp>,
    live_contexts: BTreeSet<ContextKey>,
    live_generation: u64,
    live_inputs: BTreeMap<ContextKey, u64>,
}

impl IndexCache {
    pub(in crate::analysis) fn retain_pins(&mut self, pins: BTreeSet<ScopeStamp>) {
        self.pins = pins;
        self.retire();
    }

    pub(in crate::analysis) fn ensure_scope(
        &mut self,
        scope: IndexScope,
    ) -> Result<ScopeStamp, AnalysisFailure> {
        let stamp = scope.stamp();
        if self.scopes.contains_key(&stamp) {
            return Ok(stamp);
        }
        self.retire();
        if self.scopes.len() >= MAX_INDEX_SCOPES {
            // Completed warm scopes are expendable; admitted reads are not.
            let victim = self
                .scopes
                .keys()
                .find(|stamp| !self.pins.contains(*stamp))
                .cloned();
            if let Some(victim) = victim {
                self.scopes.remove(&victim);
            } else {
                return Err(AnalysisFailure::Capacity(
                    "retained implementation scope limit exceeded".into(),
                ));
            }
        }
        self.scopes.insert(
            stamp.clone(),
            ImplementationIndex::new(scope, RelationLimits::default())?,
        );
        Ok(stamp)
    }

    pub(in crate::analysis) fn scope(&self, stamp: &ScopeStamp) -> &ImplementationIndex {
        self.scopes
            .get(stamp)
            .expect("admitted implementation scope")
    }

    pub(in crate::analysis) fn insert_context(
        &mut self,
        stamp: &ScopeStamp,
        context: &ContextCapture,
        signatures: SignatureSnapshot,
        budget: &InputBudget,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisFailure> {
        let available = MAX_INDEX_BYTES.saturating_sub(self.retained_bytes());
        self.scopes
            .get_mut(stamp)
            .expect("admitted implementation scope")
            .insert(&context.key, signatures, budget, available, cancel)?;
        Ok(())
    }

    pub(in crate::analysis) fn retained_bytes(&self) -> usize {
        self.scopes
            .values()
            .map(ImplementationIndex::retained_bytes)
            .sum()
    }

    fn retire(&mut self) {
        self.scopes.retain(|stamp, _| {
            self.pins.contains(stamp)
                || stamp.inputs.iter().all(|input| {
                    self.live_contexts.contains(&input.context)
                        && self.live_inputs.get(&input.context) == Some(&input.input)
                })
        });
    }

    pub(in crate::analysis) fn retain_contexts(&mut self, live: &BTreeSet<ContextKey>) {
        self.live_contexts = live.clone();
        self.retire();
    }

    /// Admission order prevents older reads from replacing current validity.
    /// Generation itself never participates in semantic cache identity.
    pub(in crate::analysis) fn retain_capture(&mut self, capture: &WorkspaceCapture) {
        if capture.generation > self.live_generation {
            self.live_generation = capture.generation;
            self.live_inputs = capture
                .contexts
                .iter()
                .map(|context| (context.key.clone(), context.input_revision))
                .collect();
            self.retire();
        }
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
