//! A function query resolves and checks just that body and its module constants.
use super::signature_queries::BodyEnvironment;
use super::*;
use crate::{
    hir::{BodySelection, FunctionId, FunctionKind},
    resolver::ResolvedName,
};

#[cfg(test)]
mod tests;
use kagari_common::identity::DefinitionId;

#[derive(Debug)]
pub struct FunctionAnalysis {
    owner: DefinitionId,
    function: FunctionId,
    signatures: SignatureSnapshot,
    file: Arc<FileSignatures>,
    environment: BodyEnvironment,
    names: crate::resolver::ResolvedNames,
    declarations: crate::declarations::Declarations,
    typed: AnalysisResult<crate::typeck::TypedModule>,
}

impl FunctionAnalysis {
    fn contains(&self, offset: usize) -> bool {
        self.names.scopes().iter().any(|scope| {
            scope.owner == crate::hir::BodyOwner::Function(self.function)
                && scope.span.start <= offset
                && offset <= scope.span.end
                && !scope
                    .excluded_ranges
                    .iter()
                    .any(|span| span.start <= offset && offset < span.end)
        })
    }
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        self.contains(offset)
            .then(|| super::type_at_in(self.lowered(), self.type_table(), offset))
            .flatten()
    }
    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        self.contains(offset)
            .then(|| super::member_receiver_type_in(self.lowered(), self.type_table(), offset))
            .flatten()
    }
    pub fn owner(&self) -> &DefinitionId {
        &self.owner
    }
    pub fn source(&self) -> &SourceFile {
        self.file.source()
    }
    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }
    /// Local IDs are interpreted only against this result's lowering and facts.
    pub fn lowered(&self) -> &crate::lower::LoweredModule {
        &self.file.prepared.lowered
    }
    pub fn function(&self) -> FunctionId {
        self.function
    }
    pub fn names(&self) -> &crate::resolver::ResolvedNames {
        &self.names
    }
    pub fn declarations(&self) -> &crate::declarations::Declarations {
        &self.declarations
    }
    pub fn type_table(&self) -> &crate::typeck::TypeTable {
        &self.typed.facts.type_table
    }
    /// Body diagnostics and module-constant prerequisites. Header diagnostics are
    /// available separately through the retained signature snapshot.
    pub fn diagnostics(&self) -> &[kagari_common::Diagnostic] {
        self.typed.diagnostics()
    }
    pub fn checked_bodies(&self) -> usize {
        self.typed.facts.checked_bodies
    }
    pub fn reused_bodies(&self) -> usize {
        self.typed.facts.reused_bodies
    }
}

impl AnalysisDatabase {
    pub fn body(
        &mut self,
        source: SourceSnapshot,
        owner: &DefinitionId,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<FunctionAnalysis>>, Cancelled> {
        let signatures = self.prepare_signatures(source, cancel)?;
        let target = signatures.files.values().find_map(|file| {
            let ResolvedName::Function(function) = file.declarations().definition_target(owner)?
            else {
                return None;
            };
            file.prepared
                .lowered
                .module
                .functions
                .iter()
                .find(|f| f.id == function && f.kind != FunctionKind::TraitMethod)?;
            Some((file.clone(), function))
        });
        let Some((file, function)) = target else {
            cancel.check()?;
            self.publish_body(signatures.declaration_snapshot(), None);
            self.publish_signatures(signatures);
            return Ok(None);
        };
        let environment = signatures
            .body_environments(cancel)?
            .remove(&file.source().id())
            .expect("function environment");
        let previous = self.body_cache.get(owner);
        let result = if let Some(old) =
            previous.filter(|old| Arc::ptr_eq(&old.file, &file) && old.environment == environment)
        {
            old.clone()
        } else {
            let prepared = &file.prepared;
            let selection = BodySelection::Function(function);
            let names = crate::resolver::resolve_bodies(
                &prepared.lowered,
                &prepared.names.facts,
                selection,
                cancel,
            );
            let declarations =
                prepared
                    .declarations
                    .clone()
                    .with_bindings(&prepared.lowered, &names, cancel);
            let reuse = previous
                .filter(|old| {
                    old.diagnostics().is_empty()
                        && old.file.diagnostics().is_empty()
                        && file.diagnostics().is_empty()
                        && old.file.prepared.names.facts.hosts.revision()
                            == prepared.names.facts.hosts.revision()
                        && old
                            .file
                            .prepared
                            .names
                            .facts
                            .imports
                            .same_bindings(&prepared.names.facts.imports)
                        && old.file.prepared.declarations.imported_types
                            == prepared.declarations.imported_types
                        && old.environment.imported_functions == environment.imported_functions
                        && old
                            .environment
                            .aggregates
                            .same_contracts(&environment.aggregates)
                })
                .map(|old| crate::typeck::BodyReuse {
                    previous_diagnostics: old.diagnostics(),
                    previous_lowered: old.lowered(),
                    previous_types: old.type_table(),
                    old_text: old.source().text(),
                    new_text: file.source().text(),
                });
            let typed = crate::typeck::check_bodies_controlled(
                &prepared.lowered,
                &names,
                &declarations,
                crate::typeck::BodyInputs {
                    selection,
                    signatures: &prepared.signatures,
                    imported_functions: &environment.imported_functions,
                    aggregates: &environment.aggregates,
                },
                reuse.as_ref(),
                cancel,
            );
            Arc::new(FunctionAnalysis {
                owner: owner.clone(),
                function,
                signatures: signatures.clone(),
                file,
                environment,
                names,
                declarations,
                typed,
            })
        };
        cancel.check()?;
        self.publish_body(signatures.declaration_snapshot(), Some(result.clone()));
        self.publish_signatures(signatures);
        Ok(Some(result))
    }

    pub(super) fn publish_body(
        &mut self,
        declarations: &DeclarationSnapshot,
        result: Option<Arc<FunctionAnalysis>>,
    ) {
        if declarations.revision() >= self.body_revision {
            self.body_revision = declarations.revision();
            self.body_cache.retain(|id, _| {
                declarations
                    .declaration(&crate::declarations::DeclarationId::Definition(id.clone()))
                    .is_some()
            });
            if let Some(result) = result {
                self.body_cache.insert(result.owner.clone(), result);
            }
        }
    }
}
