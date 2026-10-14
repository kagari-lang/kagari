//! Explicit selection of a public path or the separately verified root entry.

use crate::module::LoadedModule;
use kagari_contract::ids::FunctionRef;

/// Select a callable from the installed Program's verified export evidence.
///
/// Strings select public paths: `"damage"` resolves a public re-export and
/// `"main"` requires a public export with that name. `Main` instead selects the
/// root's zero-argument script entry, which may be private. Selection never scans
/// implementation names or changes its root when passed a dependency handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramEntry<'a> {
    Export(&'a str),
    Main,
}

impl<'a> From<&'a str> for ProgramEntry<'a> {
    fn from(path: &'a str) -> Self {
        Self::Export(path)
    }
}

impl<'a> ProgramEntry<'a> {
    /// Return the requested public path or `main` for diagnostics and call reports.
    pub fn label(self) -> &'a str {
        match self {
            Self::Export(path) => path,
            Self::Main => "main",
        }
    }

    /// Resolve verified slots in the exact supplying Program generation.
    /// Missing/private public paths or an absent root entry return `None`.
    /// The execution boundary must still validate runtime ownership and retention.
    pub fn resolve(self, owner: &LoadedModule) -> Option<(LoadedModule, FunctionRef)> {
        match self {
            Self::Export(path) => owner.exported_function(path),
            Self::Main => owner.entry_function(),
        }
    }
}
