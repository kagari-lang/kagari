//! A function query checks its selected body, module constants and runtime static initializers.
//! Local bodies additionally resolve lexical ancestors without checking their bodies.

#[cfg(test)]
use crate::tests::cancellation::{Checkpoint, checkpoint};

use super::signatures::BodyEnvironment;
use crate::{
    AnalysisResult,
    analysis::{
        AnalysisDatabase,
        declarations::DeclarationSnapshot,
        eligibility::BodyEligibility,
        error::AnalysisError,
        ownership,
        signatures::{FileSignatures, SignatureSnapshot},
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
    semantic::body::ModuleFacts,
    typeck::{BodyInputs, check::check_bodies_controlled, reuse::BodyReuse, table::TypeTable},
    types::TypeId,
};

use std::sync::Arc;
use {
    kagari_common::{
        cancellation::{CancellationToken, Cancelled},
        identity::{
            mapping::DefinitionMappingError,
            table::{DefinitionId, DefinitionTable},
        },
    },
    kagari_source::{diagnostic::Diagnostic, source::SourceFile, source_database::SourceSnapshot},
};

#[cfg(test)]
mod tests;

/// Immutable analysis of one selected function and its constant/static initializer prerequisites.
///
/// Retains the signature snapshot and file lowering that own all referenced HIR IDs.
/// `names`, `declarations` and `typed` contain the selected analysis facts; they do not
/// imply every function in the file was checked. Scoped definition IDs resolve through
/// [`Self::definitions`]; types returned by position queries use that same retained
/// definition scope, not portable identities.
/// Unchanged body facts can be shared with a newer signature snapshot; the query's
/// shared diagnostics and eligibility always use that requested signature context.
///
/// For `fn good() -> i32 { 1 } fn bad() -> i32 { true }`, querying `good` retains
/// its expression facts and both declarations' signatures, but does not check
/// `bad`'s body. [`Self::eligibility`] reports positive completion evidence for the
/// selected body; absent facts are not evidence of success. Cancellation returns
/// an error from [`AnalysisDatabase::body`] and publishes no partial result.
#[derive(Debug)]
pub struct FunctionAnalysis {
    signatures: SignatureSnapshot,
    facts: Arc<FunctionFacts>,
}

/// Immutable body facts kept together with their matching source and identity owners.
/// Refreshing unrelated signature inputs shares this record without copying its tables.
#[derive(Debug)]
struct FunctionFacts {
    /// Selected declaration identity in the retained definition table.
    owner: DefinitionId,
    /// Immutable prefix owning the body's scoped definition references.
    definitions: DefinitionTable,
    /// Local function slot in the lowering retained by `file`.
    function: FunctionId,
    file: Arc<FileSignatures>,
    environment: BodyEnvironment,
    /// Resolved lexical names for the selected body analysis.
    names: ResolvedNames,
    /// Declaration sites including bindings collected for the selected body.
    declarations: Declarations,
    typed: AnalysisResult<ModuleFacts>,
}

impl FunctionAnalysis {
    /// Reports whether the selected body completed under valid shared inputs.
    ///
    /// Signature/parser failures anywhere in the prepared environment invalidate
    /// this evidence, even when local recovery produced useful expression types.
    /// The body proof was recorded before diagnostic presentation truncation.
    /// A complete body does not authorize compilation of the dependency closure;
    /// constants, other bodies and linking still require their own checks.
    pub fn eligibility(&self) -> BodyEligibility {
        if self.signatures.shared_inputs_valid()
            && self
                .facts
                .typed
                .facts
                .valid_bodies
                .contains(&self.facts.function)
        {
            BodyEligibility::Complete
        } else {
            BodyEligibility::Unavailable
        }
    }

    fn contains(&self, offset: usize) -> bool {
        self.facts.names.scopes().iter().any(|scope| {
            scope.owner == BodyOwner::Function(self.facts.function)
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
        &self.facts.owner
    }

    /// Borrows the source revision owning the selected function.
    pub fn source(&self) -> &SourceFile {
        self.facts.file.source()
    }

    /// Borrows the requested signature context against which these body facts remain valid.
    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    /// Local IDs are interpreted only against this result's lowering and facts.
    pub fn lowered(&self) -> &LoweredModule {
        &self.facts.file.prepared.lowered
    }

    /// Returns the function index to look up in [`Self::lowered`].
    pub fn function(&self) -> FunctionId {
        self.facts.function
    }

    /// Borrows resolved lexical names for the selected body analysis.
    pub fn names(&self) -> &ResolvedNames {
        &self.facts.names
    }

    /// Borrows declaration sites including bindings collected for the selected body.
    pub fn declarations(&self) -> &Declarations {
        &self.facts.declarations
    }

    /// Borrows checked types and lowering decisions for the selected analysis.
    pub fn type_table(&self) -> &TypeTable {
        &self.facts.typed.facts.type_table
    }

    /// Body diagnostics and constant/static initializer prerequisites. Header diagnostics are
    /// available separately through the retained signature snapshot.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.facts.typed.diagnostics()
    }

    /// Returns the number of bodies checked while constructing this result.
    pub fn checked_bodies(&self) -> usize {
        self.facts.typed.facts.checked_bodies
    }

    /// Returns the number of bodies reused while constructing this result.
    pub fn reused_bodies(&self) -> usize {
        self.facts.typed.facts.reused_bodies
    }

    /// Whole-result sharing also requires the complete signature snapshot to match.
    /// A new context may change shared diagnostics without changing this body's inputs.
    fn same_signature_snapshot(
        &self,
        signatures: &SignatureSnapshot,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        if self.signatures.revision() != signatures.revision()
            || self.signatures.files.len() != signatures.files.len()
        {
            return Ok(false);
        }
        for (id, file) in signatures.files.iter() {
            cancel.check()?;
            if self
                .signatures
                .file(*id)
                .is_none_or(|old| !Arc::ptr_eq(old, file))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl AnalysisDatabase {
    /// Checks or reuses a selected function after preparing signature dependencies.
    /// The owner must come from this database's definition context. Callers with an
    /// external path first look it up in a declaration snapshot; foreign IDs reject.
    ///
    /// Returns `Ok(None)` when the owner does not select an available source body (including
    /// a trait method without a default). Constants and runtime static initializers,
    /// including their local function declarations, are also checked. Source
    /// diagnostics remain in the returned analysis; signature diagnostics are available
    /// through its retained [`SignatureSnapshot`].
    ///
    /// An unchanged file signature result and equal body environment permit sharing
    /// body facts. Changes elsewhere refresh the retained signature context; an
    /// unchanged complete context also permits retaining the result's `Arc`. Otherwise
    /// compatible imports, types and aggregate contracts may permit body-fact remapping;
    /// edits are not assumed reusable merely because the name matches.
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
            .body_environments(self.const_limits, cancel)?
            .remove(&file.source().id())
            .expect("function environment");
        let previous = self.body_cache.get_id(owner_id);
        let previous_environment = previous.map(|old| &old.facts.environment);
        let previous_types = previous.map(|old| &old.facts.typed);
        let result = if let Some(old) = previous.filter(|old| {
            Arc::ptr_eq(&old.facts.file, &file)
                && *previous_environment
                    .as_ref()
                    .expect("previous body environment")
                    == &environment
        }) {
            if old.same_signature_snapshot(&signatures, cancel)? {
                old.clone()
            } else {
                Arc::new(FunctionAnalysis {
                    signatures: signatures.clone(),
                    facts: old.facts.clone(),
                })
            }
        } else {
            let prepared = &file.prepared;
            let selection = BodySelection::Function(function);
            let names = resolve_bodies(&prepared.lowered, &prepared.names.facts, selection, cancel);
            let declarations =
                prepared
                    .declarations
                    .clone()
                    .with_bindings(&prepared.lowered, &names, cancel);
            let reusable = previous
                .map(|old| -> Result<bool, AnalysisError> {
                    Ok(old
                        .facts
                        .file
                        .declaration
                        .parsed
                        .physical()
                        .diagnostics()
                        .is_empty()
                        && file.declaration.parsed.physical().diagnostics().is_empty()
                        && old.diagnostics().is_empty()
                        && old.facts.file.diagnostics().is_empty()
                        && file.diagnostics().is_empty()
                        && old
                            .facts
                            .file
                            .prepared
                            .names
                            .facts
                            .imports
                            .same_bindings(&prepared.names.facts.imports)
                        && old.facts.names.catalog.same_reuse_namespaces(
                            &prepared.names.facts.catalog,
                            &old.facts.names.items,
                            &prepared.names.facts.items,
                            cancel,
                        )?
                        && old
                            .facts
                            .file
                            .prepared
                            .declarations
                            .imported_types
                            .same_reuse_inputs(
                                &prepared.declarations.imported_types,
                                old.facts.file.prepared.names.facts.items.unit.as_ref(),
                                prepared.names.facts.items.unit.as_ref(),
                            )
                        && previous_environment
                            .as_ref()
                            .expect("previous body environment")
                            .imported_functions
                            == environment.imported_functions
                        && old.facts.environment.imported_statics == environment.imported_statics
                        && old.facts.environment.imported_constants
                            == environment.imported_constants
                        && previous_environment
                            .as_ref()
                            .expect("previous body environment")
                            .aggregates
                            .same_contracts(&environment.aggregates))
                })
                .transpose()?
                .unwrap_or(false);
            let reuse = previous.filter(|_| reusable).map(|old| BodyReuse {
                previous_valid_bodies: &old.facts.typed.facts().valid_bodies,
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
                    body_signatures: &environment.signatures,
                    imported_functions: &environment.imported_functions,
                    imported_statics: &environment.imported_statics,
                    imported_constants: &environment.imported_constants,
                    constants: environment.constants.as_deref(),
                    aggregates: &environment.aggregates,
                },
                reuse.as_ref(),
                cancel,
            )?;
            let environment =
                ownership::scope(&environment, &self.definitions, cancel)?.into_records();
            let mut declarations =
                ownership::scope(&declarations, &self.definitions, cancel)?.into_records();
            let typed = ownership::scope(&typed, &self.definitions, cancel)?.into_records();
            let definitions = self.definitions.snapshot();
            declarations.publish_definitions(definitions.clone());
            Arc::new(FunctionAnalysis {
                signatures: signatures.clone(),
                facts: Arc::new(FunctionFacts {
                    owner: owner_id,
                    definitions,
                    function,
                    file,
                    environment,
                    names,
                    declarations,
                    typed,
                }),
            })
        };
        #[cfg(test)]
        checkpoint(Checkpoint::Publish);
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
                    .insert_id(result.facts.owner, result)
                    .expect("function cache uses the analysis definition scope");
            }
        }
    }
}

impl FunctionAnalysis {
    /// Borrows the identity table owning scoped IDs in this body's retained facts.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.facts.definitions
    }
}
