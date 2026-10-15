//! Recoverable source analysis and the checked HIR boundary for Kagari.
//!
//! # Reading the data
//!
//! [`lower::LoweredModule`] owns structural HIR plus source ranges. [`hir::item::Module`]
//! contains declarations and a shared [`hir::body::Body`] arena. Child nodes are
//! connected by typed IDs; [`resolver::resolved::ResolvedNames`] and
//! [`typeck::table::TypeTable`] add independent name/type/call facts using those IDs.
//! The [`hir::ids`] documentation explains where each handle is looked up.
//!
//! ```text
//! source snapshot + installed declarations
//!   -> analysis preparation: parse/lower, namespaces, declarations, signatures
//!   -> body resolution + type checking -> facts and diagnostics
//!   -> checked analysis/program -> compiler source lowering
//! ```
//!
//! This is a dependency outline, not a claim that every query scans or checks every
//! body. [`analysis::AnalysisDatabase`] prepares immutable snapshots; their query
//! APIs distinguish declaration, signature and body work and reuse retained inputs.
//! HIR does not execute scripts. [`CheckedAnalysis`] and [`program::CheckedProgram`]
//! feed compiler lowering; MIR verification and runtime execution have other owners.
//!
//! Start with [`lower::lower_module`] for a runnable node/storage example and
//! [`analyze_source`] for single-source semantic analysis with explicit providers.
//! The [repository reading guide](https://github.com/kagari-lang/kagari/blob/HEAD/docs/architecture/hir.md)
//! links the local and cross-module example traces to their implementation owners.

use crate::analysis::error::AnalysisError;
use crate::{
    analysis::ownership::recover_invalid_identity,
    hir::ids::BodySelection,
    imports::{
        ModuleGraph, ModuleImportFacts, SourceUnit, catalog::NamespaceCatalog,
        functions::ImportedFunctions, types::ImportedTypes,
    },
    language::items as language_items,
    resolver::{
        collect::{collect_declarations, resolve_bodies},
        resolved::{DeclarationNames, ResolvedNames},
    },
    typeck::{
        applications::validate_signatures,
        check::{check_bodies_controlled, check_signatures},
        const_budget::ConstLimits,
        definition_registration,
        reuse::BodyReuse,
        signature_reuse::reuse_signatures,
        supertraits::validate,
    },
};
use analysis::AnalysisDatabase;
use declarations::Declarations;
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        MAX_IDENTITY_PATH_SEGMENTS, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata, table::DefinitionTable,
    },
    span::Span,
};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind, Severity},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::parser::Parse;
use kagari_types::declaration::module::ModuleDecl;
use smallvec::SmallVec;
use std::{ops::Deref, sync::Arc};
use typeck::associated_consts;

pub mod aggregates;
pub mod analysis;
pub mod builtin;
pub mod callable;
pub mod declarations;
pub mod hir;
pub mod imports;
pub mod language;
pub mod lower;
pub mod native;

pub mod program;
pub mod resolver;
pub mod source_map;
pub mod typeck;
pub mod types;

/// Analysis diagnostics with four inline slots; additional diagnostics spill to the heap.
pub type DiagnosticBuffer = SmallVec<[Diagnostic; 4]>;

/// Owned diagnostic buffer used by checked-conversion failure paths.
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

/// A module's lowered syntax plus independently produced semantic facts.
///
/// ```text
/// AnalyzedModule
/// +-- lowered: Arc<LoweredModule> -> source + Module/Body + SourceMap
/// +-- names: ResolvedNames       -> ExprId/PlaceId -> bindings, shared namespaces
/// +-- declarations              -> source declarations and scoped identities
/// +-- signatures: Arc<AnalysisResult<ModuleSignatures>>
/// +-- typed: TypedModule      -> per-body types and selected operations
/// +-- aggregates                -> nominal/trait/implementation catalog
/// `-- imported_functions        -> callable facts for cross-module targets
/// ```
///
/// Semantic processing uses definition-table-scoped IDs. Facts may accompany
/// error diagnostics; only the
/// checked conversion produces [`CheckedAnalysis`] for executable lowering.
/// This bundle retains its source analysis and namespace/Host inputs. Reference
/// validation covers those inputs, but the bundle cannot be projected to paths,
/// serialized or rebound as though its source ownership were portable. Independently
/// reusable type/signature facts support explicit ID-to-ID remapping.
#[derive(Debug, Clone)]
pub struct AnalyzedModule {
    /// Nominal types, traits and implementation facts available to this analysis.
    pub aggregates: aggregates::AggregateCatalog,
    /// Shared matching source, node storage and source ranges.
    pub lowered: Arc<lower::LoweredModule>,
    /// Resolved scope, expression, place and pattern bindings.
    pub names: ResolvedNames,
    /// Declaration/binding metadata and definition identity context.
    pub declarations: declarations::Declarations,
    /// Checked or recovered function/constant facts, including expression types.
    pub typed: typeck::TypedModule,
    /// Shared signature-stage facts and their diagnostics.
    pub signatures: Arc<AnalysisResult<typeck::ModuleSignatures>>,
    /// Signatures/identities of callable declarations reached through imports.
    pub imported_functions: ImportedFunctions,
}

/// Useful analysis facts together with diagnostics, including recoverable errors.
///
/// Retrieving [`Self::facts`] does not validate the result. [`Self::into_checked`]
/// rejects error-severity diagnostics, while warnings do not prevent extraction.
/// Only the specialized code-generation conversion creates the checked HIR boundary.
#[derive(Debug, Clone)]
pub struct AnalysisResult<T> {
    /// Produced facts; validity is determined together with the diagnostics.
    pub(crate) facts: T,
    /// Diagnostics retained alongside the facts.
    pub(crate) diagnostics: DiagnosticBuffer,
}

impl<T> AnalysisResult<T> {
    /// Borrows facts even when error diagnostics are present.
    pub fn facts(&self) -> &T {
        &self.facts
    }

    /// Borrows diagnostics in the producer's accumulated order.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Extracts facts when no error-severity diagnostic is present.
    ///
    /// # Errors
    ///
    /// Returns the complete diagnostic buffer if any entry has error severity.
    /// This generic operation alone does not create the code-generation seal.
    pub fn into_checked(self) -> Result<T, BoxedDiagnosticBuffer> {
        if self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            Err(Box::new(self.diagnostics))
        } else {
            Ok(self.facts)
        }
    }
}

/// Only this type may cross the code-generation boundary. Its contents cannot
/// be mutated after validation by external callers.
#[derive(Debug, Clone)]
pub struct CheckedAnalysis(DefinitionMetadata<AnalyzedModule>);

impl Deref for CheckedAnalysis {
    type Target = AnalyzedModule;

    fn deref(&self) -> &Self::Target {
        self.0.records()
    }
}

impl CheckedAnalysis {
    /// Borrows the definition table that gives all scoped IDs in this result their meaning.
    pub fn definitions(&self) -> &DefinitionTable {
        self.0.definitions()
    }

    /// Share the append-only identity allocator for generated compilation definitions.
    ///
    /// Clones use one writer, so independent compilations cannot assign conflicting
    /// meanings to an ID. Appending names does not extend this checked result's
    /// retained prefix or grant declaration/execution evidence. Generated records
    /// must retain a new snapshot and pass their ordinary semantic verifier.
    pub fn definition_context(&self) -> DefinitionContext {
        self.declarations.context().clone()
    }

    /// Extract editable scoped facts for specialization, dropping executable evidence.
    /// The result retains its identity prefix and must be verified again after mutation.
    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<AnalyzedModule>, DefinitionMappingError> {
        DefinitionMetadata::checked(
            self.0.definitions().clone(),
            self.0.records().clone(),
            cancel,
        )
    }

    pub(crate) fn adopt_scoped(
        module: &AnalyzedModule,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self(DefinitionMetadata::checked(
            definitions.clone(),
            module.clone(),
            cancel,
        )?))
    }
}

/// Internal signature-stage bundle passed to body checking.
///
/// Retains declaration inputs and shared checked signatures. `local_signature_diagnostics`
/// separates reusable local diagnostics from the suffix recomputed against the completed
/// aggregate catalog; signature reuse must not preserve stale cross-module diagnostics.
#[derive(Debug, Clone)]
pub(crate) struct PreparedAnalysis {
    signatures_reused: bool,
    // The suffix after this boundary depends on the complete aggregate catalog.
    // Recompute it after all declaration signatures exist, including on reuse.
    local_signature_diagnostics: usize,
    lowered: Arc<lower::LoweredModule>,
    names: AnalysisResult<DeclarationNames>,
    declarations: declarations::Declarations,
    signatures: Arc<AnalysisResult<typeck::ModuleSignatures>>,
}

impl PreparedAnalysis {
    /// Validates signature contracts that require all modules' aggregate declarations to exist.
    fn completed_signatures(
        &self,
        aggregates: &aggregates::AggregateCatalog,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<AnalysisResult<typeck::ModuleSignatures>>>, Cancelled> {
        cancel.check()?;
        if !self.lowered.source.module_identity().within_path_limit() {
            return Ok(None);
        }
        let mut diagnostics = DiagnosticBuffer::new();
        let definitions = self.declarations.definitions();
        language_items::validate_shapes(
            &self.declarations,
            aggregates,
            &self.lowered.registered_traits,
            &mut diagnostics,
            cancel,
        );
        associated_consts::validate(
            &self.lowered,
            &self.declarations,
            aggregates,
            self.signatures.facts().type_table(),
            &mut diagnostics,
            cancel,
        );
        validate(
            &self.lowered,
            &self.declarations,
            aggregates,
            self.signatures.facts().type_table(),
            &mut diagnostics,
            cancel,
        );
        validate_signatures(
            &self.lowered,
            &self.declarations,
            self.signatures.facts(),
            aggregates,
            &mut diagnostics,
            cancel,
        );
        for (first, second) in aggregates.overlapping_implementations(definitions) {
            cancel.check()?;
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: definitions
                        .resolve(first.trait_type.declaration)
                        .expect("checked trait identity")
                        .last()
                        .map(|segment| segment.name.to_owned())
                        .unwrap_or_default(),
                    type_name: first.for_type.display_name(definitions),
                    reason: format!(
                        "overlapping implementations in {} and {}",
                        definitions
                            .resolve(first.id)
                            .expect("checked implementation identity")
                            .module(),
                        definitions
                            .resolve(second.id)
                            .expect("checked implementation identity")
                            .module()
                    ),
                })
                .with_span(Span::default()),
            );
        }
        cancel.check()?;
        if &self.signatures.diagnostics()[self.local_signature_diagnostics..]
            == diagnostics.as_slice()
        {
            return Ok(None);
        }
        let mut result = self.signatures.as_ref().clone();
        result
            .diagnostics
            .truncate(self.local_signature_diagnostics);
        result.diagnostics.extend(diagnostics);
        Ok(Some(Arc::new(result)))
    }
}

/// Internal declaration-stage bundle: shared lowering, resolved module names and declaration sites.
#[derive(Debug, Clone)]
pub(crate) struct DeclaredAnalysis {
    lowered: Arc<lower::LoweredModule>,
    names: AnalysisResult<DeclarationNames>,
    declarations: declarations::Declarations,
}

impl DeclaredAnalysis {
    /// Checks signatures or remaps compatible previous facts after comparing imported types and namespace inputs.
    fn check_signatures(
        mut self,
        imported_types: ImportedTypes,
        previous_analysis: Option<&PreparedAnalysis>,
        cancel: &CancellationToken,
    ) -> Result<PreparedAnalysis, DefinitionMappingError> {
        self.declarations.imported_types = imported_types;
        self.declarations
            .publish_definitions(self.declarations.context().snapshot());
        let previous = previous_analysis.and_then(|old| {
            if !(old
                .names
                .facts
                .imports
                .same_signature_bindings(&self.names.facts.imports)
                && old
                    .names
                    .facts
                    .catalog
                    .same_signature_namespaces(
                        &self.names.facts.catalog,
                        &old.names.facts.items,
                        &self.names.facts.items,
                        cancel,
                    )
                    .unwrap_or(false)
                && old.declarations.imported_types == self.declarations.imported_types)
            {
                return None;
            }
            if old.lowered.source.revision() == self.lowered.source.revision()
                && old.lowered.source.module_identity() == self.lowered.source.module_identity()
                && old.lowered.module.body.arena() == self.lowered.module.body.arena()
            {
                Some(old.signatures.clone())
            } else {
                reuse_signatures(&old.lowered, &old.signatures, &self.lowered, cancel).map(Arc::new)
            }
        });
        let signatures_reused = previous.is_some();
        let reused_local_diagnostics = previous.as_ref().map(|_| {
            previous_analysis
                .expect("signature reuse source")
                .local_signature_diagnostics
        });
        let signatures = previous.unwrap_or_else(|| {
            Arc::new(check_signatures(&self.lowered, &self.declarations, cancel))
        });
        let definitions = definition_registration::implementation_members(
            &self.lowered,
            &self.declarations,
            signatures.facts(),
            cancel,
        )?;
        self.declarations.publish_definitions(definitions);
        Ok(PreparedAnalysis {
            signatures_reused,
            local_signature_diagnostics: reused_local_diagnostics
                .unwrap_or(signatures.diagnostics().len()),
            lowered: self.lowered,
            names: self.names,
            declarations: self.declarations,
            signatures,
        })
    }
}

fn declare_analysis(
    lowered: Arc<lower::LoweredModule>,
    imports: Arc<ModuleImportFacts>,
    catalog: Arc<NamespaceCatalog>,
    definitions: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<DeclaredAnalysis, AnalysisError> {
    let (imports, catalog) = if imports.scope.unit.is_none() {
        let graph = ModuleGraph::build([lowered.as_ref()], cancel)?;
        (
            graph
                .imports_for(&SourceUnit::of(&lowered))
                .expect("module node")
                .clone(),
            graph.catalog,
        )
    } else {
        (imports, catalog)
    };
    let mut names = collect_declarations(&lowered, imports, catalog, cancel);
    if !lowered.source.module_identity().within_path_limit() {
        names
            .diagnostics
            .push(Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                resource: "module identity path segments",
                limit: MAX_IDENTITY_PATH_SEGMENTS,
            }));
    }
    let mut declarations = if lowered.source.module_identity().within_path_limit() {
        Declarations::collect_named(&lowered.source, &lowered, &names.facts, definitions, cancel)?
    } else {
        Declarations::empty(&lowered, &names.facts, definitions)
    };
    if lowered.source.module_identity().within_path_limit() {
        declarations.language_items =
            language_items::collect(&lowered, &declarations, &mut names.diagnostics);
    }
    Ok(DeclaredAnalysis {
        lowered,
        names,
        declarations,
    })
}

fn analyze_prepared(
    prepared: PreparedAnalysis,
    const_limits: ConstLimits,
    imported_functions: ImportedFunctions,
    aggregates: aggregates::AggregateCatalog,
    reuse: Option<&BodyReuse<'_>>,
    cancel: &CancellationToken,
) -> AnalysisResult<AnalyzedModule> {
    let PreparedAnalysis {
        signatures_reused: _,
        local_signature_diagnostics: _,
        lowered,
        names,
        declarations,
        signatures,
    } = prepared;
    let names = AnalysisResult {
        facts: resolve_bodies(&lowered, &names.facts, BodySelection::All, cancel),
        diagnostics: names.diagnostics,
    };
    let declarations = declarations.with_bindings(&lowered, &names.facts, cancel);
    let typed = check_bodies_controlled(
        &lowered,
        &names.facts,
        &declarations,
        typeck::BodyInputs {
            const_limits,
            selection: BodySelection::All,
            signatures: &signatures,
            imported_functions: &imported_functions,
            aggregates: &aggregates,
        },
        reuse,
        cancel,
    );
    let mut diagnostics = names.diagnostics.clone();
    diagnostics.extend(typed.diagnostics);
    AnalysisResult {
        facts: AnalyzedModule {
            aggregates,
            lowered,
            names: names.facts,
            declarations,
            signatures,
            imported_functions,
            typed: typed.facts,
        },
        diagnostics,
    }
}

/// Analyzes one source with explicitly installed native/foundation providers.
///
/// Returns recoverable facts and source diagnostics. This convenience entrypoint
/// creates a fresh database; use [`AnalysisDatabase`] to retain caches across edits.
///
/// # Errors
///
/// Returns [`AnalysisError`] if provider import or identity conversion fails.
///
/// # Example
///
/// ```
/// use kagari_hir::{analyze_source, hir::expr::ExprKind, types::TypeId};
/// use kagari_source::source::SourceFile;
/// use kagari_types::scalar::BuiltinType;
///
/// let source = SourceFile::new("add.kgr", "fn add(x: i32) -> i32 { val y = x + 1; y }");
/// let analysis = analyze_source(&source, kagari_stdlib::catalog::shared().unwrap()).unwrap();
/// let facts = analysis.facts();
/// let (sum, _) = facts.lowered.module.body.expressions()
///     .find(|(_, expr)| matches!(expr.kind, ExprKind::Binary { .. })).unwrap();
/// assert_eq!(facts.typed.type_table.expr_type(sum), Some(TypeId::Builtin(BuiltinType::I32)));
/// assert!(analysis.into_codegen().is_ok());
/// ```
pub fn analyze_source(
    source: &SourceFile,
    providers: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
) -> Result<AnalysisResult<AnalyzedModule>, AnalysisError> {
    if !source.module_identity().within_path_limit() {
        return recover_invalid_identity(source);
    }
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(providers);
    let snapshot = database.snapshot(
        SourceSnapshot::single_file(Arc::new(source.clone())),
        &Default::default(),
    )?;
    snapshot
        .file(source.id())
        .expect("requested source belongs to its snapshot")
        .to_unverified(&CancellationToken::default())
        .map(DefinitionMetadata::into_records)
        .map_err(AnalysisError::from)
}

/// Per-analysis constant-evaluation and semantic diagnostic limits.
pub(crate) struct AnalysisPolicy {
    const_limits: ConstLimits,
    max_semantic_diagnostics: usize,
}

/// Resolves selected lexical bodies, checks/reuses their facts and merges staged diagnostics.
pub(crate) fn analyze_parsed(
    prepared: PreparedAnalysis,
    parsed: &Parse,
    policy: AnalysisPolicy,
    imported_functions: ImportedFunctions,
    aggregates: aggregates::AggregateCatalog,
    reuse: Option<&BodyReuse<'_>>,
    cancel: &CancellationToken,
) -> AnalysisResult<AnalyzedModule> {
    let mut analyzed = analyze_prepared(
        prepared,
        policy.const_limits,
        imported_functions,
        aggregates,
        reuse,
        cancel,
    );

    for attribute in &analyzed.facts.lowered.attributes {
        if analyzed
            .facts
            .lowered
            .native_attributes
            .contains(&(attribute.span.start, attribute.span.end))
        {
            continue;
        }
        let kind = match attribute.name.as_str() {
            "meta" | "lang" => continue,
            "reflect" | "requires" | "profile" => DiagnosticKind::UnsupportedAttribute {
                name: attribute.name.clone(),
            },
            name if name.starts_with("tool::") => continue,
            _ => DiagnosticKind::UnknownAttribute {
                name: attribute.name.clone(),
            },
        };
        analyzed
            .diagnostics
            .push(Diagnostic::error(kind).with_span(attribute.span));
    }
    if analyzed.diagnostics.len() > policy.max_semantic_diagnostics {
        analyzed
            .diagnostics
            .truncate(policy.max_semantic_diagnostics);
        analyzed
            .diagnostics
            .push(Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                resource: "semantic diagnostics",
                limit: policy.max_semantic_diagnostics,
            }));
    }
    analyzed
        .diagnostics
        .extend(parsed.diagnostics().iter().cloned());
    analyzed
}

#[cfg(test)]
mod tests;

mod identity_mapping;
mod identity_records;

impl AnalysisResult<AnalyzedModule> {
    /// Validates diagnostics and adopts definition metadata for code generation.
    ///
    /// # Errors
    ///
    /// Returns diagnostics for analysis errors or failure to adopt bounded definition
    /// metadata. Successful results retain their existing identity prefix in immutable
    /// [`CheckedAnalysis`].
    pub fn into_codegen(self) -> Result<CheckedAnalysis, BoxedDiagnosticBuffer> {
        let module = self.into_checked()?;
        CheckedAnalysis::adopt_scoped(
            &module,
            module.declarations.definitions(),
            &CancellationToken::default(),
        )
        .map_err(|_| {
            Box::new(DiagnosticBuffer::from_iter([Diagnostic::error(
                DiagnosticKind::CompileLimitExceeded {
                    resource: "definition metadata",
                    limit: 1_000_000,
                },
            )]))
        })
    }
}
