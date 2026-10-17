//! Source binding categories, independent of canonical definition identity.
use kagari_common::identity::DefinitionKind;

/// Independent lookup and conflict domains for source declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameNamespace {
    Type,
    Value,
}

impl NameNamespace {
    pub const ALL: [Self; 2] = [Self::Type, Self::Value];

    /// Classifies declarations that may appear in portable module exports.
    pub fn of_definition(kind: DefinitionKind) -> Option<Self> {
        match kind {
            DefinitionKind::Struct
            | DefinitionKind::Enum
            | DefinitionKind::Trait
            | DefinitionKind::AssociatedType => Some(Self::Type),
            DefinitionKind::Function | DefinitionKind::Const | DefinitionKind::Variant => {
                Some(Self::Value)
            }
            _ => None,
        }
    }
}

/// An authored export alias; the target retains its canonical definition path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExportName {
    pub namespace: NameNamespace,
    pub name: String,
}

impl ExportName {
    pub fn new(namespace: NameNamespace, name: impl Into<String>) -> Self {
        Self {
            namespace,
            name: name.into(),
        }
    }
}
