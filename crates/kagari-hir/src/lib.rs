use crate::{
    hir::ids::BodySelection,
    imports::{functions::ImportedFunctions, types::ImportedTypes},
    resolver::{
        collect::{collect_declarations, resolve_bodies},
        resolved::{DeclarationNames, ResolvedNames},
    },
    typeck::{
        applications::validate_signatures,
        check::{check_bodies_controlled, check_signatures},
        const_budget::ConstLimits,
        reuse::BodyReuse,
        signature_reuse::reuse_signatures,
        supertraits::validate,
    },
};
use analysis::AnalysisDatabase;

use declarations::Declarations;
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    diagnostic::{Diagnostic, DiagnosticKind, Severity},
    identity::{
        DefinitionPath, MAX_IDENTITY_PATH_SEGMENTS,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
        metadata::DefinitionMetadata,
        reference::DefinitionReference,
        table::{DefinitionId, DefinitionTable},
    },
    source::SourceFile,
    source_database::SourceSnapshot,
    span::Span,
};
use kagari_syntax::parser::Parse;
use smallvec::SmallVec;
use std::{ops::Deref, sync::Arc};
use typeck::associated_consts;
pub mod aggregates;
pub mod analysis;
pub mod builtin;
pub mod callable;
pub mod declarations;
pub mod hir;
pub mod host;
pub mod imports;
pub mod language;
pub mod lower;
pub mod native;

pub mod program;
pub mod resolver;
pub mod source_map;
pub mod typeck;
pub mod types;

pub type DiagnosticBuffer = SmallVec<[Diagnostic; 4]>;
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

#[derive(Debug, Clone)]
pub struct AnalyzedModule<I: DefinitionReference = DefinitionPath> {
    pub aggregates: aggregates::AggregateCatalog<I>,
    pub lowered: Arc<lower::LoweredModule>,
    pub names: ResolvedNames,
    pub declarations: declarations::Declarations<I>,
    pub typed: typeck::TypedModule<I>,
    pub signatures: Arc<AnalysisResult<typeck::ModuleSignatures<I>>>,
    pub imported_functions: ImportedFunctions<I>,
}

#[derive(Debug, Clone)]
pub struct AnalysisResult<T> {
    pub(crate) facts: T,
    pub(crate) diagnostics: DiagnosticBuffer,
}

impl<T> AnalysisResult<T> {
    pub fn facts(&self) -> &T {
        &self.facts
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

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
pub struct CheckedAnalysis(DefinitionMetadata<AnalyzedModule<DefinitionId>>);

impl Deref for CheckedAnalysis {
    type Target = AnalyzedModule<DefinitionId>;

    fn deref(&self) -> &Self::Target {
        self.0.records()
    }
}

impl AnalysisResult<AnalyzedModule> {
    pub fn into_codegen(self) -> Result<CheckedAnalysis, BoxedDiagnosticBuffer> {
        let module = self.into_checked()?;
        CheckedAnalysis::adopt(&module, &CancellationToken::default()).map_err(|_| {
            Box::new(DiagnosticBuffer::from_iter([Diagnostic::error(
                DiagnosticKind::CompileLimitExceeded {
                    resource: "definition metadata",
                    limit: 1_000_000,
                },
            )]))
        })
    }
}

impl CheckedAnalysis {
    pub fn definitions(&self) -> &DefinitionTable {
        self.0.definitions()
    }

    /// Materialize authoring facts for source specialization. The mutable result
    /// carries no executable seal and must pass MIR verification after lowering.
    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<AnalyzedModule, DefinitionMappingError> {
        self.0.to_paths(cancel)
    }

    pub(crate) fn adopt(
        module: &AnalyzedModule,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        let context = module.declarations.context();
        let mut records = module.map_identities(&mut DefinitionMapper::new(
            &mut |path| context.intern(path).map_err(Into::into),
            cancel,
        ))?;
        let definitions = context.snapshot();
        records
            .declarations
            .publish_definitions(definitions.clone());
        Ok(Self(DefinitionMetadata::checked(
            definitions,
            records,
            cancel,
        )?))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedAnalysis<I: DefinitionReference = DefinitionPath> {
    signatures_reused: bool,
    // The suffix after this boundary depends on the complete aggregate catalog.
    // Recompute it after all declaration signatures exist, including on reuse.
    local_signature_diagnostics: usize,
    lowered: Arc<lower::LoweredModule>,
    names: AnalysisResult<DeclarationNames>,
    declarations: declarations::Declarations<I>,
    signatures: Arc<AnalysisResult<typeck::ModuleSignatures<I>>>,
}

impl PreparedAnalysis {
    fn completed_signatures(
        &self,
        aggregates: &aggregates::AggregateCatalog,
        cancel: &CancellationToken,
    ) -> Result<Option<Arc<AnalysisResult<typeck::ModuleSignatures>>>, Cancelled> {
        let mut diagnostics = DiagnosticBuffer::new();
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
        diagnostics.extend(self.declarations.hosts.validate_trait_implementations(
            aggregates,
            self.lowered.source.module_identity(),
            cancel,
        )?);
        for implementation in aggregates.implementations() {
            cancel.check()?;
            if implementation.id.module == *self.lowered.source.module_identity()
                && self
                    .declarations
                    .hosts
                    .implements(&implementation.trait_type, &implementation.for_type)
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                        trait_name: implementation
                            .trait_type
                            .declaration
                            .path
                            .last()
                            .map(|segment| segment.name.clone())
                            .unwrap_or_default(),
                        type_name: implementation.for_type.display_name(),
                        reason: "host and script implementations overlap".into(),
                    })
                    .with_span(Span::default()),
                );
            }
        }
        for (first, second) in aggregates.overlapping_implementations() {
            cancel.check()?;
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: first
                        .trait_type
                        .declaration
                        .path
                        .last()
                        .map(|segment| segment.name.clone())
                        .unwrap_or_default(),
                    type_name: first.for_type.display_name(),
                    reason: format!(
                        "overlapping implementations in {} and {}",
                        first.id.module, second.id.module
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

#[derive(Debug, Clone)]
pub(crate) struct DeclaredAnalysis<I: DefinitionReference = DefinitionPath> {
    lowered: Arc<lower::LoweredModule>,
    names: AnalysisResult<DeclarationNames>,
    declarations: declarations::Declarations<I>,
}

impl DeclaredAnalysis {
    fn check_signatures(
        mut self,
        imported_types: ImportedTypes,
        previous_analysis: Option<&PreparedAnalysis>,
        cancel: &CancellationToken,
    ) -> PreparedAnalysis {
        self.declarations.imported_types = imported_types;
        let previous = previous_analysis.and_then(|old| {
            if !(old.names.facts.hosts.revision() == self.names.facts.hosts.revision()
                && old
                    .names
                    .facts
                    .imports
                    .same_bindings(&self.names.facts.imports)
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
        PreparedAnalysis {
            signatures_reused,
            local_signature_diagnostics: reused_local_diagnostics
                .unwrap_or(signatures.diagnostics().len()),
            lowered: self.lowered,
            names: self.names,
            declarations: self.declarations,
            signatures,
        }
    }
}

fn declare_analysis(
    lowered: Arc<lower::LoweredModule>,
    hosts: Arc<host::HostDeclarations>,
    imports: Arc<imports::ModuleImports>,
    definitions: &DefinitionContext,
    cancel: &CancellationToken,
) -> DeclaredAnalysis {
    let mut names = collect_declarations(&lowered, hosts, imports, cancel);
    if !lowered.source.module_identity().within_path_limit() {
        names
            .diagnostics
            .push(Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                resource: "module identity path segments",
                limit: MAX_IDENTITY_PATH_SEGMENTS,
            }));
    }
    let declarations =
        Declarations::collect_named(&lowered.source, &lowered, &names.facts, definitions, cancel);
    DeclaredAnalysis {
        lowered,
        names,
        declarations,
    }
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

pub fn analyze_source(source: &SourceFile) -> AnalysisResult<AnalyzedModule> {
    let snapshot = AnalysisDatabase::default()
        .snapshot(
            SourceSnapshot::single_file(Arc::new(source.clone())),
            &Default::default(),
        )
        .expect("language declarations must prepare for uncancelled analysis");
    snapshot
        .file(source.id())
        .expect("requested source belongs to its snapshot")
        .result()
        .clone()
}

pub(crate) struct AnalysisPolicy {
    const_limits: ConstLimits,
    max_semantic_diagnostics: usize,
}

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
            "meta" => continue,
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
