//! AST-to-HIR construction, source-site retention and uninterpreted attributes.
//!
//! [`lower_module`] is the standalone convenience entrypoint. Analysis uses the
//! already parsed AST through `lower_module_controlled`. The internal `Lowerer`
//! coordinates slot allocation and record origins. Category modules implement the
//! syntax transformations. No import lookup or type inference happens here.

use crate::{hir::item::Module, lower::context::Lowerer};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source::SourceFile;
use kagari_syntax::{
    ast::module::ModuleSyntax,
    parser::{ParseOptions, parse},
};
use std::sync::Arc;

mod attribute;
pub(crate) mod context;
mod expr;
mod identity;
mod item;
mod stmt;
mod ty;

/// A source unit's owned HIR and matching optional source origins.
///
/// ```text
/// LoweredModule
/// +-- source: Arc<SourceFile>       // retained text + identity/revision
/// +-- module: Module               // declaration vectors + shared HirNodes
/// +-- input_kind: ModuleInputKind  // source or structured registration
/// ```
///
/// Source and structured inputs create ordinary declaration records.
/// Lowering preserves incomplete shapes and unresolved
/// names: this value is neither typed nor a code-generation authorization.
#[derive(Debug, Clone)]
pub struct LoweredModule {
    /// Retained source or a source-less registration's logical graph key.
    /// Use `source_surface()` before treating this value as authored text.
    pub source: Arc<SourceFile>,
    /// Owned declaration collections and node storage.
    pub module: Module,
    pub(crate) input_kind: ModuleInputKind,
}

/// Determines whether the retained source is authored or a structured registration key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleInputKind {
    Source,
    Structured,
}

impl LoweredModule {
    /// Authored source when available. A structured registration retains `source`
    /// only as a logical graph key; it is not a navigation or implementation surface.
    pub fn source_surface(&self) -> Option<&Arc<SourceFile>> {
        (self.input_kind == ModuleInputKind::Source).then_some(&self.source)
    }
}

/// Parses and lowers a source file using a fresh lowering identity and default cancellation.
///
/// This convenience entrypoint returns structural HIR, not parse diagnostics or
/// checked analysis. Use [`crate::analysis::AnalysisDatabase`] for the diagnostic
/// and semantic pipeline. Missing syntax can produce placeholder nodes.
///
/// # Examples
///
/// ```
/// use kagari_hir::{hir::expr::ExprKind, lower::lower_module};
/// use kagari_source::source::SourceFile;
///
/// let source = SourceFile::new("add.kg", "fn add(x: i32) -> i32 { x + 1 }");
/// let lowered = lower_module(&source);
/// let function = &lowered.module.functions[0];
/// let block = lowered.module.block(function.implementation.script_body().unwrap());
/// let sum = block.tail_expr.unwrap();
/// let ExprKind::Binary { lhs, rhs, .. } = &lowered.module.expr(sum).kind else {
///     panic!("expected the addition node");
/// };
/// assert!(matches!(lowered.module.expr(*lhs).kind, ExprKind::Name { .. }));
/// assert!(matches!(lowered.module.expr(*rhs).kind, ExprKind::Literal(_)));
/// let span = lowered.module.expr(sum).origin.diagnostic_span();
/// // Semantic ranges include written tokens, excluding outer whitespace and comments.
/// assert_eq!(&source.text()[span.start..span.end], "x + 1");
/// ```
/// # Panics
///
/// Panics if a manually supplied inline source extent does not identify a module
/// in its text. AnalysisDatabase validates this association with a recoverable error.
pub fn lower_module(source: &SourceFile) -> LoweredModule {
    let parsed = parse(source, ParseOptions::default(), &Default::default())
        .expect("fresh cancellation token");
    let syntax = match source.inline_range() {
        Some(range) => ModuleSyntax::at_content_range(&parsed, range)
            .expect("inline source extent must identify its syntax body"),
        None => parsed.module(),
    };
    lower_module_controlled(Arc::new(source.clone()), &syntax, &Default::default())
}

/// Lowers a logical AST view and its matching source into fresh owned, origin-bearing HIR.
///
/// The caller must pair the view with its retained source revision and keep parse
/// diagnostics separately. Cancellation may leave partial structural HIR; callers
/// must check the shared token before accepting or publishing the returned module.
pub(crate) fn lower_module_controlled(
    source: Arc<SourceFile>,
    module: &ModuleSyntax,
    cancel: &CancellationToken,
) -> LoweredModule {
    let mut lowerer = Lowerer::new(cancel.clone());
    lowerer.lower_module(module);
    LoweredModule::from_parts(source, lowerer.finish(), ModuleInputKind::Source)
}

impl LoweredModule {
    pub(crate) fn from_parts(
        source: Arc<SourceFile>,
        module: Module,
        input_kind: ModuleInputKind,
    ) -> Self {
        Self {
            source,
            module,
            input_kind,
        }
    }
}
