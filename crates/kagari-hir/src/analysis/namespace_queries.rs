//! Category-aware source navigation and module binding projection.
use crate::{
    analysis::AnalysisSnapshot,
    imports::{BindingOrigin, ImportKind, catalog::LookupHit},
};
use kagari_source::identity::FileId;

impl AnalysisSnapshot {
    /// Returns the unique selected target; dual-category import sites have no single answer.
    pub fn source_target_at(&self, file: FileId, offset: usize) -> Option<LookupHit> {
        let mut hits = self.source_targets_at(file, offset).into_iter();
        let hit = hits.next()?;
        hits.next().is_none().then_some(hit)
    }

    /// Returns every selected category at the most specific physical source site.
    /// A named import can select both Type and Value, preserving each target's origins.
    pub fn source_targets_at(&self, file: FileId, offset: usize) -> Vec<LookupHit> {
        let Some(analysis) = self.analysis_at(file, offset) else {
            return vec![];
        };
        let facts = analysis.result.records().facts();
        let sites = analysis
            .type_hits
            .iter()
            .map(|(span, hit)| (*span, hit))
            .chain(facts.names.path_hits.iter().map(|(span, hit)| (*span, hit)))
            .chain(
                facts
                    .names
                    .imports
                    .path_hits
                    .iter()
                    .map(|(span, hit)| (span.range, hit)),
            )
            .filter(|(span, _)| span.start <= offset && offset < span.end)
            .collect::<Vec<_>>();
        if let Some(length) = sites.iter().map(|(span, _)| span.end - span.start).min() {
            let mut hits: Vec<LookupHit> = Vec::new();
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
                    hits.push(hit.clone());
                }
            }
            return hits;
        }
        if let Some((_, hit)) = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, _)| {
                let span = facts
                    .lowered
                    .source_map
                    .expr_reference_span(id)
                    .unwrap_or_else(|| facts.lowered.source_map.expr_span(id));
                (span.start <= offset && offset < span.end)
                    .then(|| {
                        facts
                            .names
                            .lookup_hit(id)
                            .map(|hit| (span.end - span.start, hit))
                    })
                    .flatten()
            })
            .min_by_key(|(length, _)| *length)
        {
            return vec![hit.clone()];
        }
        let Some(directive) = facts
            .names
            .imports
            .directives
            .iter()
            .filter(|directive| {
                directive.span.range.start <= offset && offset < directive.span.range.end
            })
            .min_by_key(|directive| directive.span.range.end - directive.span.range.start)
        else {
            return vec![];
        };
        directive
            .resolution
            .targets()
            .map(|target| LookupHit {
                namespace: target.namespace(),
                target: target.clone(),
                via: vec![match directive.kind {
                    ImportKind::Named { .. } => BindingOrigin::NamedImport(directive.id.clone()),
                    ImportKind::Glob => BindingOrigin::GlobImport(directive.id.clone()),
                }],
            })
            .collect()
    }
}
