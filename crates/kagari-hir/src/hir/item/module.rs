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
/// +-- slot 0: path="hir::expr::Condition", alias="Condition", alias_explicit=false
/// `-- slot 1: path="hir::expr::ExprKind",  alias="Kind",      alias_explicit=true
/// ```
///
/// The lowered `alias` always contains a spelling; import analysis distinguishes
/// an explicit alias from the default terminal name using `alias_explicit`.
/// For a glob, this spelling is not a new scope binding. The resolved directive,
/// scope candidates and namespace catalog are separate [`crate::imports`] records.
#[derive(Debug, Clone)]
pub struct Import {
    /// Whether the source supplied `as name`, rather than using a derived name.
    pub alias_explicit: bool,
    /// Byte range of the root use tree shared by its flattened leaves.
    pub root_span: Span,
    /// Use visibility, including public re-export intent.
    pub visibility: Visibility,
    /// Explicit or derived terminal spelling; not a binding name for globs.
    pub alias: String,
    /// Joined `::` path accumulated from enclosing use-tree prefixes.
    pub path: String,
    /// Byte range of this leaf, with outer trivia trimmed.
    pub span: Span,
    /// Whether this leaf ends in `*` and imports the target namespace's eligible names.
    pub glob: bool,
}

/// Ordered `Vec<ModuleDecl>` storage for the owning declaration records.
pub type ModuleDeclBuffer = Vec<ModuleDecl>;
/// Ordered `Vec<Import>` storage for the owning declaration records.
pub type ImportBuffer = Vec<Import>;
