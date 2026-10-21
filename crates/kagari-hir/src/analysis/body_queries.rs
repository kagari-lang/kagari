//! A function query checks its selected body and module constants.
//! Local bodies additionally resolve lexical ancestors without checking their bodies.

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
            table::{DefinitionId, DefinitionTable},
        },
    },
    kagari_source::{diagnostic::Diagnostic, source::SourceFile, source_database::SourceSnapshot},
};

#[cfg(test)]
mod tests;

/// Immutable analysis of one selected function and its constant prerequisites.
///
/// Retains the signature snapshot and file lowering that own all referenced HIR IDs.
/// `names`, `declarations` and `typed` contain the selected analysis facts; they do not
/// imply every function in the file was checked. Scoped definition IDs resolve through
/// [`Self::definitions`], while position type queries return portable identities.
#[derive(Debug)]
pub struct FunctionAnalysis {
    /// Returns the selected declaration's identity in this result's definition table.
    owner: DefinitionId,
    /// Borrows the identity table owning scoped IDs in this result.
    definitions: DefinitionTable,
    /// Returns the function index to look up in [`Self::lowered`].
    function: FunctionId,
    signatures: SignatureSnapshot,
    file: Arc<FileSignatures>,
    environment: BodyEnvironment,
    /// Borrows resolved lexical names for the selected body analysis.
    names: ResolvedNames,
    /// Borrows declaration sites including bindings collected for the selected body.
    declarations: Declarations,
    typed: AnalysisResult<TypedModule>,
}

impl FunctionAnalysis {
    fn contains(&self, offset: usize) -> bool {
        self.names.scopes().iter().any(|scope| {
            scope.owner == BodyOwner::Function(self.function)
                && scope.span.start <= offset
                && offset <= scope.span.end
        })
    }

    /// Returns a checked type in this result's definition scope at a byte offset inside the selected function.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        let ty = self
            .contains(offset)
            .then(|| super::type_at_in(self.lowered(), self.type_table(), offset))
            .flatten()?;
        Some(ty)
    }

    /// Returns a member receiver type in this result's definition scope inside the selected function.
    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        let ty = self
            .contains(offset)
            .then(|| super::member_receiver_type_in(self.lowered(), self.type_table(), offset))
            .flatten()?;
        Some(ty)
    }

    /// Returns the selected declaration's identity in this result's definition table.
    pub fn owner(&self) -> &DefinitionId {
        &self.owner
    }

    /// Borrows the source revision owning the selected function.
    pub fn source(&self) -> &SourceFile {
        self.file.source()
    }

    /// Borrows signature dependencies retained while this body was checked.
    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    /// Local IDs are interpreted only against this result's lowering and facts.
    pub fn lowered(&self) -> &LoweredModule {
        &self.file.prepared.lowered
    }

    /// Returns the function index to look up in [`Self::lowered`].
    pub fn function(&self) -> FunctionId {
        self.function
    }

    /// Borrows resolved lexical names for the selected body analysis.
    pub fn names(&self) -> &ResolvedNames {
        &self.names
    }

    /// Borrows declaration sites including bindings collected for the selected body.
    pub fn declarations(&self) -> &Declarations {
        &self.declarations
    }

    /// Borrows checked types and lowering decisions for the selected analysis.
    pub fn type_table(&self) -> &TypeTable {
        &self.typed.facts.type_table
    }

    /// Body diagnostics and module-constant prerequisites. Header diagnostics are
    /// available separately through the retained signature snapshot.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.typed.diagnostics()
    }

    /// Returns the number of bodies checked while constructing this result.
    pub fn checked_bodies(&self) -> usize {
        self.typed.facts.checked_bodies
    }

    /// Returns the number of bodies reused while constructing this result.
    pub fn reused_bodies(&self) -> usize {
        self.typed.facts.reused_bodies
    }
}

impl AnalysisDatabase {
    /// Checks or reuses a selected function after preparing signature dependencies.
    /// The owner must come from this database's definition context. Callers with an
    /// external path first look it up in a declaration snapshot; foreign IDs reject.
    ///
    /// Returns `Ok(None)` when the owner does not select an available source body (including
    /// a trait method without a default). Constant prerequisites are also checked. Source
    /// diagnostics remain in the returned analysis; signature diagnostics are available
    /// through its retained [`SignatureSnapshot`].
    ///
    /// An unchanged signature result and equal body environment permit retaining the old
    /// `Arc`. Otherwise compatible imports, types and aggregate contracts may permit
    /// body-fact remapping; edits are not assumed reusable merely because the name matches.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError`] for cancellation, invalid native inputs or identity mapping.
    /// Cancellation is checked before publishing the selected-body result.
    pub fn body(
        &mut self,
        source: SourceSnapshot,
        owner: &DefinitionId,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<FunctionAnalysis>>, AnalysisError> {
        let signatures = self.prepare_signatures(source, cancel)?;
        let definitions = self.definitions.snapshot();
        definitions
            .resolve(*owner)
            .map_err(DefinitionMappingError::from)?;
        let owner_id = *owner;
        let target = signatures.files.values().find_map(|file| {
            let ResolvedName::Function(function) = file.declarations().definition_target(owner)?
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
        let previous = self.body_cache.get_id(owner_id);
        let previous_environment = previous.map(|old| &old.environment);
        let previous_types = previous.map(|old| &old.typed);
        let result = if let Some(old) = previous.filter(|old| {
            Arc::ptr_eq(&old.file, &file)
                && *previous_environment
                    .as_ref()
                    .expect("previous body environment")
                    == &environment
        }) {
            old.clone()
        } else {
            let prepared = &file.prepared;
            let selection = BodySelection::Function(function);
            let names = resolve_bodies(&prepared.lowered, &prepared.names.facts, selection, cancel);
            let declarations =
                prepared
                    .declarations
                    .clone()
                    .with_bindings(&prepared.lowered, &names, cancel);
            let reuse = previous
                .filter(|old| {
                    old.file
                        .declaration
                        .parsed
                        .physical()
                        .diagnostics()
                        .is_empty()
                        && file.declaration.parsed.physical().diagnostics().is_empty()
                        && old.diagnostics().is_empty()
                        && old.file.diagnostics().is_empty()
                        && file.diagnostics().is_empty()
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
                    previous_valid_bodies: &old.typed.facts().valid_bodies,
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

    /// Publishes a selected result for a non-older revision and drops owners absent from declarations.
    pub(super) fn publish_body(
        &mut self,
        declarations: &DeclarationSnapshot,
        result: Option<Arc<FunctionAnalysis>>,
    ) {
        if declarations.revision() >= self.body_revision {
            self.body_revision = declarations.revision();
            self.body_cache.retain_ids(|id, _| {
                declarations
                    .declaration(&DeclarationId::Definition(id))
                    .is_some()
            });
            if let Some(result) = result {
                self.body_cache
                    .insert_id(result.owner, result)
                    .expect("function cache uses the analysis definition scope");
            }
        }
    }
}

impl FunctionAnalysis {
    /// Borrows the identity table owning scoped IDs in this result.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }
}
