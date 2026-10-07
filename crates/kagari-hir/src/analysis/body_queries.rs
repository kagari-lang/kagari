//! A function query resolves and checks just that body and its module constants.

use super::signature_queries::BodyEnvironment;
use crate::{
    AnalysisResult,
    analysis::{
        AnalysisDatabase,
        declaration_queries::DeclarationSnapshot,
        error::AnalysisError,
        ownership,
        signature_queries::{FileSignatures, SignatureSnapshot},
    },
    declarations::{DeclarationId, Declarations},
    hir::{
        ids::{BodyOwner, BodySelection, FunctionId},
        item::function::FunctionKind,
    },
    lower::LoweredModule,
    resolver::{
        collect::resolve_bodies,
        resolved::{ResolvedName, ResolvedNames},
    },
    typeck::{
        BodyInputs, TypedModule, check::check_bodies_controlled, reuse::BodyReuse, table::TypeTable,
    },
    types::TypeId,
};

use std::sync::Arc;
use {
    kagari_common::{
        cancellation::CancellationToken,
        identity::{
            mapping::DefinitionMappingError,
            reference::DefinitionReference,
            table::{DefinitionId, DefinitionTable, DefinitionTableError},
        },
    },
    kagari_source::{diagnostic::Diagnostic, source::SourceFile, source_database::SourceSnapshot},
};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct FunctionAnalysis {
    owner: DefinitionId,
    definitions: DefinitionTable,
    function: FunctionId,
    signatures: SignatureSnapshot,
    file: Arc<FileSignatures>,
    environment: BodyEnvironment<DefinitionId>,
    names: ResolvedNames,
    declarations: Declarations<DefinitionId>,
    typed: AnalysisResult<TypedModule<DefinitionId>>,
}

impl FunctionAnalysis {
    fn contains(&self, offset: usize) -> bool {
        self.names.scopes().iter().any(|scope| {
            scope.owner == BodyOwner::Function(self.function)
                && scope.span.start <= offset
                && offset <= scope.span.end
        })
    }

    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        let ty = self
            .contains(offset)
            .then(|| super::type_at_in(self.lowered(), self.type_table(), offset))
            .flatten()?;
        ownership::paths(&ty, &self.definitions, &CancellationToken::default()).ok()
    }

    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        let ty = self
            .contains(offset)
            .then(|| super::member_receiver_type_in(self.lowered(), self.type_table(), offset))
            .flatten()?;
        ownership::paths(&ty, &self.definitions, &CancellationToken::default()).ok()
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
    pub fn lowered(&self) -> &LoweredModule {
        &self.file.prepared.lowered
    }

    pub fn function(&self) -> FunctionId {
        self.function
    }

    pub fn names(&self) -> &ResolvedNames {
        &self.names
    }

    pub fn declarations(&self) -> &Declarations<DefinitionId> {
        &self.declarations
    }

    pub fn type_table(&self) -> &TypeTable<DefinitionId> {
        &self.typed.facts.type_table
    }

    /// Body diagnostics and module-constant prerequisites. Header diagnostics are
    /// available separately through the retained signature snapshot.
    pub fn diagnostics(&self) -> &[Diagnostic] {
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
    pub fn body<I: DefinitionReference>(
        &mut self,
        source: SourceSnapshot,
        owner: &I,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<FunctionAnalysis>>, AnalysisError> {
        let signatures = self.prepare_signatures(source, cancel)?;
        let definitions = self.definitions.snapshot();
        let owner_id = match owner.resolve(&definitions) {
            Ok(id) => id,
            Err(DefinitionTableError::UnmappedDefinition) => {
                self.publish_signatures(signatures);
                return Ok(None);
            }
            Err(error) => return Err(DefinitionMappingError::from(error).into()),
        };
        let owner = definitions
            .resolve(owner_id)
            .map_err(DefinitionMappingError::from)?
            .to_path();
        let target = signatures.files.values().find_map(|file| {
            let ResolvedName::Function(function) = file
                .declarations()
                .definition_target(&file.declarations().definitions().lookup(&owner)?)?
            else {
                return None;
            };
            file.prepared.lowered.module.functions.iter().find(|f| {
                f.id == function
                    && f.body.is_some()
                    && (f.kind != FunctionKind::TraitMethod
                        || file
                            .prepared
                            .lowered
                            .module
                            .traits
                            .iter()
                            .flat_map(|item| &item.methods)
                            .any(|method| method.function == function && method.has_default))
            })?;
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
        let previous = self.body_cache.get(&owner);
        let previous_environment = previous
            .map(|old| ownership::paths(&old.environment, &old.definitions, cancel))
            .transpose()?;
        let previous_types = previous
            .map(|old| ownership::paths(&old.typed, &old.definitions, cancel))
            .transpose()?;
        let result = if let Some(old) = previous.filter(|old| {
            Arc::ptr_eq(&old.file, &file)
                && *previous_environment
                    .as_ref()
                    .expect("previous body environment")
                    == environment
        }) {
            old.clone()
        } else {
            let prepared = file.authoring(cancel)?;
            let selection = BodySelection::Function(function);
            let names = resolve_bodies(&prepared.lowered, &prepared.names.facts, selection, cancel);
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
                        && old
                            .names
                            .catalog
                            .same_reuse_namespaces(
                                &prepared.names.facts.catalog,
                                &old.names.items,
                                &prepared.names.facts.items,
                                &prepared.names.facts.hosts,
                                cancel,
                            )
                            .unwrap_or(false)
                        && old.file.prepared.declarations.imported_types
                            == file.prepared.declarations.imported_types
                        && previous_environment
                            .as_ref()
                            .expect("previous body environment")
                            .imported_functions
                            == environment.imported_functions
                        && previous_environment
                            .as_ref()
                            .expect("previous body environment")
                            .aggregates
                            .same_contracts(&environment.aggregates)
                })
                .map(|old| BodyReuse {
                    previous_diagnostics: old.diagnostics(),
                    previous_lowered: old.lowered(),
                    previous_types: &previous_types
                        .as_ref()
                        .expect("previous body types")
                        .facts()
                        .type_table,
                    old_text: old.source().text(),
                    new_text: file.source().text(),
                });
            let typed = check_bodies_controlled(
                &prepared.lowered,
                &names,
                &declarations,
                BodyInputs {
                    const_limits: self.const_limits,
                    selection,
                    signatures: &prepared.signatures,
                    imported_functions: &environment.imported_functions,
                    aggregates: &environment.aggregates,
                },
                reuse.as_ref(),
                cancel,
            );
            let environment =
                ownership::scope(&environment, &self.definitions, cancel)?.into_records();
            let mut declarations =
                ownership::scope(&declarations, &self.definitions, cancel)?.into_records();
            let typed = ownership::scope(&typed, &self.definitions, cancel)?.into_records();
            let definitions = self.definitions.snapshot();
            declarations.publish_definitions(definitions.clone());
            Arc::new(FunctionAnalysis {
                owner: owner_id,
                definitions,
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
                    .declaration(&DeclarationId::Definition(id.clone()))
                    .is_some()
            });
            if let Some(result) = result {
                let owner = result
                    .definitions
                    .resolve(result.owner)
                    .expect("published body owner")
                    .to_path();
                self.body_cache
                    .insert(owner, result)
                    .expect("bounded function cache identity");
            }
        }
    }
}

impl FunctionAnalysis {
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    pub fn type_table_to_paths(
        &self,
        cancel: &CancellationToken,
    ) -> Result<TypeTable, DefinitionMappingError> {
        ownership::paths(self.type_table(), &self.definitions, cancel)
    }
}
