//! Bounded relation cache: scope coverage, retained rows and explicit resource pins.
use crate::{
    analysis::{
        AnalysisFailure,
        indexes::{ImplementationIndex, IndexFailure},
    },
    workspace::{ContextCapture, ContextKey, WorkspaceCapture, budget::InputBudget},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::analysis::signatures::{SignatureSnapshot, relations::RelationLimits};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
const MAX_INDEX_SCOPES: usize = 64;
pub(in crate::analysis) type ScopeStamp = (u64, u64);

/// One warm scope plus older scopes still owned by admitted requests.
#[derive(Default)]
pub(in crate::analysis) struct IndexCache {
    scopes: BTreeMap<ScopeStamp, ImplementationIndex>,
    pins: BTreeSet<ScopeStamp>,
    live_contexts: BTreeSet<ContextKey>,
    live_generation: u64,
}

impl IndexCache {
    pub(in crate::analysis) fn retain_pins(&mut self, pins: BTreeSet<ScopeStamp>) {
        self.pins = pins;
        self.retire();
    }

    pub(in crate::analysis) fn ensure_scope(
        &mut self,
        stamp: ScopeStamp,
        capture: Arc<WorkspaceCapture>,
    ) -> Result<(), AnalysisFailure> {
        if self.scopes.contains_key(&stamp) {
            return Ok(());
        }
        self.retire();
        if self.scopes.len() >= MAX_INDEX_SCOPES {
            return Err(AnalysisFailure::Capacity(
                "retained implementation scope limit exceeded".into(),
            ));
        }
        self.scopes.insert(
            stamp,
            ImplementationIndex::new(capture, stamp.1, RelationLimits::default())?,
        );
        Ok(())
    }

    pub(in crate::analysis) fn scope(&self, stamp: ScopeStamp) -> &ImplementationIndex {
        self.scopes
            .get(&stamp)
            .expect("admitted implementation scope")
    }

    pub(in crate::analysis) fn insert_context(
        &mut self,
        stamp: ScopeStamp,
        context: &ContextCapture,
        signatures: SignatureSnapshot,
        budget: &InputBudget,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisFailure> {
        let available = MAX_INDEX_BYTES.saturating_sub(self.retained_bytes());
        self.scopes
            .get_mut(&stamp)
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
        let newest = self.scopes.keys().next_back().copied().filter(|stamp| {
            stamp.0 == self.live_generation
                && self.scopes[stamp]
                    .capture
                    .contexts
                    .iter()
                    .all(|context| self.live_contexts.contains(&context.key))
        });
        self.scopes
            .retain(|stamp, _| Some(*stamp) == newest || self.pins.contains(stamp));
    }

    pub(in crate::analysis) fn retain_contexts(&mut self, live: &BTreeSet<ContextKey>) {
        self.live_contexts = live.clone();
        self.retire();
    }

    pub(in crate::analysis) fn retain_generation(&mut self, generation: u64) {
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
