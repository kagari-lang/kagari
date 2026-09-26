//! Source/semantic identity is distinct from runtime slots and display spelling.
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);
pub const MAX_IDENTITY_PATH_SEGMENTS: usize = 64;

fn module_path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    crate::decode_limits::bounded_vec(
        deserializer,
        MAX_IDENTITY_PATH_SEGMENTS,
        "module identity path segment",
    )
}

fn definition_path<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<DefinitionPathSegment>, D::Error> {
    crate::decode_limits::bounded_vec(
        deserializer,
        MAX_IDENTITY_PATH_SEGMENTS,
        "definition identity path segment",
    )
}

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
    #[serde(deserialize_with = "module_path")]
    pub path: Vec<String>,
}

impl ModuleIdentity {
    pub fn within_path_limit(&self) -> bool {
        self.path.len() <= MAX_IDENTITY_PATH_SEGMENTS
    }

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
    #[serde(deserialize_with = "definition_path")]
    pub path: Vec<DefinitionPathSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DefinitionKind {
    Function,
    Const,
    Module,
    Struct,
    Field,
    Enum,
    Trait,
    Impl,
    Method,
    Variant,
    AssociatedType,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DefinitionPathSegment {
    pub kind: DefinitionKind,
    pub name: String,
    /// Source-order occurrence among declarations with the same parent, kind and name.
    /// Usually zero; also distinguishes malformed duplicates and unnamed impl blocks.
    pub occurrence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSpan {
    pub file: FileId,
    pub revision: Revision,
    pub range: crate::Span,
}

impl DefinitionId {
    pub fn within_path_limit(&self) -> bool {
        self.module.within_path_limit() && self.path.len() <= MAX_IDENTITY_PATH_SEGMENTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::Options;

    fn codec() -> impl Options {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
    }

    #[test]
    fn identity_path_lengths_are_checked_before_decoding_segments() {
        let module = ModuleIdentity::single_file("main.kgr");
        let mut bytes = codec().serialize(&module).unwrap();
        let count_offset = codec().serialized_size(&module.package).unwrap() as usize;
        bytes[count_offset..count_offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let error = codec().deserialize::<ModuleIdentity>(&bytes).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("module identity path segment count limit exceeded")
        );

        let id = DefinitionId {
            module,
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Function,
                name: "main".into(),
                occurrence: 0,
            }],
        };
        let mut bytes = codec().serialize(&id).unwrap();
        let count_offset = codec().serialized_size(&id.module).unwrap() as usize;
        bytes[count_offset..count_offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let error = codec().deserialize::<DefinitionId>(&bytes).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("definition identity path segment count limit exceeded")
        );
    }
}
