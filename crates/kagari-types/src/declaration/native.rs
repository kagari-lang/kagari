//! Portable native constructor families and declared storage capabilities.
//! Physical slot representation and installed Rust storage belong to execution.

use crate::declaration::TypeDef;
use crate::range::RangeKind;
use kagari_common::identity::DefinitionKind;
use serde::{Deserialize, Serialize};

/// Portable storage capabilities contain parameter positions, never Rust types,
/// function pointers or container names interpreted by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeStorageLayout {
    Opaque,
    Sequence { element: usize },
    Map { key: usize, value: usize },
    Set { element: usize },
    Iterator { item: usize },
}

impl NativeStorageLayout {
    pub fn valid_parameters(self, arity: usize) -> bool {
        match self {
            Self::Opaque => true,
            Self::Sequence { element } | Self::Set { element } => element < arity,
            Self::Iterator { item } => item < arity,
            Self::Map { key, value } => key < arity && value < arity && key != value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NativeTypeConstructor {
    String,
    Array,
    Map,
    Set,
    Iter,
    Range(RangeKind),
}

impl NativeTypeConstructor {
    pub fn arity(self) -> usize {
        match self {
            Self::String | Self::Range(RangeKind::Full) => 0,
            Self::Map => 2,
            Self::Array | Self::Set | Self::Iter | Self::Range(_) => 1,
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
