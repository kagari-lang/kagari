use crate::{
    navigation::{NavigationIndex, NavigationTarget},
    snapshot::SnapshotQueries,
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::imports::{BindingOrigin, ImportKind};
use kagari_source::identity::FileId;

impl SnapshotQueries<'_> {
    /// Returns the unique selected target; dual-category import sites have no single answer.
    pub fn source_target_at(&self, file: FileId, offset: usize) -> Option<NavigationTarget> {
        let mut hits = self.source_targets_at(file, offset).into_iter();
        let hit = hits.next()?;
        hits.next().is_none().then_some(hit)
    }

    /// Returns every selected category at the most specific physical source site.
    /// A named import can select both Type and Value, preserving each target's origins.
    pub fn source_targets_at(&self, file: FileId, offset: usize) -> Vec<NavigationTarget> {
        let Some(analysis) = self.analysis_at(file, offset) else {
            return vec![];
        };
        let facts = analysis.result().facts();
        let mut indexes = self.navigation.borrow_mut();
        let navigation = indexes.entry(analysis.source().id()).or_insert_with(|| {
            NavigationIndex::build(analysis, &CancellationToken::default())
                .expect("uncancelled navigation")
        });
        let sites = navigation
            .hits()
            .iter()
            .map(|(span, hit)| (*span, hit))
            .filter(|(span, _)| span.start <= offset && offset < span.end)
            .collect::<Vec<_>>();
        if let Some(length) = sites.iter().map(|(span, _)| span.end - span.start).min() {
            let mut hits: Vec<NavigationTarget> = Vec::new();
            for (span, hit) in sites {
                if span.end - span.start != length {
                    continue;
                }
                if let Some(previous) = hits.iter_mut().find(|previous| {
                    previous.namespace == hit.namespace && previous.target == hit.target
                }) {
                    for origin in &hit.via {
                        if !previous.via.contains(origin) {
                            previous.via.push(origin.clone());
                        }
                    }
                } else {
                    hits.push(NavigationTarget::from(hit));
                }
            }
            return hits;
        }

        let Some(directive) = facts
            .names
            .imports
            .directives
            .iter()
            .filter_map(|directive| {
                let span = directive.span?.range;
                (span.start <= offset && offset < span.end).then_some((directive, span))
            })
            .min_by_key(|(_, span)| span.end - span.start)
            .map(|(directive, _)| directive)
        else {
            return vec![];
        };
        directive
            .resolution
            .targets()
            .map(|target| NavigationTarget {
                namespace: target.namespace(),
                target: *target,
                via: vec![match directive.kind {
                    ImportKind::Named { .. } => BindingOrigin::NamedImport(directive.id),
                    ImportKind::Glob => BindingOrigin::GlobImport(directive.id),
                }],
            })
            .collect()
    }
}
