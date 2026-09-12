pub mod analysis;
pub mod builtin;
pub mod declarations;
pub mod hir;
pub mod lower;
pub mod profile;
pub mod resolver;
pub mod source_map;
pub mod typeck;
pub mod types;

use kagari_common::Diagnostic;
use kagari_syntax::ast;
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

fn analyze_syntax(
    source: std::sync::Arc<kagari_common::SourceFile>,
    module: &ast::SourceFile,
    reuse: Option<&typeck::BodyReuse<'_>>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<AnalyzedModule> {
    let lowered = lower::lower_module_controlled(source.clone(), module, cancel);
    let names = resolver::resolve_names_controlled(&lowered, cancel);
    let declarations = declarations::Declarations::collect(&source, &lowered, &names.facts, cancel);
    let typed =
        typeck::check_module_controlled(&lowered, &names.facts, &declarations, reuse, cancel);
    let mut diagnostics = names.diagnostics;
    diagnostics.extend(typed.diagnostics);
    AnalysisResult {
        facts: AnalyzedModule {
            lowered,
            names: names.facts,
            declarations,
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
    analyze_parsed(
        std::sync::Arc::new(source.clone()),
        &parsed,
        profile,
        None,
        &Default::default(),
    )
}

pub(crate) fn analyze_parsed(
    source: std::sync::Arc<kagari_common::SourceFile>,
    parsed: &kagari_syntax::Parse,
    profile: LanguageFeatureProfile,
    reuse: Option<&typeck::BodyReuse<'_>>,
    cancel: &kagari_common::cancellation::CancellationToken,
) -> AnalysisResult<AnalyzedModule> {
    let mut analyzed = analyze_syntax(source, &parsed.syntax(), reuse, cancel);
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
