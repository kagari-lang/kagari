//! A function query checks its selected body, module constants and runtime static initializers.
//! Local bodies additionally resolve lexical ancestors without checking their bodies.

#[cfg(test)]
use crate::tests::cancellation::{Checkpoint, checkpoint};

use crate::{
    AnalysisResult,
    analysis::{
        AnalysisDatabase,
        eligibility::BodyEligibility,
        error::AnalysisError,
        ownership,
        signatures::{FileSignatures, SignatureSnapshot},
    },
    declarations::Declarations,
    hir::{
        ids::{BodySelection, FunctionId},
        item::function::FunctionKind,
    },
    lower::LoweredModule,
    resolver::{
        collect::resolve_bodies,
        resolved::{ResolvedName, ResolvedNames},
    },
    semantic::body::ModuleFacts,
    typeck::{BodyInputs, check::check_bodies_controlled, table::TypeTable},
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

/// Immutable analysis of one selected function and its constant/static initializer prerequisites.
///
/// Retains the signature snapshot and file lowering that own all referenced HIR IDs.
/// `names`, `declarations` and `typed` contain the selected analysis facts; they do not
/// imply every function in the file was checked. Scoped definition IDs resolve through
/// [`Self::definitions`]; checked types use that same retained definition scope,
/// not portable identities. Analyzer views provide cursor selection separately.
///
/// For `fn good() -> i32 { 1 } fn bad() -> i32 { true }`, querying `good` retains
/// its expression facts and both declarations' signatures, but does not check
/// `bad`'s body. [`Self::eligibility`] reports positive completion evidence for the
/// selected body; absent facts are not evidence of success. Cancellation returns
/// an error from [`AnalysisDatabase::body`] and publishes no partial result.
#[derive(Debug)]
pub struct FunctionAnalysis {
    signatures: SignatureSnapshot,
    file: Arc<FileSignatures>,
    facts: FunctionFacts,
}

#[derive(Debug)]
struct FunctionFacts {
    /// Selected declaration identity in the retained definition table.
    owner: DefinitionId,
    /// Immutable prefix owning the body's scoped definition references.
    definitions: DefinitionTable,
    /// Local function slot interpreted only against the matching query file's lowering.
    function: FunctionId,
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

    /// Returns the selected declaration's identity in this result's definition table.
    pub fn owner(&self) -> &DefinitionId {
        &self.facts.owner
    }

    /// Borrows the source revision owning the selected function.
    pub fn source(&self) -> &SourceFile {
        self.file.source()
    }

    /// Borrows the requested signature context against which these body facts remain valid.
    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    /// Local IDs are interpreted only against this result's lowering and facts.
    pub fn lowered(&self) -> &LoweredModule {
        &self.file.prepared.lowered
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
}

impl AnalysisDatabase {
    /// Checks a selected function after preparing signature dependencies.
    /// The owner must come from this database's definition context. Callers with an
    /// external path first look it up in a declaration snapshot; foreign IDs reject.
    ///
    /// Returns `Ok(None)` when the owner does not select an available source body (including
    /// a trait method without a default). Constants and runtime static initializers,
    /// including their local function declarations, are also checked. Source
    /// diagnostics remain in the returned analysis; signature diagnostics are available
    /// through its retained [`SignatureSnapshot`].
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
                    && f.implementation.script_body().is_some()
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
            self.publish_signatures(signatures);
            return Ok(None);
        };
        let environment = signatures
            .body_environments(self.const_limits, cancel)?
            .remove(&file.source().id())
            .expect("function environment");
        let prepared = &file.prepared;
        let selection = BodySelection::Function(function);
        let names = resolve_bodies(
            &prepared.lowered,
            &prepared.record.names.facts,
            selection,
            cancel,
        );
        let declarations =
            prepared
                .record
                .declarations
                .clone()
                .with_bindings(&prepared.lowered, &names, cancel);
        let typed = check_bodies_controlled(
            &prepared.lowered,
            &names,
            &declarations,
            BodyInputs {
                const_limits: self.const_limits,
                selection,
                signatures: &prepared.record.signatures,
                body_signatures: &environment.signatures,
                imported_functions: &environment.imported_functions,
                imported_statics: &environment.imported_statics,
                imported_constants: &environment.imported_constants,
                constants: environment.constants.as_deref(),
                aggregates: &environment.aggregates,
            },
            cancel,
        )?;
        let mut declarations =
            ownership::scope(&declarations, &self.definitions, cancel)?.into_records();
        let typed = ownership::scope(&typed, &self.definitions, cancel)?.into_records();
        let definitions = self.definitions.snapshot();
        declarations.publish_definitions(definitions.clone());
        let result = Arc::new(FunctionAnalysis {
            signatures: signatures.clone(),
            file,
            facts: FunctionFacts {
                owner: owner_id,
                definitions,
                function,
                names,
                declarations,
                typed,
            },
        });
        #[cfg(test)]
        checkpoint(Checkpoint::Publish);
        cancel.check()?;
        self.publish_signatures(signatures);
        Ok(Some(result))
    }
}

impl FunctionAnalysis {
    /// Borrows the identity table owning scoped IDs in this body's retained facts.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.facts.definitions
    }
}
