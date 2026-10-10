//! Child module headers and flattened import syntax, before graph construction.

use crate::hir::ids::ModuleId;
use kagari_common::span::Span;
use kagari_types::visibility::Visibility;

/// A child module header stored in its parent's Module.modules, not the child HIR.
///
/// ```text
/// pub mod nested { fn value() -> i32 { 1 } }
/// ModuleDecl { id: m, visibility: Public, name: "nested", inline: true }
/// m -> parent.modules[m.index()]; matching SourceMap module span
/// analysis preparation -> separate child SourceUnit and LoweredModule for value
/// ```
///
/// `id` is parent-local allocation metadata; `pub`, the identifier and braces
/// determine visibility, name and `inline`. `mod nested;` gives Private visibility
/// and `inline: false`, requiring external source discovery. Even for inline
/// syntax the function is not embedded in this record or the parent's function
/// collection. Analysis owns child-source preparation and physical-offset routing.
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

/// One flattened use-tree leaf, stored inline in Module.imports before resolution.
///
/// ```text
/// pub use pkg::math::{sum as add, value, ops::*};
/// imports = [
///     Import { visibility: Public, path: "pkg::math::sum",
///              kind: Named { alias: Some("add") }, root_span: root_tree, span: sum_leaf },
///     Import { visibility: Public, path: "pkg::math::value",
///              kind: Named { alias: None }, root_span: root_tree, span: value_leaf },
///     Import { visibility: Public, path: "pkg::math::ops",
///              kind: Glob, root_span: root_tree, span: glob_leaf },
/// ]
/// local_name() -> Some("add"), Some("value"), None respectively
/// ```
///
/// `path` is the accumulated spelling after flattening, not a resolved declaration.
/// `span` is the terminal use-tree leaf token range (including its alias when
/// present); `root_span` is the outer use-tree token range, here
/// `pkg::math::{sum as add, value, ops::*}`, excluding `pub use` and the semicolon.
/// SourceMap separately retains path-prefix sites by import slot.
/// Without `pub`, visibility is Private. Import analysis builds directives and
/// Type/Value bindings from these leaves; a glob is not expanded into more HIR
/// Import records, and one named leaf may resolve in both namespaces.
#[derive(Debug, Clone)]
pub struct Import {
    /// Named/aliased leaf or glob; see the source-to-field examples above.
    pub kind: ImportLeaf,
    /// Outer use-tree token range shared by all its flattened leaves.
    pub root_span: Span,
    /// Written use visibility, retained for later import/re-export access checks.
    pub visibility: Visibility,
    /// Accumulated path spelling; a glob path excludes the final `::*`.
    pub path: String,
    /// This terminal use-tree leaf token range, including any alias syntax.
    pub span: Span,
}

/// The terminal form of a flattened Import, independent of path and visibility.
///
/// | Source | Kind | Import.local_name() |
/// | --- | --- | --- |
/// | `use pkg::sum;` | `Named { alias: None }` | `Some("sum")` |
/// | `use pkg::sum as add;` | `Named { alias: Some("add") }` | `Some("add")` |
/// | `use pkg::*;` | `Glob` | `None` |
///
/// Named imports retain an optional written alias, not a selected semantic target.
/// Glob expansion and visibility filtering belong to the namespace catalog.
#[derive(Debug, Clone)]
pub enum ImportLeaf {
    /// One selected name; `alias` is the written `as` name, absent when omitted.
    Named {
        /// Explicit local alias; the path terminal supplies the name when absent.
        alias: Option<String>,
    },
    /// `*` imports eligible namespace members; no single local name is stored.
    Glob,
}

impl Import {
    /// Returns the written alias/path terminal for a named leaf, or None for a glob.
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
