//! Thread-confined semantic state and owned editor projections.
pub(crate) mod diagnostics;
pub(crate) mod environment;
pub(crate) mod worker;

#[cfg(test)]
mod tests;

use crate::{
    analysis::environment::{PreparedEnvironment, prepare_standard},
    documents::DocumentInput,
    workspace::{
        ContextCapture, ContextKey,
        budget::{InputBudget, InputLease},
    },
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_hir::analysis::{AnalysisDatabase, AnalysisSnapshot, error::AnalysisError};
use kagari_source::generated::{GeneratedDocument, GeneratedDocuments};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    error::Error,
    fmt, iter,
    sync::Arc,
};

#[derive(Debug)]
pub(crate) enum AnalysisFailure {
    Cancelled,
    Capacity(String),
    Unavailable(String),
}

impl fmt::Display for AnalysisFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("analysis cancelled"),
            Self::Capacity(message) => message.fmt(formatter),
            Self::Unavailable(message) => message.fmt(formatter),
        }
    }
}

impl Error for AnalysisFailure {}

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
        self.contexts.clear();
        self.environment = None;
        self.environment_lease = None;
    }

    /// Generated views are presentation files for installed metadata, never inputs
    /// from which a replacement native contract can be inferred.
    pub fn generated_document(&self, source_name: &str) -> Option<&GeneratedDocument> {
        self.environment
            .as_ref()?
            .publications
            .iter()
            .find(|document| document.source_name() == source_name)
    }

    pub fn check_generated_views(
        &self,
        documents: &[DocumentInput],
    ) -> Result<(), AnalysisFailure> {
        for document in documents {
            if let Some(generated) = self.generated_document(&document.source_name)
                && document
                    .source
                    .as_ref()
                    .is_none_or(|source| source.text() != generated.text())
            {
                return Err(AnalysisFailure::Unavailable(format!(
                    "generated native view {} differs from its installed declarations; restore or close the view",
                    document.uri.as_str()
                )));
            }
        }
        Ok(())
    }

    /// Retire databases and their source leases when their contexts disappear.
    pub fn retain_contexts(&mut self, live: &BTreeSet<ContextKey>) {
        self.contexts.retain(|key, _| live.contains(key));
    }

    fn snapshot(
        &mut self,
        context: &ContextCapture,
        cancel: &CancellationToken,
    ) -> Result<RetainedAnalysis, AnalysisFailure> {
        cancel.check()?;
        let baseline = self.environment.as_ref().ok_or_else(|| {
            AnalysisFailure::Unavailable("native environment is not ready".into())
        })?;
        let (state, fresh) = match self.contexts.entry(context.key.clone()) {
            Entry::Occupied(entry) => (entry.into_mut(), false),
            Entry::Vacant(entry) => (
                entry.insert(ContextAnalysis {
                    database: baseline
                        .context_database()
                        .map_err(AnalysisFailure::Unavailable)?,
                    capture: context.clone(),
                    lease: InputLease::default(),
                }),
                true,
            ),
        };
        if context.sources.revision() < state.capture.sources.revision() {
            return Err(AnalysisFailure::Unavailable(
                "analysis received an older context revision".into(),
            ));
        }
        let mut published = false;
        let result = (|| {
            let snapshot = state.database.snapshot(context.sources.clone(), cancel)?;
            published = true;
            let sources = snapshot.declaration_snapshot().files().flat_map(|file| {
                iter::once(file.source())
                    .chain(file.source().copied_sources())
                    .cloned()
                    .map(Arc::new)
            });
            let lease = self
                .budget
                .retain(sources)
                .map_err(|error| AnalysisFailure::Capacity(error.to_string()))?;
            state.capture = context.clone();
            state.lease = lease.clone();
            Ok(RetainedAnalysis {
                snapshot,
                _lease: lease,
            })
        })();
        if result.is_err() && (fresh || published) {
            // First preparation may initialize uncharged native sources. A
            // published replacement that fails admission also cannot stay cached.
            // Transactional failure on an admitted database preserves its old lease.
            self.contexts.remove(&context.key);
        }
        result
    }
}
