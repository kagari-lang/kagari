//! Portable native constructor families and declared storage capabilities.
//! Physical slot representation and installed Rust storage belong to execution.

use crate::declaration::TypeDef;
use crate::range::RangeKind;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
    table::{DefinitionId, DefinitionTable},
};
use serde::{Deserialize, Serialize};

/// Portable storage capabilities contain parameter positions, never Rust types,
/// function pointers or container names interpreted by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeStorageLayout {
    Opaque,

    /// One invariant completed-output parameter; only the runtime supplies storage.
    Future,
    /// Cached shared output, independent of the retired execution frames.
    Task,
    /// Runtime-local admission authority; only the host creates scopes.
    TaskScope,
}

impl NativeStorageLayout {
    pub fn valid_parameters(self, arity: usize) -> bool {
        match self {
            Self::Opaque => true,
            Self::Future | Self::Task => arity == 1,
            Self::TaskScope => arity == 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NativeTypeConstructor {
    String,

    Range(RangeKind),
    // The retired reference constructor occupied tag 2. Leave it unassigned.
}

/// Tests exact language ownership, including declaration kind and occurrence.
pub fn language_type(
    id: DefinitionId,
    definitions: &DefinitionTable,
    package: &str,
    module: &str,
    name: &str,
) -> bool {
    definitions.resolve(id).is_ok_and(|view| {
        view.module().package.0 == package
            && view.module().path == [module]
            && view.segments().count() == 1
            && view.last().is_some_and(|part| {
                part.kind == DefinitionKind::AssociatedType
                    && part.name == name
                    && part.occurrence == 0
            })
    })
}

impl NativeTypeConstructor {
    /// The source spelling behind a portable String or range in a registered signature.
    pub fn declaration(self) -> DefinitionPath {
        let (package, module, name) = match self {
            Self::String => ("kagari-alloc", "string", "String"),
            Self::Range(kind) => ("kagari-core", "ops", kind.name()),
        };
        DefinitionPath {
            module: ModuleIdentity {
                package: PackageId(package.into()),
                path: vec![module.into()],
            },
            path: vec![DefinitionPathSegment {
                kind: self.declaration_kind(),
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    /// Language constructors have fixed declaration owners, independent of registration.
    pub fn from_definition(id: DefinitionId, definitions: &DefinitionTable) -> Option<Self> {
        if language_type(id, definitions, "kagari-alloc", "string", "String") {
            return Some(Self::String);
        }
        for kind in RangeKind::ALL {
            if language_type(id, definitions, "kagari-core", "ops", kind.name()) {
                return Some(Self::Range(kind));
            }
        }
        None
    }

    pub fn arity(self) -> usize {
        match self {
            Self::String | Self::Range(RangeKind::Full) => 0,

            Self::Range(_) => 1,
        }
    }

    pub fn declaration_kind(self) -> DefinitionKind {
        DefinitionKind::AssociatedType
    }

    pub fn shape_valid(self, declaration: &TypeDef) -> bool {
        declaration.fields.is_empty()
            && declaration.variants.is_empty()
            && declaration.generic_params.len() == self.arity()
    }
}
