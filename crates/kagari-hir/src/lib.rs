pub mod builtin;
pub mod hir;
pub mod lower;
pub mod profile;
pub mod resolver;
pub mod source_map;
pub mod typeck;
pub mod types;

use kagari_common::Diagnostic;
use kagari_syntax::ast;

pub use profile::LanguageFeatureProfile;

pub type DiagnosticBuffer = smallvec::SmallVec<[Diagnostic; 4]>;
pub type BoxedDiagnosticBuffer = Box<DiagnosticBuffer>;

#[derive(Debug, Clone)]
pub struct AnalyzedModule {
    pub lowered: lower::LoweredModule,
    pub names: resolver::ResolvedNames,
    pub typed: typeck::TypedModule,
}

pub fn analyze_module(module: &ast::SourceFile) -> Result<AnalyzedModule, BoxedDiagnosticBuffer> {
    let lowered = lower::lower_module(module);
    let names = resolver::resolve_names(&lowered)?;
    let typed = typeck::check_module(&lowered, &names)?;
    Ok(AnalyzedModule {
        lowered,
        names,
        typed,
    })
}

pub fn analyze_module_with_profile(
    module: &ast::SourceFile,
    profile: LanguageFeatureProfile,
) -> Result<AnalyzedModule, BoxedDiagnosticBuffer> {
    let analyzed = analyze_module(module)?;
    profile::validate_profile(&analyzed, profile)?;
    Ok(analyzed)
}

pub fn analyze(module: &ast::SourceFile) -> Result<typeck::TypedModule, BoxedDiagnosticBuffer> {
    analyze_module(module).map(|module| module.typed)
}

#[cfg(test)]
mod tests;
