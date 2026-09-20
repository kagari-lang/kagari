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

mod body_queries;
pub use body_queries::FunctionAnalysis;
mod declaration_queries;
mod signature_queries;
pub use declaration_queries::{DeclarationSnapshot, FileDeclarations};
pub use signature_queries::{FileSignatures, SignatureSnapshot};

pub use kagari_common::cancellation::{CancellationToken, Cancelled};

#[derive(Debug)]
pub struct FileAnalysis {
    signatures_reused: bool,
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
    pub fn host_field_at(
        &self,
        offset: usize,
    ) -> Option<&kagari_common::host_interface::HostFieldDeclaration> {
        let facts = self.result.facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, _)| {
                let field = facts.typed.type_table.expr_field(id)?;
                let span = facts.lowered.source_map.expr_span(id);
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                Some((span.end - span.start, facts.names.hosts.field(field)?))
            })
            .min_by_key(|(length, _)| *length)
            .map(|(_, field)| field)
    }
    /// Whether this result's signature query reused earlier checked facts.
    /// An unchanged file shares its existing result and this original statistic.
    pub fn signatures_reused(&self) -> bool {
        self.signatures_reused
    }

    pub fn signatures(&self) -> &Arc<AnalysisResult<crate::typeck::ModuleSignatures>> {
        &self.result.facts().signatures
    }

    pub fn source_function_at(&self, offset: usize) -> Option<&crate::imports::ImportedFunction> {
        let facts = self.result.facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, _)| {
                let span = facts.lowered.source_map.expr_span(id);
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                let function = facts
                    .imported_functions
                    .get(facts.names.expr_resolution(id)?)?;
                Some((span.end - span.start, function))
            })
            .min_by_key(|(length, _)| *length)
            .map(|(_, function)| function)
            .or_else(|| {
                facts
                    .names
                    .imports
                    .entries
                    .iter()
                    .enumerate()
                    .find_map(|(index, import)| {
                        (import.span.start <= offset && offset < import.span.end)
                            .then(|| {
                                facts
                                    .imported_functions
                                    .get(crate::resolver::ResolvedName::SourceImport(index))
                            })
                            .flatten()
                    })
            })
    }

    pub fn host_function_at(
        &self,
        offset: usize,
    ) -> Option<&kagari_common::host_interface::HostFunctionDeclaration> {
        let facts = self.result.facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                if let ExprKind::Call { callee, .. } = expr.kind
                    && let Some(call) = facts.typed.type_table.call_resolution(id)
                    && let crate::typeck::CallTarget::HostFunction(host) = call.target
                {
                    let span = facts.lowered.source_map.expr_span(callee);
                    return (span.start <= offset && offset < span.end)
                        .then_some((span.end - span.start, host));
                }
                let span = facts.lowered.source_map.expr_span(id);
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                let crate::resolver::ResolvedName::HostFunction(host) =
                    facts.names.expr_resolution(id)?
                else {
                    return None;
                };
                Some((span.end - span.start, host))
            })
            .min_by_key(|(len, _)| *len)
            .and_then(|(_, host)| facts.names.hosts.function(host))
            .or_else(|| {
                facts
                    .names
                    .imports
                    .entries
                    .iter()
                    .enumerate()
                    .find_map(|(index, import)| {
                        if !(import.span.start <= offset && offset < import.span.end) {
                            return None;
                        }
                        let crate::resolver::ResolvedName::HostFunction(host) = facts
                            .names
                            .imports
                            .resolved_name(crate::resolver::ResolvedName::SourceImport(index))?
                        else {
                            return None;
                        };
                        facts.names.hosts.function(host)
                    })
            })
    }
    /// Portable host documentation has no synthetic source-file location.
    pub fn host_type_at(
        &self,
        offset: usize,
    ) -> Option<&kagari_common::host_interface::HostTypeDeclaration> {
        use crate::{resolver::ResolvedName, typeck::TypeTarget};
        let facts = self.result.facts();
        if let Some((index, _)) = facts
            .lowered
            .source_map
            .type_spans()
            .iter()
            .enumerate()
            .filter(|(_, span)| span.start <= offset && offset < span.end)
            .min_by_key(|(_, span)| span.end - span.start)
        {
            let Some(TypeTarget::Host(id)) = facts
                .typed
                .type_table
                .type_ref(facts.lowered.source_map.type_id(index))?
                .target
            else {
                return None;
            };
            return facts.names.hosts.type_declaration(id);
        }
        if let Some(TypeId::Host(id)) = self.type_at(offset) {
            return facts
                .names
                .hosts
                .nominal_type(&id)
                .and_then(|id| facts.names.hosts.type_declaration(id));
        }
        facts
            .names
            .imports
            .entries
            .iter()
            .enumerate()
            .find_map(|(index, import)| {
                if !(import.span.start <= offset && offset < import.span.end) {
                    return None;
                }
                let ResolvedName::HostType(id) = facts
                    .names
                    .imports
                    .resolved_name(ResolvedName::SourceImport(index))?
                else {
                    return None;
                };
                facts.names.hosts.type_declaration(id)
            })
    }
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
        type_at_in(&facts.lowered, &facts.typed.type_table, offset)
    }

    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        let facts = self.result.facts();
        member_receiver_type_in(&facts.lowered, &facts.typed.type_table, offset)
    }

    pub fn definition_at(&self, offset: usize) -> Option<&crate::declarations::Declaration> {
        let facts = self.result.facts();
        if let Some(member) = facts.declarations.member_at(offset) {
            return Some(member);
        }
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
                        facts.typed.type_table.expr_field(id).and_then(|field| {
                            facts
                                .aggregates
                                .field(field)
                                .map(|field| &field.declaration)
                        })
                    })
                    .or_else(|| {
                        facts
                            .typed
                            .type_table
                            .enum_constructor(id)
                            .filter(|_| {
                                matches!(facts.lowered.module.expr(id).kind, ExprKind::Name(_))
                            })
                            .and_then(|target| target.variant.as_ref())
                            .and_then(|variant| facts.aggregates.variant(variant))
                            .map(|variant| &variant.declaration)
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
                    .place_resolution(facts.lowered.source_map.place_id(index))
                    .and_then(|target| facts.declarations.target(target))
                    .or_else(|| {
                        facts
                            .typed
                            .type_table
                            .place_field(facts.lowered.source_map.place_id(index))
                            .and_then(|field| {
                                facts
                                    .aggregates
                                    .field(field)
                                    .map(|field| &field.declaration)
                            })
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
                    crate::typeck::CallTarget::Function(function) => Some((
                        facts.lowered.source_map.expr_span(*callee),
                        facts
                            .declarations
                            .target(crate::resolver::ResolvedName::Function(function))?,
                    )),
                    crate::typeck::CallTarget::TraitMethod(function) => Some((
                        facts.lowered.source_map.expr_span(*callee),
                        &facts.aggregates.trait_method(&function)?.declaration,
                    )),
                    _ => None,
                }
            });
        // Select the innermost annotation before resolving its target. An unknown
        // type argument must not navigate to the enclosing application declaration.
        if let Some((index, _)) = facts
            .lowered
            .source_map
            .type_spans()
            .iter()
            .enumerate()
            .filter(|(_, span)| span.start <= offset && offset < span.end)
            .min_by_key(|(_, span)| span.end - span.start)
        {
            let target = facts
                .typed
                .type_table
                .type_ref(facts.lowered.source_map.type_id(index))?
                .target?;
            return match target {
                crate::typeck::TypeTarget::Host(_) => None,
                crate::typeck::TypeTarget::Source(id) => facts
                    .declarations
                    .imported_types()
                    .target(id)
                    .map(|ty| &ty.declaration),
                crate::typeck::TypeTarget::Struct(id) => facts
                    .declarations
                    .target(crate::resolver::ResolvedName::Struct(id)),
                crate::typeck::TypeTarget::Enum(id) => facts
                    .declarations
                    .target(crate::resolver::ResolvedName::Enum(id)),
                crate::typeck::TypeTarget::Trait(id) => facts
                    .declarations
                    .target(crate::resolver::ResolvedName::Trait(id)),
                crate::typeck::TypeTarget::Generic(id) => facts.declarations.generic_parameter(id),
            };
        }
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

fn type_at_in(
    lowered: &crate::lower::LoweredModule,
    table: &crate::typeck::TypeTable,
    offset: usize,
) -> Option<TypeId> {
    let expressions = lowered.module.body.expressions().filter_map(|(id, _)| {
        let span = lowered.source_map.expr_span(id);
        (span.start <= offset && offset < span.end)
            .then(|| table.expr_type(id).map(|ty| (span.end - span.start, ty)))
            .flatten()
    });
    let types = lowered
        .source_map
        .type_spans()
        .iter()
        .enumerate()
        .filter_map(|(index, span)| {
            if !(span.start <= offset && offset < span.end) {
                return None;
            }
            table
                .type_ref(lowered.source_map.type_id(index))
                .map(|resolved| (span.end - span.start, resolved.ty.clone()))
        });
    expressions
        .chain(types)
        .min_by_key(|(len, _)| *len)
        .map(|(_, ty)| ty)
}

fn member_receiver_type_in(
    lowered: &crate::lower::LoweredModule,
    table: &crate::typeck::TypeTable,
    offset: usize,
) -> Option<TypeId> {
    lowered
        .module
        .body
        .expressions()
        .filter_map(|(id, expr)| {
            let ExprKind::Field { receiver, .. } = &expr.kind else {
                return None;
            };
            let span = lowered.source_map.expr_span(id);
            if span.start <= offset && offset <= span.end {
                table
                    .expr_type(*receiver)
                    .map(|ty| (span.end - span.start, ty))
            } else {
                None
            }
        })
        .min_by_key(|(len, _)| *len)
        .map(|(_, ty)| ty)
}

#[derive(Debug)]
pub struct AnalysisDatabase {
    body_cache: HashMap<kagari_common::identity::DefinitionId, Arc<FunctionAnalysis>>,
    body_revision: Revision,
    declaration_cache: Option<DeclarationSnapshot>,
    signature_cache: Option<SignatureSnapshot>,
    files: HashMap<FileId, Arc<FileAnalysis>>,
    latest_revision: Revision,
    hosts: Arc<crate::host::HostDeclarations>,
}

impl Default for AnalysisDatabase {
    fn default() -> Self {
        Self {
            body_cache: HashMap::new(),
            body_revision: Revision::default(),
            declaration_cache: None,
            signature_cache: None,
            files: HashMap::new(),
            latest_revision: Revision::default(),
            hosts: crate::host::HostDeclarations::empty(),
        }
    }
}

impl AnalysisDatabase {
    pub fn set_host_declarations(&mut self, hosts: Arc<crate::host::HostDeclarations>) {
        self.hosts = hosts;
    }
    pub fn snapshot(
        &mut self,
        source: SourceSnapshot,
        profile: LanguageFeatureProfile,
        cancel: &CancellationToken,
    ) -> Result<AnalysisSnapshot, Cancelled> {
        cancel.check()?;
        let signature_snapshot = self.prepare_signatures(source.clone(), cancel)?;
        let graph = signature_snapshot.declarations.graph.clone();
        let signatures = signature_snapshot
            .files
            .iter()
            .map(|(id, file)| {
                (
                    *id,
                    (
                        file.prepared.lowered.source.clone(),
                        file.declaration.parsed.clone(),
                        file.prepared.clone(),
                    ),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut environments = signature_snapshot.body_environments(cancel)?;
        let mut files = HashMap::new();
        for (id, (file, parsed, prepared)) in signatures {
            cancel.check()?;
            let signature_queries::BodyEnvironment {
                imported_functions,
                aggregates,
            } = environments.remove(&id).expect("prepared body environment");
            let imports = prepared.names.facts.imports.clone();
            let analysis = match self.files.get(&id) {
                Some(previous)
                    if previous.source.revision() == file.revision()
                        && previous.result.facts().lowered.module.body.arena()
                            == prepared.lowered.module.body.arena()
                        && previous.profile == profile
                        && previous.result.facts().names.hosts.revision()
                            == self.hosts.revision()
                        && previous.result.facts().names.imports == imports
                        && previous.result.facts().imported_functions == imported_functions
                        && previous.result.facts().aggregates == aggregates
                        && previous.result.facts().declarations.imported_types
                            == prepared.declarations.imported_types =>
                {
                    previous.clone()
                }
                _ => {
                    let signatures_reused = prepared.signatures_reused;
                    let reuse = self
                        .files
                        .get(&id)
                        .filter(|old| {
                            old.result.diagnostics().is_empty()
                                && old.result.facts().names.hosts.revision()
                                    == self.hosts.revision()
                                && old.result.facts().names.imports.same_bindings(&imports)
                                && old.result.facts().imported_functions == imported_functions
                                && old.result.facts().aggregates.same_contracts(&aggregates)
                                && old.result.facts().declarations.imported_types
                                    == prepared.declarations.imported_types
                                && old.source.module_identity() == file.module_identity()
                        })
                        .map(|old| crate::typeck::BodyReuse {
                            previous_lowered: &old.result.facts().lowered,
                            previous_types: &old.result.facts().typed.type_table,
                            old_text: old.source.text(),
                            new_text: file.text(),
                        });
                    let result = analyze_parsed(
                        prepared,
                        &parsed,
                        profile,
                        imported_functions,
                        aggregates,
                        reuse.as_ref(),
                        cancel,
                    );
                    Arc::new(FileAnalysis {
                        signatures_reused,
                        source: file,
                        profile,
                        parsed,
                        result,
                    })
                }
            };
            files.insert(id, analysis);
        }
        cancel.check()?;
        self.publish_signatures(signature_snapshot.clone());
        if source.revision() >= self.latest_revision {
            self.latest_revision = source.revision();
            self.files = files.clone();
        }
        Ok(AnalysisSnapshot {
            signatures: signature_snapshot,
            revision: source.revision(),
            host_revision: self.hosts.revision(),
            graph,
            files: Arc::new(files),
        })
    }
}

#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    signatures: SignatureSnapshot,
    revision: Revision,
    host_revision: u64,
    graph: Arc<crate::imports::ModuleGraph>,
    files: Arc<HashMap<FileId, Arc<FileAnalysis>>>,
}

impl AnalysisSnapshot {
    /// The declaration query consumed by this complete analysis.
    pub fn declaration_snapshot(&self) -> &DeclarationSnapshot {
        self.signatures.declaration_snapshot()
    }

    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    pub fn source_import_at(
        &self,
        file: FileId,
        offset: usize,
    ) -> Option<crate::imports::SourceImport> {
        use crate::imports::ImportTarget;
        let facts = self.file(file)?.result.facts();
        let expression = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, _)| {
                let span = facts.lowered.source_map.expr_span(id);
                if !(span.start <= offset && offset < span.end) {
                    return None;
                }
                let binding = facts.names.expr_resolution(id)?;
                let ImportTarget::Source(target) = facts.names.imports.binding(binding)? else {
                    return None;
                };
                Some((span.end - span.start, target.clone()))
            })
            .min_by_key(|(length, _)| *length)
            .map(|(_, target)| target);
        expression.or_else(|| {
            facts.names.imports.entries.iter().find_map(|import| {
                if !(import.span.start <= offset && offset < import.span.end) {
                    return None;
                }
                match &import.target {
                    Some(ImportTarget::Source(target)) => Some(target.clone()),
                    _ => None,
                }
            })
        })
    }

    pub fn definition_at(
        &self,
        file: FileId,
        offset: usize,
    ) -> Option<&crate::declarations::Declaration> {
        if let Some(declaration) = self.file(file)?.definition_at(offset) {
            return Some(declaration);
        }
        use crate::{hir::ExportItem, resolver::ResolvedName};
        let crate::imports::ImportTarget::Source(target) = self
            .graph
            .resolve_export(self.source_import_at(file, offset)?, &Default::default())
            .ok()??
        else {
            return None;
        };
        let file = self.file(target.file)?;
        let resolved = match target.item? {
            ExportItem::Function(id) => ResolvedName::Function(id),
            ExportItem::Const(id) => ResolvedName::Const(id),
            ExportItem::Module(id) => ResolvedName::Module(id),
            ExportItem::Struct(id) => ResolvedName::Struct(id),
            ExportItem::Enum(id) => ResolvedName::Enum(id),
            ExportItem::Trait(id) => ResolvedName::Trait(id),
            ExportItem::Import(_) => return None,
        };
        file.result.facts().declarations.target(resolved)
    }
    pub fn module_graph(&self) -> &crate::imports::ModuleGraph {
        &self.graph
    }
    pub fn host_revision(&self) -> u64 {
        self.host_revision
    }
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
mod arena_tests;
#[cfg(test)]
mod constructor_tests;
#[cfg(test)]
mod generic_type_tests;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod member_tests;
#[cfg(test)]
mod namespace_tests;
#[cfg(test)]
mod owner_tests;
#[cfg(test)]
mod payload_tests;
#[cfg(test)]
mod prelude_tests;
#[cfg(test)]
mod signature_tests;
#[cfg(test)]
mod trait_catalog_tests;
#[cfg(test)]
mod trait_identity_tests;
#[cfg(test)]
mod trait_reference_tests;
#[cfg(test)]
mod type_application_tests;
#[cfg(test)]
mod type_name_tests;

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
            Some(TypeId::Struct(crate::types::NominalType {
                declaration: facts
                    .result()
                    .facts()
                    .declarations
                    .definition(crate::resolver::ResolvedName::Struct(
                        crate::hir::StructId::new(0)
                    ))
                    .unwrap()
                    .clone(),
                arguments: Vec::new(),
            }))
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
