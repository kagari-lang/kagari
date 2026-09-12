pub mod analysis;
pub mod builtin;
pub mod declarations;
pub mod hir;
pub mod host;
pub mod imports;
pub mod lower;
pub mod profile;
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
    pub lowered: lower::LoweredModule,
    pub names: resolver::ResolvedNames,
    pub declarations: declarations::Declarations,
    pub typed: typeck::TypedModule,
    pub signatures: std::sync::Arc<AnalysisResult<typeck::ModuleSignatures>>,
    pub imported_functions: imports::ImportedFunctions,
    name_diagnostics: DiagnosticBuffer,
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
    pub fn into_codegen(mut self) -> Result<CheckedAnalysis, BoxedDiagnosticBuffer> {
        for import in &self.facts.names.imports.entries {
            if let Some(imports::ImportTarget::Source(target)) = &import.target {
                self.diagnostics.push(
                    kagari_common::Diagnostic::error(
                        kagari_common::DiagnosticKind::ModuleLinkRequired {
                            module: target.module.to_string(),
                        },
                    )
                    .with_span(import.span),
                );
            }
        }
        self.into_checked().map(CheckedAnalysis)
    }
}

pub(crate) struct PreparedAnalysis {
    cached: bool,
    lowered: lower::LoweredModule,
    names: AnalysisResult<resolver::ResolvedNames>,
    declarations: declarations::Declarations,
    signatures: std::sync::Arc<AnalysisResult<typeck::ModuleSignatures>>,
}

impl PreparedAnalysis {
    fn from_cached(facts: &AnalyzedModule) -> Self {
        Self {
            cached: true,
            lowered: facts.lowered.clone(),
            names: AnalysisResult {
                facts: facts.names.clone(),
                diagnostics: facts.name_diagnostics.clone(),
            },
            declarations: facts.declarations.clone(),
            signatures: facts.signatures.clone(),
        }
    }
}

fn prepare_analysis(
    lowered: lower::LoweredModule,
    hosts: std::sync::Arc<host::HostDeclarations>,
    imports: std::sync::Arc<imports::ModuleImports>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> PreparedAnalysis {
    let names = resolver::resolve_names_controlled(&lowered, hosts, imports, cancel);
    let declarations =
        declarations::Declarations::collect(&lowered.source, &lowered, &names.facts, cancel);
    let signatures = std::sync::Arc::new(typeck::check_signatures(&lowered, &declarations, cancel));
    PreparedAnalysis {
        cached: false,
        lowered,
        names,
        declarations,
        signatures,
    }
}

fn analyze_prepared(
    prepared: PreparedAnalysis,
    imported_functions: imports::ImportedFunctions,
    reuse: Option<&typeck::BodyReuse<'_>>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<AnalyzedModule> {
    let PreparedAnalysis {
        cached,
        lowered,
        names,
        declarations,
        signatures,
    } = prepared;
    // Signature reuse does not extend the lifetime of body-local binding identities.
    let declarations = if cached {
        declarations::Declarations::collect(&lowered.source, &lowered, &names.facts, cancel)
    } else {
        declarations
    };
    let typed = typeck::check_module_controlled(
        &lowered,
        &names.facts,
        &declarations,
        &signatures,
        &imported_functions,
        reuse,
        cancel,
    );
    let mut diagnostics = names.diagnostics.clone();
    diagnostics.extend(typed.diagnostics);
    AnalysisResult {
        facts: AnalyzedModule {
            lowered,
            names: names.facts,
            declarations,
            signatures,
            imported_functions,
            typed: typed.facts,
            name_diagnostics: names.diagnostics,
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
    let prepared = prepare_analysis(lowered, hosts, imports, &Default::default());
    let imported_functions = imports::FunctionCatalog::new([&prepared])
        .bindings(&prepared.names.facts.imports, &Default::default())
        .expect("uncancelled source analysis");
    analyze_parsed(
        prepared,
        &parsed,
        profile,
        imported_functions,
        None,
        &Default::default(),
    )
}

pub(crate) fn analyze_parsed(
    prepared: PreparedAnalysis,
    parsed: &kagari_syntax::Parse,
    profile: LanguageFeatureProfile,
    imported_functions: imports::ImportedFunctions,
    reuse: Option<&typeck::BodyReuse<'_>>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<AnalyzedModule> {
    let mut analyzed = analyze_prepared(prepared, imported_functions, reuse, cancel);
    if let Err(diagnostics) = profile::validate_profile(&analyzed.facts, profile) {
        analyzed.diagnostics.extend(*diagnostics);
    }
    analyzed
        .diagnostics
        .extend(parsed.diagnostics().iter().cloned());
    analyzed
}

#[cfg(test)]
mod tests;
