//! Protocol-independent immutable source analysis. Queries never execute code.
use std::{collections::HashMap, sync::Arc};

use kagari_common::{
    SourceFile,
    identity::{FileId, Revision},
    source_database::SourceSnapshot,
};

use crate::{
    AnalysisResult, AnalyzedModule, LanguageFeatureProfile, analyze_parsed, hir::ExprKind,
    types::TypeId,
};

pub use kagari_common::cancellation::{CancellationToken, Cancelled};

#[derive(Debug)]
pub struct FileAnalysis {
    source: Arc<SourceFile>,
    profile: LanguageFeatureProfile,
    parsed: kagari_syntax::Parse,
    result: AnalysisResult<AnalyzedModule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInfo {
    pub ty: TypeId,
    pub declaration: crate::declarations::Declaration,
}

impl FileAnalysis {
    pub fn syntax(&self) -> kagari_syntax::ast::SourceFile {
        self.parsed.syntax()
    }
    pub fn source(&self) -> &SourceFile {
        &self.source
    }
    pub fn result(&self) -> &AnalysisResult<AnalyzedModule> {
        &self.result
    }

    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        let facts = self.result.facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, _)| {
                let span = facts.lowered.source_map.expr_span(id);
                (span.start <= offset && offset < span.end)
                    .then(|| {
                        facts
                            .typed
                            .type_table
                            .expr_type(id)
                            .map(|ty| (span.end - span.start, ty))
                    })
                    .flatten()
            })
            .min_by_key(|(len, _)| *len)
            .map(|(_, ty)| ty)
    }

    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        let facts = self.result.facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let ExprKind::Field { receiver, .. } = &expr.kind else {
                    return None;
                };
                let span = facts.lowered.source_map.expr_span(id);
                if span.start <= offset && offset <= span.end {
                    facts
                        .typed
                        .type_table
                        .expr_type(*receiver)
                        .map(|ty| (span.end - span.start, ty))
                } else {
                    None
                }
            })
            .min_by_key(|(len, _)| *len)
            .map(|(_, ty)| ty)
    }

    pub fn definition_at(&self, offset: usize) -> Option<&crate::declarations::Declaration> {
        let facts = self.result.facts();
        let expressions = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, _)| {
                let span = facts.lowered.source_map.expr_span(id);
                let target = facts
                    .names
                    .expr_resolution(id)
                    .and_then(|target| facts.declarations.target(target))
                    .or_else(|| {
                        facts
                            .typed
                            .type_table
                            .expr_field(id)
                            .and_then(|field| facts.declarations.field(field))
                    })?;
                Some((span, target))
            });
        let places = facts
            .lowered
            .source_map
            .place_spans()
            .iter()
            .enumerate()
            .filter_map(|(index, span)| {
                let target = facts
                    .names
                    .place_resolution(crate::hir::PlaceId::new(index))
                    .and_then(|target| facts.declarations.target(target))
                    .or_else(|| {
                        facts
                            .typed
                            .type_table
                            .place_field(crate::hir::PlaceId::new(index))
                            .and_then(|field| facts.declarations.field(field))
                    })?;
                Some((*span, target))
            });
        let calls = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                let ExprKind::Call { callee, .. } = &expr.kind else {
                    return None;
                };
                let call = facts.typed.type_table.call_resolution(id)?;
                match call.target {
                    crate::typeck::CallTarget::Function(function)
                    | crate::typeck::CallTarget::TraitMethod(function) => Some((
                        facts.lowered.source_map.expr_span(*callee),
                        facts
                            .declarations
                            .target(crate::resolver::ResolvedName::Function(function))?,
                    )),
                    _ => None,
                }
            });
        expressions
            .chain(places)
            .chain(calls)
            .filter(|(span, _)| span.start <= offset && offset < span.end)
            .min_by_key(|(span, _)| span.end - span.start)
            .map(|(_, target)| target)
    }

    pub fn visible_bindings(&self, offset: usize) -> Vec<BindingInfo> {
        let facts = self.result.facts();
        facts
            .names
            .visible_bindings(offset)
            .into_iter()
            .filter_map(|binding| {
                let declaration = facts.declarations.target(binding.resolved)?.clone();
                let ty = match binding.resolved {
                    crate::resolver::ResolvedName::Local(id) => {
                        facts.typed.type_table.local_type(id)
                    }
                    crate::resolver::ResolvedName::Param(id) => facts
                        .typed
                        .functions
                        .iter()
                        .flat_map(|function| &function.params)
                        .find(|param| param.id == id)
                        .map(|param| param.ty.clone()),
                    _ => None,
                }
                .unwrap_or(TypeId::Unknown);
                Some(BindingInfo { declaration, ty })
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct AnalysisDatabase {
    files: HashMap<FileId, Arc<FileAnalysis>>,
    latest_revision: Revision,
}

impl AnalysisDatabase {
    pub fn snapshot(
        &mut self,
        source: SourceSnapshot,
        profile: LanguageFeatureProfile,
        cancel: &CancellationToken,
    ) -> Result<AnalysisSnapshot, Cancelled> {
        cancel.check()?;
        let mut files = HashMap::new();
        for file in source.files() {
            cancel.check()?;
            let analysis = match self.files.get(&file.id()) {
                Some(previous)
                    if previous.source.revision() == file.revision()
                        && previous.profile == profile =>
                {
                    previous.clone()
                }
                _ => {
                    let parsed = self
                        .files
                        .get(&file.id())
                        .filter(|old| old.source.revision() == file.revision())
                        .map(|old| Ok(old.parsed.clone()))
                        .unwrap_or_else(|| {
                            kagari_syntax::parser::parse_with_cancellation(file, cancel)
                        })?;
                    cancel.check()?;
                    let reuse = self
                        .files
                        .get(&file.id())
                        .filter(|old| {
                            old.result.diagnostics().is_empty()
                                && old.source.module_identity() == file.module_identity()
                        })
                        .map(|old| crate::typeck::BodyReuse {
                            previous: old.result.facts(),
                            old_text: old.source.text(),
                            new_text: file.text(),
                        });
                    let result =
                        analyze_parsed(file.clone(), &parsed, profile, reuse.as_ref(), cancel);
                    Arc::new(FileAnalysis {
                        source: file.clone(),
                        profile,
                        parsed,
                        result,
                    })
                }
            };
            files.insert(file.id(), analysis);
        }
        cancel.check()?;
        if source.revision() >= self.latest_revision {
            self.latest_revision = source.revision();
            self.files = files.clone();
        }
        Ok(AnalysisSnapshot {
            revision: source.revision(),
            files: Arc::new(files),
        })
    }
}

#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    revision: Revision,
    files: Arc<HashMap<FileId, Arc<FileAnalysis>>>,
}

impl AnalysisSnapshot {
    pub fn revision(&self) -> Revision {
        self.revision
    }
    pub fn file(&self, id: FileId) -> Option<&Arc<FileAnalysis>> {
        self.files.get(&id)
    }

    pub fn declaration(
        &self,
        id: &crate::declarations::DeclarationId,
    ) -> Option<&crate::declarations::Declaration> {
        self.files
            .values()
            .find_map(|file| file.result.facts().declarations.get(id))
    }
}

#[cfg(test)]
mod identity_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BuiltinType;
    use kagari_common::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn body_edits_reuse_other_bodies_but_signatures_invalidate_them() {
        let mut sources = SourceDatabase::default();
        let file = sources
            .set(
                "body.kgr",
                "fn a() -> i32 { 1 } fn b() -> i32 { a() }".into(),
                SourceLayer::Base,
            )
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let token = CancellationToken::default();
        let first = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile::default(),
                &token,
            )
            .unwrap();
        assert_eq!(
            first
                .file(file)
                .unwrap()
                .result
                .facts()
                .typed
                .checked_bodies,
            2
        );
        sources
            .set(
                "body.kgr",
                "fn a() -> i32 { 100 + 2 } fn b() -> i32 { a() }".into(),
                SourceLayer::Overlay,
            )
            .unwrap();
        let second = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile::default(),
                &token,
            )
            .unwrap();
        let result = &second.file(file).unwrap().result;
        assert_eq!(result.facts().typed.reused_bodies, 1);
        assert_eq!(result.facts().typed.checked_bodies, 1);
        assert!(result.clone().into_codegen().is_ok());
        sources
            .set(
                "body.kgr",
                "fn a() -> bool { true } fn b() -> i32 { a() }".into(),
                SourceLayer::Overlay,
            )
            .unwrap();
        let third = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile::default(),
                &token,
            )
            .unwrap();
        let result = &third.file(file).unwrap().result;
        assert_eq!(result.facts().typed.reused_bodies, 0);
        assert!(!result.diagnostics().is_empty());
    }

    #[test]
    fn broken_body_preserves_neighbor_and_member_receiver() {
        let mut sources = SourceDatabase::default();
        let text = "struct P { var n: i32 } fn bad() { val p = P { n: 1 }; p. } fn good() -> i32 { val answer = 42; answer }";
        let file = sources
            .set("a.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        let facts = snapshot.file(file).unwrap();
        assert!(!facts.result().diagnostics().is_empty());
        assert_eq!(
            facts.member_receiver_type(text.find("p. }").unwrap() + 2),
            Some(TypeId::Struct("P".into()))
        );
        let offset = text.rfind("answer").unwrap();
        assert_eq!(
            facts.type_at(offset),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert_eq!(
            facts
                .visible_bindings(offset)
                .iter()
                .map(|b| b.declaration.name.as_str())
                .collect::<Vec<_>>(),
            vec!["answer"]
        );
        assert!(facts.result.clone().into_codegen().is_err());
    }

    #[test]
    fn snapshots_reuse_unchanged_files_and_cancellation_does_not_publish() {
        let mut sources = SourceDatabase::default();
        let a = sources
            .set("a.kgr", "fn a() -> i32 { 1 }".into(), SourceLayer::Base)
            .unwrap();
        let b = sources
            .set("b.kgr", "fn b() -> i32 { 2 }".into(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let first = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        sources
            .set("b.kgr", "fn b() -> i32 { 3 }".into(), SourceLayer::Overlay)
            .unwrap();
        let second = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        assert!(Arc::ptr_eq(first.file(a).unwrap(), second.file(a).unwrap()));
        assert!(!Arc::ptr_eq(
            first.file(b).unwrap(),
            second.file(b).unwrap()
        ));
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(
            db.snapshot(
                sources.snapshot(),
                LanguageFeatureProfile::default(),
                &cancel
            )
            .is_err()
        );
        assert!(first.file(b).unwrap().source().text().contains("{ 2 }"));
    }
}
