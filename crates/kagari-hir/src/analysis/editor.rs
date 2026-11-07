//! Position-selected editor facts retain their source and identity owners.

use crate::typeck::GenericBounds;
use crate::{
    aggregates::implementations::ImplementationSearchError,
    analysis::{
        AnalysisDatabase, BindingInfo, FileAnalysis,
        body::FunctionAnalysis,
        error::AnalysisError,
        projection::EditorFacts,
        signatures::{FileSignatures, SignatureSnapshot},
    },
    declarations::{Declaration, DeclarationId},
    imports::{ResolvedTarget, SourceItem},
    queries::{
        documentation::DeclarationDocumentation,
        methods::{self, MethodCompletion},
    },
    resolver::resolved::ResolvedName,
    types::TypeId,
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};
use kagari_source::{identity::FileId, source::SourceFile, source_database::SourceSnapshot};
use std::sync::Arc;

pub mod completion;

/// Retained signature and position-owned editor facts.
///
/// Function positions use selected-body analysis. Module const/static initializer
/// positions currently retain a full-file result from a complete snapshot; this
/// explicit fallback can check unrelated bodies and is counted as full analysis.
///
/// Offsets are UTF-8 byte offsets in the retained physical source. Inline modules
/// use their own logical lowering while preserving physical offsets. Missing facts
/// in malformed source return `None`. Header and function positions do not request
/// the complete snapshot fallback.
#[derive(Debug, Clone)]
pub struct EditorAnalysis {
    signatures: SignatureSnapshot,
    file: Arc<FileSignatures>,
    body: Option<EditorBody>,
}

#[derive(Debug, Clone)]
enum EditorBody {
    Function(Arc<FunctionAnalysis>),
    Initializer(Arc<FileAnalysis>),
}

impl AnalysisDatabase {
    /// Prepares the innermost logical file and routes the position to its semantic owner.
    ///
    /// Header positions do not require a body. Const/static initializer positions
    /// currently use the complete snapshot fallback; this may check other bodies.
    /// Nested local functions select
    /// the innermost body; lexical ancestor declarations remain available. Errors
    /// follow [`Self::body`], and cancellation never publishes a partial result.
    pub fn editor(
        &mut self,
        source: SourceSnapshot,
        file: FileId,
        offset: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<EditorAnalysis>, AnalysisError> {
        let signatures = self.prepare_signatures(source.clone(), cancel)?;
        let Some(physical) = signatures.file(file) else {
            return Ok(None);
        };
        let file = signatures
            .files
            .values()
            .filter(|candidate| {
                candidate.source().origin_id() == file
                    && candidate
                        .source()
                        .inline_range()
                        .is_some_and(|range| range.start <= offset && offset < range.end)
            })
            .min_by_key(|candidate| {
                let range = candidate.source().inline_range().expect("inline candidate");
                range.end - range.start
            })
            .unwrap_or(physical)
            .clone();
        let owner = file
            .prepared
            .lowered
            .module
            .functions
            .iter()
            .filter(|function| {
                function.body.is_some_and(|body| {
                    let span = file.prepared.lowered.source_map.block_span(body);
                    span.start <= offset && offset <= span.end
                })
            })
            .min_by_key(|function| {
                let span = file.prepared.lowered.source_map.function_span(function.id);
                span.end - span.start
            })
            .and_then(|function| {
                file.declarations()
                    .target(ResolvedName::Function(function.id))
            })
            .and_then(|declaration| match declaration.id {
                DeclarationId::Definition(id) => Some(id),
                _ => None,
            });
        if owner.is_none()
            && file
                .prepared
                .lowered
                .module
                .consts
                .iter()
                .map(|item| item.initializer)
                .chain(
                    file.prepared
                        .lowered
                        .module
                        .statics
                        .iter()
                        .map(|item| item.initializer),
                )
                .any(|initializer| {
                    let span = file.prepared.lowered.source_map.expr_span(initializer);
                    span.start < span.end && span.start <= offset && offset <= span.end
                })
        {
            // Reuse the existing complete owner rather than treating an initializer
            // as a function or copying its checker into the editor adapter. Do not
            // publish this draft signature context before the full query completes.
            let snapshot = self.snapshot(source, cancel)?;
            let logical = file.source().id();
            let signatures = snapshot.signature_snapshot().clone();
            let file = signatures
                .file(logical)
                .expect("initializer signature file")
                .clone();
            let analysis = snapshot
                .file(logical)
                .expect("initializer analysis file")
                .clone();
            return Ok(Some(EditorAnalysis {
                signatures,
                file,
                body: Some(EditorBody::Initializer(analysis)),
            }));
        }
        let body = if let Some(owner) = owner {
            self.body_with_signatures(signatures.clone(), &owner, cancel)?
                .map(EditorBody::Function)
        } else {
            cancel.check()?;
            self.publish_signatures(signatures.clone());
            None
        };
        Ok(Some(EditorAnalysis {
            signatures,
            file,
            body,
        }))
    }
}

impl EditorAnalysis {
    /// The logical source owning the selected position; inline views retain their physical origin.
    pub fn source(&self) -> &SourceFile {
        self.file.source()
    }

    /// Checked signature context retained by this query.
    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    /// Selected function facts, absent for headers and the initializer fallback.
    pub fn body(&self) -> Option<&Arc<FunctionAnalysis>> {
        match &self.body {
            Some(EditorBody::Function(body)) => Some(body),
            _ => None,
        }
    }

    fn editor_facts(&self) -> Option<EditorFacts<'_>> {
        match self.body.as_ref()? {
            EditorBody::Function(body) => Some(body.editor_facts()),
            EditorBody::Initializer(file) => Some(file.editor_facts()),
        }
    }

    /// Finds a declaration in the selected body or checked signature.
    pub fn definition_at(&self, offset: usize) -> Option<&Declaration> {
        self.editor_facts()
            .and_then(|facts| facts.definition_at(offset))
            .or_else(|| self.file.definition_at(offset))
            .or_else(|| {
                let names = &self.file.prepared.names.facts;
                let hit = self
                    .editor_facts()
                    .and_then(|facts| {
                        facts
                            .names
                            .path_hits
                            .iter()
                            .filter(|(span, _)| span.start <= offset && offset < span.end)
                            .min_by_key(|(span, _)| span.end - span.start)
                            .map(|(_, hit)| hit)
                    })
                    .or_else(|| {
                        names
                            .imports
                            .path_hits
                            .iter()
                            .filter(|(span, _)| {
                                span.range.start <= offset && offset < span.range.end
                            })
                            .min_by_key(|(span, _)| span.range.end - span.range.start)
                            .map(|(_, hit)| hit)
                    })?;
                let ResolvedTarget::Source(target) = &hit.target else {
                    return None;
                };
                let file = self.signatures.file(target.unit.file)?;
                if !target
                    .unit
                    .matches(&file.prepared.lowered, file.definitions())
                {
                    return None;
                }
                if let SourceItem::Variant(id) = target.item {
                    file.declarations().variant(id)
                } else {
                    file.declarations().target(target.item.local()?)
                }
            })
    }

    /// Checked type at this position in the retained definition scope.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        let ty = match self.body.as_ref() {
            Some(EditorBody::Function(body)) => body.type_at(offset),
            Some(EditorBody::Initializer(file)) => file.type_at(offset),
            None => None,
        };
        ty.or_else(|| self.file.type_at(offset))
    }

    /// Checked receiver at a complete or recovered member access.
    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        match self.body.as_ref()? {
            EditorBody::Function(body) => body.member_receiver_type(offset),
            EditorBody::Initializer(file) => file.member_receiver_type(offset),
        }
    }

    /// Visible lexical bindings with selected-body types.
    pub fn visible_bindings(&self, offset: usize) -> Vec<BindingInfo> {
        self.editor_facts()
            .map_or_else(Vec::new, |facts| facts.visible_bindings(offset))
    }

    /// Cancellable method candidates from the position owner's receiver facts.
    /// Search exhaustion is an error rather than an incomplete successful list.
    pub fn method_completions(
        &self,
        offset: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<MethodCompletion>, ImplementationSearchError> {
        let Some(facts) = self.editor_facts() else {
            return Ok(Vec::new());
        };
        methods::methods(
            facts,
            self.definitions(),
            self.member_receiver_type(offset),
            offset,
            cancel,
        )
    }

    /// Actual table prefix owning selected body types and bindings.
    pub fn definitions(&self) -> &DefinitionTable {
        match &self.body {
            Some(EditorBody::Function(body)) => body.definitions(),
            Some(EditorBody::Initializer(file)) => file.definitions(),
            None => self.signatures.definitions(),
        }
    }

    /// Bounds of the innermost selected function, for receiver implementation applicability.
    pub fn generic_bounds(&self, offset: usize) -> GenericBounds {
        self.editor_facts()
            .and_then(|facts| {
                facts
                    .typed
                    .signatures
                    .functions()
                    .iter()
                    .filter(|function| {
                        let span = facts.lowered.source_map.function_span(function.id);
                        span.start <= offset && offset <= span.end
                    })
                    .min_by_key(|function| {
                        let span = facts.lowered.source_map.function_span(function.id);
                        span.end - span.start
                    })
                    .map(|function| function.bounds.clone())
            })
            .unwrap_or_default()
    }

    /// Source documentation selected through this exact declaration context.
    pub fn documentation_at(&self, offset: usize) -> Option<DeclarationDocumentation> {
        self.signatures
            .declaration_snapshot()
            .documentation(&self.definition_at(offset)?.id)
    }
}

impl SignatureSnapshot {
    /// Resolves a declaration only within the retained signature environment.
    pub fn declaration(&self, id: &DeclarationId) -> Option<&Declaration> {
        self.declaration_snapshot().declaration(id)
    }

    /// Retained source, including inline and copied generated origins.
    pub fn source(&self, file: FileId) -> Option<&SourceFile> {
        self.declaration_snapshot().source(file)
    }
}

#[cfg(test)]
mod tests;
