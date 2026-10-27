//! Portable native constructor families and declared storage capabilities.
//! Physical slot representation and installed Rust storage belong to execution.

use crate::declaration::TypeDef;
use crate::range::RangeKind;
use crate::reference::ReferenceAccess;
use kagari_common::identity::DefinitionKind;
use serde::{Deserialize, Serialize};

/// Portable storage capabilities contain parameter positions, never Rust types,
/// function pointers or container names interpreted by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeStorageLayout {
    Opaque,
    Iterator {
        item: usize,
    },
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
            Self::Iterator { item } => item < arity,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NativeTypeConstructor {
    String,
    Iter,
    Range(RangeKind),
    Reference(ReferenceAccess),
}

impl NativeTypeConstructor {
    pub fn arity(self) -> usize {
        match self {
            Self::String | Self::Range(RangeKind::Full) => 0,

            Self::Iter | Self::Range(_) | Self::Reference(_) => 1,
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
