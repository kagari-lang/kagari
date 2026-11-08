//! Thread-confined semantic state and owned editor projections.
pub(crate) mod diagnostics;
pub(crate) mod editor;
pub(crate) mod environment;
pub(crate) mod features;
mod generated_views;
pub(crate) mod indexes;
pub(crate) mod worker;

#[cfg(any(test, feature = "verification"))]
pub(crate) mod verification;

#[cfg(test)]
mod tests;

use crate::{
    analysis::{
        environment::{PreparedEnvironment, prepare_standard},
        features::navigation::implementation::ImplementationSearches,
        indexes::cache::IndexCache,
    },
    workspace::{
        ContextCapture, ContextKey,
        budget::{InputBudget, InputLease},
    },
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_hir::analysis::{AnalysisDatabase, AnalysisSnapshot, error::AnalysisError};
use kagari_source::generated::{GeneratedDocument, GeneratedDocuments};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum AnalysisFailure {
    #[error("analysis cancelled")]
    Cancelled,
    #[error("{0}")]
    Capacity(String),
    #[error("{0}")]
    Unavailable(String),
}

impl From<Cancelled> for AnalysisFailure {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

impl From<AnalysisError> for AnalysisFailure {
    fn from(error: AnalysisError) -> Self {
        match error {
            AnalysisError::Cancelled => Self::Cancelled,
            error => Self::Unavailable(error.to_string()),
        }
    }
}

struct ContextAnalysis {
    database: AnalysisDatabase,
    /// Full-snapshot publication replaces every user input-bearing cache atomically.
    /// Previously admitted caches and leases survive cancelled preparation.
    capture: ContextCapture,
    lease: InputLease,
}

/// A retained semantic result always keeps its input allocations charged, even
/// after the mutable context database is retired or replaced.
struct RetainedAnalysis {
    snapshot: AnalysisSnapshot,
    _lease: InputLease,
}

/// Constructed and accessed only by the dedicated analysis thread.
pub(crate) struct AnalysisState {
    source_root: String,
    budget: InputBudget,
    projection_limits: diagnostics::ProjectionLimits,
    environment: Option<PreparedEnvironment>,
    environment_lease: Option<InputLease>,
    contexts: BTreeMap<ContextKey, ContextAnalysis>,
    indexes: IndexCache,
    implementation: ImplementationSearches,
}

impl AnalysisState {
    pub(super) fn new(
        source_root: String,
        budget: InputBudget,
        projection_limits: diagnostics::ProjectionLimits,
    ) -> Self {
        Self {
            source_root,
            budget,
            projection_limits,
            environment: None,
            environment_lease: None,
            contexts: BTreeMap::new(),
            indexes: IndexCache::default(),
            implementation: ImplementationSearches::default(),
        }
    }

    /// Validate and warm the complete native batch without filesystem access.
    /// Publication of the returned plans belongs to the acquisition thread.
    pub fn prepare(
        &mut self,
        documents: &GeneratedDocuments,
        cancel: &CancellationToken,
    ) -> Result<Arc<[GeneratedDocument]>, AnalysisFailure> {
        self.discard_environment();
        cancel.check()?;
        let prepared =
            prepare_standard(&self.source_root, documents, cancel).map_err(|message| {
                if cancel.check().is_err() {
                    AnalysisFailure::Cancelled
                } else {
                    AnalysisFailure::Unavailable(message)
                }
            })?;
        cancel.check()?;
        let lease = self
            .budget
            .retain(prepared.sources.iter().cloned())
            .map_err(|error| AnalysisFailure::Unavailable(error.to_string()))?;
        let publications = prepared.publications.clone();
        self.environment = Some(prepared);
        self.environment_lease = Some(lease);
        Ok(publications)
    }

    /// Retire a baseline whose publication failed. A validated but unpublished
    /// native source cannot become a semantic or navigation dependency.
    pub fn discard_environment(&mut self) {
        self.implementation = ImplementationSearches::default();
        self.indexes = IndexCache::default();
        self.contexts.clear();
        self.environment = None;
        self.environment_lease = None;
    }

    /// Retire databases and their source leases when their contexts disappear.
    pub fn retain_contexts(&mut self, live: &BTreeSet<ContextKey>) {
        self.indexes.retain_contexts(live);
        self.contexts.retain(|key, _| live.contains(key));
    }

    fn snapshot(
        &mut self,
        context: &ContextCapture,
        cancel: &CancellationToken,
    ) -> Result<RetainedAnalysis, AnalysisFailure> {
        let (snapshot, lease) = self.query_context(context, cancel, |database| {
            database.snapshot(context.sources.clone(), cancel)
        })?;
        Ok(RetainedAnalysis {
            snapshot,
            _lease: lease,
        })
    }
}

impl AnalysisState {
    pub fn retain_input_generation(&mut self, generation: u64) {
        self.indexes.retain_generation(generation);
    }

    #[cfg(test)]
    pub(crate) fn index_cursor_count(&self) -> usize {
        self.implementation.len()
    }

    pub fn retain_requests(&mut self, live: &BTreeSet<u64>) {
        self.implementation.retain_requests(live);
        self.indexes.retain_pins(self.implementation.pins());
    }
}
