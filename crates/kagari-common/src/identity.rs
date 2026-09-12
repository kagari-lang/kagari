//! Source/semantic identity is distinct from runtime slots and display spelling.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FileId(u64);

impl FileId {
    pub(crate) fn fresh() -> Self {
        Self(
            NEXT_FILE
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("source identity exhausted"),
        )
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Revision(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PackageId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModuleIdentity {
    pub package: PackageId,
    pub path: Vec<String>,
}

impl ModuleIdentity {
    /// An unnamed package uses the complete source name as its module path.
    pub fn single_file(source_name: impl Into<String>) -> Self {
        Self {
            package: PackageId("source".into()),
            path: vec![source_name.into()],
        }
    }
}

impl Default for ModuleIdentity {
    fn default() -> Self {
        Self::single_file("<anonymous>")
    }
}

impl std::fmt::Display for ModuleIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.package.0, self.path.join("::"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DefinitionId {
    pub module: ModuleIdentity,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSpan {
    pub file: FileId,
    pub revision: Revision,
    pub range: crate::Span,
}
