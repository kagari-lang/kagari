//! Physical cursor routing over a retained semantic snapshot.
use crate::{
    file::{FileQueries, type_reference_target_at},
    navigation::NavigationIndex,
    sites::SourceSites,
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::{
    analysis::{AnalysisSnapshot, FileAnalysis},
    declarations::{Declaration, DeclarationId},
    imports::{ResolvedTarget, SourceItem},
    typeck::table::ResolvedTypeTarget,
};
use kagari_source::identity::FileId;
use std::{cell::RefCell, collections::HashMap, sync::Arc};

/// Editor queries and query-local indexes tied to one exact snapshot.
#[derive(Debug)]
pub struct SnapshotQueries<'a> {
    pub(crate) analysis: &'a AnalysisSnapshot,
    pub(crate) navigation: RefCell<HashMap<FileId, NavigationIndex<'a>>>,
}

impl<'a> SnapshotQueries<'a> {
    /// Borrows an immutable semantic snapshot without preparing navigation.
    pub fn new(analysis: &'a AnalysisSnapshot) -> Self {
        Self {
            analysis,
            navigation: RefCell::new(HashMap::new()),
        }
    }

    /// Select the innermost inline module at an editor position in a physical file.
    pub fn analysis_at(&self, file: FileId, offset: usize) -> Option<&'a Arc<FileAnalysis>> {
        let physical = self.analysis.file(file)?;
        self.analysis
            .files()
            .filter(|candidate| {
                let source = candidate.source();
                source.origin_id() == file
                    && source
                        .inline_range()
                        .is_some_and(|range| range.start <= offset && offset < range.end)
            })
            .min_by_key(|candidate| {
                let range = candidate.source().inline_range().expect("inline candidate");
                range.end - range.start
            })
            .or(Some(physical))
    }

    /// Finds a declaration at a physical source position, including inline-module routing.
    pub fn definition_at(&self, file: FileId, offset: usize) -> Option<&'a Declaration> {
        let analysis = self.analysis_at(file, offset)?;
        if let Some(declaration) = FileQueries::new(analysis).definition_at(offset) {
            return Some(declaration);
        }
        let facts = analysis.result().facts();
        let sites = SourceSites::build(
            &facts.lowered,
            &analysis.syntax(),
            &CancellationToken::default(),
        )
        .expect("uncancelled source sites");
        if let Some(Some(ResolvedTypeTarget::AssociatedType(member))) =
            type_reference_target_at(&facts.lowered, &sites, facts.typed.type_table(), offset)
        {
            return self
                .analysis
                .declaration(&DeclarationId::Definition(member));
        }
        let target = self.source_target_at(file, offset)?.target;
        let ResolvedTarget::Source(target) = target else {
            return None;
        };
        let file = self.analysis.file(target.unit.file)?;
        if !target.unit.matches(
            &file.result().facts().lowered,
            file.result().facts().names.catalog.definitions(),
        ) {
            return None;
        }
        if let SourceItem::Variant(id) = target.item {
            return file.result().facts().declarations.variant(id);
        }
        file.result()
            .facts()
            .declarations
            .target(target.item.local()?)
    }
}
