pub mod aggregates;
pub mod analysis;
pub mod builtin;
pub mod declarations;
pub mod hir;
pub mod host;
pub mod imports;
pub mod lower;
pub mod profile;
pub mod program;
pub mod resolver;
pub mod source_map;
pub mod typeck;
pub mod types;

use kagari_common::Diagnostic;
use std::ops::Deref;

pub use profile::LanguageFeatureProfile;

pub type DiagnosticBuffer = smallvec::SmallVec<[Diagnostic; 4]>;
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

#[derive(Debug, Clone)]
pub struct AnalyzedModule {
    pub aggregates: aggregates::AggregateCatalog,
    pub lowered: std::sync::Arc<lower::LoweredModule>,
    pub names: resolver::ResolvedNames,
    pub declarations: declarations::Declarations,
    pub typed: typeck::TypedModule,
    pub signatures: std::sync::Arc<AnalysisResult<typeck::ModuleSignatures>>,
    pub imported_functions: imports::ImportedFunctions,
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
            .any(|d| d.severity == kagari_common::Severity::Error)
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
pub struct CheckedAnalysis(AnalyzedModule);

impl Deref for CheckedAnalysis {
    type Target = AnalyzedModule;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AnalysisResult<AnalyzedModule> {
    pub fn into_codegen(self) -> Result<CheckedAnalysis, BoxedDiagnosticBuffer> {
        self.into_checked().map(CheckedAnalysis)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedAnalysis {
    signatures_reused: bool,
    // The suffix after this boundary depends on the complete aggregate catalog.
    // Recompute it after all declaration signatures exist, including on reuse.
    local_signature_diagnostics: usize,
    lowered: std::sync::Arc<lower::LoweredModule>,
    names: AnalysisResult<resolver::DeclarationNames>,
    declarations: declarations::Declarations,
    signatures: std::sync::Arc<AnalysisResult<typeck::ModuleSignatures>>,
}

impl PreparedAnalysis {
    fn completed_signatures(
        &self,
        aggregates: &aggregates::AggregateCatalog,
        cancel: &kagari_common::cancellation::CancellationToken,
    ) -> Result<
        Option<std::sync::Arc<AnalysisResult<typeck::ModuleSignatures>>>,
        kagari_common::cancellation::Cancelled,
    > {
        let mut diagnostics = DiagnosticBuffer::new();
        typeck::validate_signature_applications(
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
                    Diagnostic::error(kagari_common::DiagnosticKind::InvalidTraitImpl {
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
                    .with_span(kagari_common::Span::default()),
                );
            }
        }
        for (first, second) in aggregates.overlapping_implementations() {
            cancel.check()?;
            diagnostics.push(
                Diagnostic::error(kagari_common::DiagnosticKind::InvalidTraitImpl {
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
                .with_span(kagari_common::Span::default()),
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
        Ok(Some(std::sync::Arc::new(result)))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DeclaredAnalysis {
    lowered: std::sync::Arc<lower::LoweredModule>,
    names: AnalysisResult<resolver::DeclarationNames>,
    declarations: declarations::Declarations,
}

impl DeclaredAnalysis {
    fn check_signatures(
        mut self,
        imported_types: imports::ImportedTypes,
        previous_analysis: Option<&PreparedAnalysis>,
        cancel: &kagari_common::cancellation::CancellationToken,
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
                typeck::reuse_signatures(&old.lowered, &old.signatures, &self.lowered, cancel)
                    .map(std::sync::Arc::new)
            }
        });
        let signatures_reused = previous.is_some();
        let reused_local_diagnostics = previous.as_ref().map(|_| {
            previous_analysis
                .expect("signature reuse source")
                .local_signature_diagnostics
        });
        let signatures = previous.unwrap_or_else(|| {
            std::sync::Arc::new(typeck::check_signatures(
                &self.lowered,
                &self.declarations,
                cancel,
            ))
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
    lowered: std::sync::Arc<lower::LoweredModule>,
    hosts: std::sync::Arc<host::HostDeclarations>,
    imports: std::sync::Arc<imports::ModuleImports>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> DeclaredAnalysis {
    let names = resolver::collect_declarations(&lowered, hosts, imports, cancel);
    let declarations =
        declarations::Declarations::collect_named(&lowered.source, &lowered, &names.facts, cancel);
    DeclaredAnalysis {
        lowered,
        names,
        declarations,
    }
}

fn analyze_prepared(
    prepared: PreparedAnalysis,
    const_limits: typeck::ConstLimits,
    imported_functions: imports::ImportedFunctions,
    aggregates: aggregates::AggregateCatalog,
    reuse: Option<&typeck::BodyReuse<'_>>,
    cancel: &kagari_common::cancellation::CancellationToken,
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
        facts: resolver::resolve_bodies(&lowered, &names.facts, hir::BodySelection::All, cancel),
        diagnostics: names.diagnostics,
    };
    let declarations = declarations.with_bindings(&lowered, &names.facts, cancel);
    let typed = typeck::check_bodies_controlled(
        &lowered,
        &names.facts,
        &declarations,
        typeck::BodyInputs {
            const_limits,
            selection: hir::BodySelection::All,
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

pub fn analyze_source(
    source: &kagari_common::SourceFile,
    profile: LanguageFeatureProfile,
) -> AnalysisResult<AnalyzedModule> {
    let parsed = kagari_syntax::parse(source);
    let lowered = lower::lower_module_controlled(
        std::sync::Arc::new(source.clone()),
        &parsed.syntax(),
        &Default::default(),
    );
    let hosts = host::HostDeclarations::empty();
    let graph = imports::ModuleGraph::build([&lowered], &hosts, &Default::default())
        .expect("uncancelled source analysis");
    let imports = graph
        .node(source.module_identity())
        .unwrap()
        .imports
        .clone();
    let declared = declare_analysis(
        std::sync::Arc::new(lowered),
        hosts,
        imports,
        &Default::default(),
    );
    let imported_types = imports::TypeCatalog::new([&declared])
        .bindings(&declared.names.facts.imports, &Default::default())
        .expect("uncancelled source analysis");
    let mut prepared = declared.check_signatures(imported_types, None, &Default::default());
    let imported_functions = imports::FunctionCatalog::new([&prepared])
        .bindings(&prepared.names.facts.imports, &Default::default())
        .expect("uncancelled source analysis");
    let mut aggregates = aggregates::AggregateCatalog::default();
    aggregates
        .add_module(
            &prepared.lowered,
            &prepared.declarations,
            prepared.signatures.facts(),
            &Default::default(),
        )
        .expect("uncancelled source analysis");
    if let Some(signatures) = prepared
        .completed_signatures(&aggregates, &Default::default())
        .expect("uncancelled source analysis")
    {
        prepared.signatures = signatures;
    }
    analyze_parsed(
        prepared,
        &parsed,
        AnalysisPolicy {
            profile,
            const_limits: Default::default(),
            max_semantic_diagnostics: 1_000,
        },
        imported_functions,
        aggregates,
        None,
        &Default::default(),
    )
}

pub(crate) struct AnalysisPolicy {
    profile: LanguageFeatureProfile,
    const_limits: typeck::ConstLimits,
    max_semantic_diagnostics: usize,
}

pub(crate) fn analyze_parsed(
    prepared: PreparedAnalysis,
    parsed: &kagari_syntax::Parse,
    policy: AnalysisPolicy,
    imported_functions: imports::ImportedFunctions,
    aggregates: aggregates::AggregateCatalog,
    reuse: Option<&typeck::BodyReuse<'_>>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<AnalyzedModule> {
    let mut analyzed = analyze_prepared(
        prepared,
        policy.const_limits,
        imported_functions,
        aggregates,
        reuse,
        cancel,
    );
    if let Err(diagnostics) = profile::validate_profile(&analyzed.facts, policy.profile) {
        analyzed.diagnostics.extend(*diagnostics);
    }
    for attribute in &analyzed.facts.lowered.attributes {
        let kind = match attribute.name.as_str() {
            "meta" => continue,
            "reflect" | "requires" | "profile" => {
                kagari_common::DiagnosticKind::UnsupportedAttribute {
                    name: attribute.name.clone(),
                }
            }
            name if name.starts_with("tool::") => continue,
            _ => kagari_common::DiagnosticKind::UnknownAttribute {
                name: attribute.name.clone(),
            },
        };
        analyzed
            .diagnostics
            .push(kagari_common::Diagnostic::error(kind).with_span(attribute.span));
    }
    if analyzed.diagnostics.len() > policy.max_semantic_diagnostics {
        analyzed
            .diagnostics
            .truncate(policy.max_semantic_diagnostics);
        analyzed.diagnostics.push(kagari_common::Diagnostic::error(
            kagari_common::DiagnosticKind::CompileLimitExceeded {
                resource: "semantic diagnostics",
                limit: policy.max_semantic_diagnostics,
            },
        ));
    }
    analyzed
        .diagnostics
        .extend(parsed.diagnostics().iter().cloned());
    analyzed
}

#[cfg(test)]
mod tests;
