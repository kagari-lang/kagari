//! Child module headers and flattened import syntax, before graph construction.

use crate::hir::ids::ModuleId;
use kagari_common::span::Span;
use kagari_types::visibility::Visibility;

/// A child module header, not a recursively stored module body.
///
/// ```text
/// mod nested { pub fn value() -> i32 { 1 } }
/// parent Module.modules[id.index()] -> ModuleDecl { name: "nested", inline: true, ... }
/// analysis preparation -> separate child SourceUnit + LoweredModule
/// ```
///
/// `mod nested;` instead has `inline: false`. Module discovery and inline-source
/// preparation belong to [`crate::analysis`], not this record.
#[derive(Debug, Clone)]
pub struct ModuleDecl {
    /// Parent-local slot in `Module.modules`.
    pub id: ModuleId,
    /// Visibility declared on the child module header.
    pub visibility: Visibility,
    /// Child module segment as written.
    pub name: String,
    /// Whether the declaration has a braced inline body.
    pub inline: bool,
}

/// One flattened source use-tree leaf before resolution.
///
/// ```text
/// use hir::expr::{Condition, ExprKind as Kind};
/// Module.imports
/// +-- slot 0: path="hir::expr::Condition", kind=Named { alias: None }
/// `-- slot 1: path="hir::expr::ExprKind", kind=Named { alias: Some("Kind") }
/// ```
///
/// Syntax remains one leaf even when resolution produces both type and value bindings.
#[derive(Debug, Clone)]
pub struct Import {
    pub kind: ImportLeaf,
    pub root_span: Span,
    pub visibility: Visibility,
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ImportLeaf {
    Named { alias: Option<String> },
    Glob,
}

impl Import {
    pub fn local_name(&self) -> Option<&str> {
        match &self.kind {
            ImportLeaf::Named { alias } => {
                alias.as_deref().or_else(|| self.path.rsplit("::").next())
            }
            ImportLeaf::Glob => None,
        }
    }
}

/// Ordered `Vec<ModuleDecl>` storage for the owning declaration records.
pub type ModuleDeclBuffer = Vec<ModuleDecl>;
/// Ordered `Vec<Import>` storage for the owning declaration records.
pub type ImportBuffer = Vec<Import>;
