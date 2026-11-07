//! Retained query leases and context-owned selected editor analysis.

use crate::{
    analysis::{AnalysisFailure, AnalysisState, ContextAnalysis, features::FeatureInput},
    workspace::{ContextCapture, budget::InputLease},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::analysis::{
    AnalysisDatabase, editor::EditorAnalysis, error::AnalysisError, signatures::SignatureSnapshot,
};

/// Source lease follows the immutable result even when its database is retired.
pub(super) struct RetainedEditor {
    pub editor: EditorAnalysis,
    pub(super) _lease: InputLease,
}

pub(super) struct RetainedSignatures {
    pub snapshot: SignatureSnapshot,
    pub(super) _lease: InputLease,
}

impl AnalysisState {
    pub(super) fn editor_for_input(
        &mut self,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<RetainedEditor, AnalysisFailure> {
        cancel.check()?;
        self.check_generated_views(&input.capture, &input.documents)?;
        if let Some(name) = self
            .generated_document_for(&input.capture, &input.source_name)
            .map(|document| document.source_name().to_owned())
        {
            let baseline = self.environment.as_mut().ok_or_else(|| {
                AnalysisFailure::Unavailable("native environment is not ready".into())
            })?;
            let editor = baseline
                .editor(&name, input.offset, cancel)?
                .ok_or_else(|| {
                    AnalysisFailure::Unavailable(
                        "native view is absent from its validated environment".into(),
                    )
                })?;
            return Ok(RetainedEditor {
                editor,
                _lease: self
                    .environment_lease
                    .clone()
                    .expect("prepared baseline source lease"),
            });
        }
        let selected = input
            .capture
            .document_contexts
            .get(&input.source_name)
            .ok_or_else(|| {
                AnalysisFailure::Unavailable("document has no admitted source context".into())
            })?;
        let context = input
            .capture
            .contexts
            .iter()
            .find(|context| context.key == selected.context)
            .ok_or_else(|| {
                AnalysisFailure::Unavailable("admitted context is unavailable".into())
            })?;
        if !context.semantic_available {
            return Err(AnalysisFailure::Unavailable(
                "document context is unavailable for semantic queries".into(),
            ));
        }
        let (editor, lease) = self.query_context(context, cancel, |database| {
            database.editor(context.sources.clone(), selected.file, input.offset, cancel)
        })?;
        Ok(RetainedEditor {
            editor: editor.ok_or_else(|| {
                AnalysisFailure::Unavailable("selected source file is absent from analysis".into())
            })?,
            _lease: lease,
        })
    }

    pub(super) fn signatures_for_context(
        &mut self,
        context: &ContextCapture,
        cancel: &CancellationToken,
    ) -> Result<RetainedSignatures, AnalysisFailure> {
        if let Some(document) = self.generated_context_document(context) {
            let source = context
                .sources
                .module(&context.root)
                .expect("generated view root source");
            if source.text() != document.text() {
                return Err(AnalysisFailure::Unavailable(
                    "native presentation view differs from its installed source".into(),
                ));
            }
            let snapshot = self
                .environment
                .as_mut()
                .expect("generated document has an environment")
                .signatures(cancel)?;
            return Ok(RetainedSignatures {
                snapshot,
                _lease: self.environment_lease.clone().expect("native source lease"),
            });
        }
        let (snapshot, lease) = self.query_context(context, cancel, |database| {
            database.signatures(context.sources.clone(), cancel)
        })?;
        Ok(RetainedSignatures {
            snapshot,
            _lease: lease,
        })
    }

    /// Ordinary retained reads may be older than the latest background capture.
    /// An older query gets a transient database so its cache cannot displace newer
    /// state. Obsolete identity growth is bounded relative to the current live
    /// declaration inventory, with a 65,536-node minimum allowance. Old results
    /// retain their table and are joined only through explicit provenance.
    pub(super) fn query_context<R>(
        &mut self,
        context: &ContextCapture,
        cancel: &CancellationToken,
        operation: impl FnOnce(&mut AnalysisDatabase) -> Result<R, AnalysisError>,
    ) -> Result<(R, InputLease), AnalysisFailure> {
        cancel.check()?;
        let baseline = self.environment.as_ref().ok_or_else(|| {
            AnalysisFailure::Unavailable("native environment is not ready".into())
        })?;
        let older = self
            .contexts
            .get(&context.key)
            .is_some_and(|state| context.sources.revision() < state.capture.sources.revision());
        if older {
            let mut database = baseline
                .context_database()
                .map_err(AnalysisFailure::Unavailable)?;
            let result = operation(&mut database)?;
            let lease = self
                .budget
                .retain(database.retained_sources())
                .map_err(|error| AnalysisFailure::Capacity(error.to_string()))?;
            return Ok((result, lease));
        }
        if self.contexts.get(&context.key).is_some_and(|state| {
            state.database.definition_count() > state.database.definition_retention_limit()
        }) {
            self.contexts.remove(&context.key);
        }
        let fresh = !self.contexts.contains_key(&context.key);
        if fresh {
            self.contexts.insert(
                context.key.clone(),
                ContextAnalysis {
                    database: baseline
                        .context_database()
                        .map_err(AnalysisFailure::Unavailable)?,
                    capture: context.clone(),
                    lease: InputLease::default(),
                },
            );
        }
        let state = self
            .contexts
            .get_mut(&context.key)
            .expect("admitted query context");
        let result = operation(&mut state.database).map_err(AnalysisFailure::from);
        // Failed first preparation may have initialized native files or interned
        // identities; charge every actual cache owner before retaining the DB.
        let lease = self
            .budget
            .retain(state.database.retained_sources())
            .map_err(|error| AnalysisFailure::Capacity(error.to_string()));
        match (result, lease) {
            (Ok(result), Ok(lease)) => {
                state.capture = context.clone();
                state.lease = lease.clone();
                Ok((result, lease))
            }
            (Err(error), Ok(lease)) => {
                if fresh {
                    self.contexts.remove(&context.key);
                } else {
                    state.lease = lease;
                }
                Err(error)
            }
            (_, Err(error)) => {
                self.contexts.remove(&context.key);
                Err(error)
            }
        }
    }
}
