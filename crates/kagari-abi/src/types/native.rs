//! Native representation constructors carried by public type contracts.
//! These are runtime shape facts, not a source declaration or name-resolution table.

use crate::{standard::surface::StandardEnum, types::TypeAbi};
use kagari_common::{identity::DefinitionKind, range::RangeKind};
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

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeTypeConstructor {
    String,
    Array,
    Map,
    Set,
    Iter,
    Range(RangeKind),
    Enum(StandardEnum),
}

impl NativeTypeConstructor {
    pub fn arity(self) -> usize {
        match self {
            Self::String | Self::Range(RangeKind::Full) => 0,
            Self::Map => 2,
            Self::Array | Self::Set | Self::Iter | Self::Range(_) => 1,
            Self::Enum(kind) => kind.arity(),
        }
    }

    pub fn declaration_kind(self) -> DefinitionKind {
        match self {
            Self::Enum(_) => DefinitionKind::Enum,
            _ => DefinitionKind::AssociatedType,
        }
    }

    pub(crate) fn shape_valid(self, declaration: &TypeAbi) -> bool {
        if !declaration.fields.is_empty() || declaration.generic_params.len() != self.arity() {
            return false;
        }
        let Self::Enum(kind) = self else {
            return declaration.variants.is_empty();
        };
        kind.variants().len() == declaration.variants.len()
            && kind
                .variants()
                .iter()
                .zip(&declaration.variants)
                .all(|(expected, actual)| match expected.payload() {
                    None => actual.payload.is_empty(),
                    Some(slot) => {
                        actual.payload.as_slice() == [declaration.generic_params[slot].as_type()]
                    }
                })
    }
}
